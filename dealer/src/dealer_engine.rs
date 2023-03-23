use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

use msgs::api::{
    Api, AvailableCurrenciesResponse, InvoiceResponse, InvoiceResponseError, QuoteResponse, QuoteResponseError,
    SwapRequest, SwapResponse, SwapResponseError,
};
use msgs::dealer::*;
use msgs::kollider_client::*;
use msgs::Message;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::{Add, Sub};
use ws_client::WsClient;
use xerror::dealer::*;

use core_types::{kollider_client::*, *};

use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use std::time::{Duration, Instant, SystemTime};
use utils::currencies::get_base_currency_from_symbol;
use utils::time::time_now;
use utils::xlogging::{init_log, LoggingSettings};
use uuid::Uuid;
use xerror::kollider_client::KolliderClientError;

const QUOTE_TTL_MS: u64 = 5000;

pub struct HedgeSettings {
    // The amount of unhedged value to tolerate before a an adjustment.
    pub max_exposure: Option<u64>,
}

pub struct DealerPnl {
    pub total_pnl: Option<Decimal>,
    pub funding_pnl: Option<Decimal>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DealerEngineSettings {
    pub psql_url: String,
    pub dealer_bank_pull_address: String,
    pub dealer_bank_push_address: String,

    pub kollider_api_key: String,
    pub kollider_api_secret: String,
    pub kollider_api_passphrase: String,

    pub risk_tolerances: HashMap<String, u64>,

    pub kollider_ws_url: String,
    pub logging_settings: LoggingSettings,
    // pub hedge_settings: HashMap<Currency, HedgeSettings>,
    pub influx_host: Option<String>,
    pub influx_org: String,
    pub influx_bucket: String,
    pub influx_token: String,

    pub position_min_leverage: Decimal,
    pub position_max_leverage: Decimal,
    pub leverage_check_interval_ms: u64,

    pub spread: Decimal,
    pub max_single_order_quantities: HashMap<String, u64>,
}

pub struct DealerEngine {
    _positions: HashMap<Currency, u64>,
    ws_client: Box<dyn WsClient>,
    level2_data: HashMap<Symbol, Level2State>,
    bid_quotes: HashMap<Symbol, BTreeMap<u64, Decimal>>,
    ask_quotes: HashMap<Symbol, BTreeMap<u64, Decimal>>,
    risk_tolerances: HashMap<Currency, u64>,
    // timestamp in microseconds is used as quote id
    guaranteed_quotes: BTreeMap<u128, QuoteResponse>,
    has_received_init_data: bool,
    has_received_symbols: bool,
    has_received_positions: bool,
    is_kollider_authenticated: bool,
    logger: slog::Logger,
    last_bank_state: Option<BankState>,
    last_bank_state_timestamp: Option<Instant>,
    hedged_qtys: HashMap<Symbol, Decimal>,
    position_min_leverage: Decimal,
    position_max_leverage: Decimal,
    leverage_check_interval_ms: u64,
    last_leverage_check_timestamp: Instant,
    spread: Decimal,
    funding_profit: Decimal,
    max_single_order_quantities: HashMap<String, u64>,
}

impl DealerEngine {
    pub fn new(
        settings: DealerEngineSettings,
        ws_client: impl WsClient + 'static,
        initial_funding_pnl: Decimal,
    ) -> Self {
        let mut settings = settings;

        let risk_tolerances = settings
            .risk_tolerances
            .into_iter()
            .map(|(c, r)| {
                let currency = match Currency::from_str(&c) {
                    Ok(converted) => converted,
                    Err(err) => {
                        panic!("Failed to convert a settings item {c} into a currency, reason: {err:?}");
                    }
                };
                (currency, r)
            })
            .collect::<HashMap<Currency, u64>>();

        settings.logging_settings.name = String::from("Dealer");
        let logger = init_log(&settings.logging_settings);

        let hedged_qtys = HashMap::new();

        // making sure that leverage adjustment action is performed first time position state is received
        let last_leverage_check_timestamp =
            Instant::now().sub(Duration::from_millis(settings.leverage_check_interval_ms + 1));

        Self {
            risk_tolerances,
            ws_client: Box::new(ws_client),
            _positions: HashMap::new(),
            level2_data: HashMap::new(),
            bid_quotes: HashMap::new(),
            ask_quotes: HashMap::new(),
            last_bank_state: None,
            has_received_init_data: false,
            has_received_symbols: false,
            has_received_positions: false,
            is_kollider_authenticated: false,
            guaranteed_quotes: BTreeMap::new(),
            last_bank_state_timestamp: None,
            hedged_qtys,
            logger,
            position_min_leverage: settings.position_min_leverage,
            position_max_leverage: settings.position_max_leverage,
            leverage_check_interval_ms: settings.leverage_check_interval_ms,
            last_leverage_check_timestamp,
            spread: settings.spread,
            funding_profit: initial_funding_pnl,
            max_single_order_quantities: settings.max_single_order_quantities,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.has_received_init_data
    }

    pub fn has_bank_state(&self) -> bool {
        self.last_bank_state.is_some()
    }

    pub fn get_hedged_quantity(&self, symbol: Symbol) -> Result<Decimal, KolliderClientError> {
        let position_state = self.ws_client.get_position_state(&symbol)?;
        let position = match position_state {
            Some(p) => match p.side {
                None => dec!(0),
                Some(_) => p.quantity,
            },
            None => dec!(0),
        };
        Ok(position)
    }

    pub fn check_has_received_initial_data(&mut self) {
        if self.has_received_positions && self.has_received_symbols && self.is_kollider_authenticated {
            self.has_received_init_data = true;
        } else {
            self.has_received_init_data = false;
        }
    }

    /// We always round up. Every contract is currently worth one unit of the quote currency.
    pub fn calc_num_contracts_for_value(
        &self,
        value: Decimal,
        _symbol: Symbol,
        _denom: Denom,
    ) -> Result<Decimal, DealerError> {
        //TODO: We can make this up to the configured contrac size. Currently 1 contract is worth
        // BTCUSD.PERP => 1 USD
        // EURUSD.PERP => 1 EUR
        // GBPUSD.PERP => 1 GBP

        let value_in_whole_currency_units = value;

        if value_in_whole_currency_units > dec!(0.0) {
            Ok(value_in_whole_currency_units.round_dp_with_strategy(0, RoundingStrategy::AwayFromZero))
        } else {
            Ok(value_in_whole_currency_units.round_dp_with_strategy(0, RoundingStrategy::ToZero))
        }
    }

    pub fn sweep_excess_funds<F: FnMut(Message)>(&self, listener: &mut F) {
        if let Some(balances) = self.ws_client.get_all_balances() {
            slog::info!(self.logger, "Sweeping: {:?}", balances);
            if let Some(sat_balance) = balances.cash.get(&Symbol::from("SAT")) {
                if *sat_balance > dec!(10_000) {
                    if let Some(amount) = sat_balance.to_u64() {
                        let msg = Message::Dealer(Dealer::CreateInvoiceRequest(CreateInvoiceRequest {
                            req_id: Uuid::new_v4(),
                            amount,
                            memo: "Excess funds withdrawal".to_string(),
                        }));
                        listener(msg);
                    } else {
                        slog::info!(
                            self.logger,
                            "Sweeping excess funds failed. Could not convert balance value: {} to u64",
                            sat_balance
                        );
                    }
                }
            }
        }
    }

    pub fn check_health<F: FnMut(Message)>(&self, listener: &mut F) {
        slog::info!(self.logger, "Checking Dealer Health.");
        let is_authenticated = self.ws_client.is_authenticated();
        let available_currencies = self
            .ws_client
            .get_tradable_symbols()
            .into_iter()
            .filter_map(|(symbol, _)| {
                let base = &symbol[0..3];
                let quote_currency_str = &symbol[3..6];
                if let Ok(quote) = Currency::from_str(quote_currency_str) {
                    if base == "BTC" {
                        Some(quote)
                    } else {
                        None
                    }
                } else {
                    slog::error!(
                        self.logger,
                        "Could not convert {} to a valid currency",
                        quote_currency_str
                    );
                    None
                }
            })
            .collect::<HashSet<Currency>>();

        let mut available_currencies = available_currencies.into_iter().collect::<Vec<_>>();
        let rates = available_currencies
            .iter()
            .filter_map(|currency| {
                let money = Money::new(Currency::BTC, dec!(0.00000001));
                let (rate, _fees) = self.get_rate(money, *currency);
                rate.map(|rate| ((Currency::BTC, *currency), rate))
            })
            .collect();
        available_currencies.push(Currency::BTC);

        let status = if is_authenticated {
            HealthStatus::Running
        } else {
            HealthStatus::Down
        };

        let dealer_health = DealerHealth {
            status,
            available_currencies,
            rates,
            timestamp: time_now(),
        };

        let msg = Message::Dealer(Dealer::Health(dealer_health));
        listener(msg);
    }

    pub fn check_risk<F: FnMut(Message)>(&mut self, _listener: &mut F) {
        if let Some(state) = self.last_bank_state.clone() {
            self.check_risk_from_bank_state(state, _listener);
        }
    }

    fn check_risk_from_bank_state<F: FnMut(Message)>(&mut self, bank_state: BankState, _listener: &mut F) {
        slog::info!(self.logger, "Checking Risk.");

        if !self.has_received_init_data {
            slog::info!(self.logger, "Not received all data. Skip checking risk.");
            return;
        }

        slog::info!(self.logger, "{:?}", bank_state);
        for (_account_id, account) in bank_state.fiat_exposures.into_iter() {
            let currency = account.currency;
            let exposure = account.balance;

            if currency == Currency::BTC || currency == Currency::KKP {
                continue;
            }

            let symbol = Symbol::from(currency);
            let denom = Denom::from_currency(currency);

            let qty_contracts_required = match self.calc_num_contracts_for_value(exposure, symbol.clone(), denom) {
                Ok(q) => q,
                Err(_) => continue,
            };

            slog::info!(
                self.logger,
                "Target number of {} contracts: {}",
                symbol,
                qty_contracts_required
            );

            let currently_hedged_qty = match self.ws_client.get_position_state(&symbol) {
                Ok(position_state) => match position_state {
                    Some(p) => match p.side {
                        None => dec!(0),
                        Some(side) => {
                            let side_sign = Decimal::new(side.to_sign(), 0);
                            side_sign * p.quantity
                        }
                    },
                    None => dec!(0),
                },
                Err(err) => {
                    slog::info!(
                        self.logger,
                        "Position state is undefined: {:?} - skipping risk calculation for symbol: {}",
                        err,
                        symbol
                    );
                    continue;
                }
            };

            self.hedged_qtys.insert(symbol.clone(), currently_hedged_qty);

            slog::info!(
                self.logger,
                "Current number of {} contracts: {}",
                symbol,
                currently_hedged_qty
            );

            // If negative we need to sell more and if positive we need to buy more.
            // This works under the assumption that qty_contracts_required is <= 0.
            let delta_qty = qty_contracts_required - currently_hedged_qty;

            let risk_tolerance = match self.risk_tolerances.get(&currency) {
                Some(t) => t,
                None => continue,
            };

            if delta_qty.abs() < Decimal::new(*risk_tolerance as i64, 0) {
                slog::info!(
                    self.logger,
                    "Delta qty of {} within risk tolerance of {}. NO ACTION.",
                    delta_qty,
                    risk_tolerance
                );
                continue;
            }

            let (order_quantity, trade_side) = match delta_qty.to_i64() {
                Some(converted) => (converted.unsigned_abs(), Side::from_sign(converted)),
                None => {
                    slog::error!(
                        self.logger,
                        "Could not convert delta quantity of {} into i64",
                        delta_qty,
                    );
                    panic!("Could not convert delta quantity into i64");
                }
            };

            let capped_order_quantity = match self.max_single_order_quantities.get(&symbol) {
                Some(cap) => {
                    slog::info!(
                        self.logger,
                        "Capping order quantity for {} from {} to {}",
                        symbol,
                        order_quantity,
                        cap
                    );
                    *cap
                }
                None => order_quantity,
            };

            slog::info!(
                self.logger,
                "Placing trade on side: {:?} of qty: {} for symbol: {}",
                trade_side,
                capped_order_quantity,
                symbol
            );

            self.ws_client
                .make_order(capped_order_quantity, symbol, trade_side)
                .expect("Failed to create order");
        }
    }

    pub fn process_msg<F: FnMut(Message)>(&mut self, msg: Message, listener: &mut F) {
        match msg {
            Message::Api(msg) => match msg {
                Api::SwapRequest(swap_request) => {
                    let mut swap_response = SwapResponse {
                        req_id: swap_request.req_id,
                        uid: swap_request.uid,
                        success: true,
                        amount: swap_request.amount,
                        from: swap_request.from,
                        to: swap_request.to,
                        rate: None,
                        error: None,
                        fees: None,
                    };
                    if swap_request.from == swap_request.to {
                        swap_response.success = false;
                        swap_response.error = Some(SwapResponseError::Invalid);
                        let msg = Message::Api(Api::SwapResponse(swap_response));
                        listener(msg);
                        return;
                    }
                    let time_now = SystemTime::now();
                    let invalidated_quotes = time_now
                        .sub(Duration::from_millis(QUOTE_TTL_MS))
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("System time should not be set to earlier than epoch start")
                        .as_micros();
                    self.guaranteed_quotes = self.guaranteed_quotes.split_off(&invalidated_quotes);
                    let is_linear = if swap_request.from == Currency::BTC || swap_request.to == Currency::BTC {
                        ConversionInfo::new(swap_request.from, swap_request.to).is_linear()
                    } else {
                        true
                    };
                    let (current_rate, fees) = self.get_rate(swap_request.amount, swap_request.to);
                    match swap_request.quote_id {
                        None => {
                            if current_rate.is_some() {
                                swap_response.rate = current_rate;
                                swap_response.fees = fees;
                            } else {
                                swap_response.success = false;
                                swap_response.error = Some(SwapResponseError::CurrencyNotAvailable);
                            }
                        }
                        Some(quote_id) => match self.guaranteed_quotes.remove(&quote_id) {
                            None => {
                                swap_response.success = false;
                                swap_response.error = Some(SwapResponseError::InvalidQuoteId);
                            }
                            Some(quote) => match validate_quote(&quote, &swap_request) {
                                Ok(_) => {
                                    let best_rate = get_better_rate(quote.rate, current_rate, is_linear);
                                    let best_fees = if best_rate == quote.rate { quote.fees } else { fees };
                                    swap_response.rate = best_rate;
                                    swap_response.fees = best_fees;
                                }
                                Err(_) => {
                                    swap_response.success = false;
                                    swap_response.error = Some(SwapResponseError::InvalidQuoteId);
                                }
                            },
                        },
                    }
                    let msg = Message::Api(Api::SwapResponse(swap_response));
                    listener(msg);
                }
                Api::QuoteRequest(quote_request) => {
                    let mut quote_response = QuoteResponse {
                        req_id: quote_request.req_id,
                        uid: quote_request.uid,
                        amount: quote_request.amount,
                        from: quote_request.from,
                        to: quote_request.to,
                        valid_until: 0,
                        rate: None,
                        quote_id: None,
                        error: None,
                        fees: None,
                    };
                    let (rate, fees) = self.get_rate(quote_request.amount, quote_request.to);
                    if rate.is_some() {
                        let time_now = SystemTime::now();
                        let quote_id = time_now
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .expect("System time should not be set to earlier than epoch start")
                            .as_micros();
                        let valid_until = time_now
                            .add(Duration::from_millis(QUOTE_TTL_MS))
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .expect("System time should not be set to earlier than epoch start")
                            .as_millis() as u64;
                        quote_response.quote_id = Some(quote_id);
                        quote_response.rate = rate;
                        quote_response.valid_until = valid_until;
                        quote_response.fees = fees;
                        self.guaranteed_quotes.insert(quote_id, quote_response.clone());
                    } else {
                        quote_response.error = Some(QuoteResponseError::CurrencyNotAvailable);
                    }
                    let msg = Message::Api(Api::QuoteResponse(quote_response));
                    listener(msg);
                }
                Api::AvailableCurrenciesRequest(available_currencies_request) => {
                    let tradable_symbols = self.ws_client.get_tradable_symbols();
                    let mut currencies = tradable_symbols
                        .into_iter()
                        .filter_map(|(s, _)| get_base_currency_from_symbol(s).ok())
                        .collect::<HashSet<Currency>>()
                        .into_iter()
                        .collect::<Vec<Currency>>();
                    currencies.push(Currency::BTC);

                    let response = AvailableCurrenciesResponse {
                        currencies,
                        req_id: available_currencies_request.req_id,
                        error: None,
                    };
                    let msg = Message::Api(Api::AvailableCurrenciesResponse(response));
                    listener(msg);
                }
                Api::InvoiceRequest(invoice_request) => {
                    let conversion_info = ConversionInfo::new(Currency::BTC, invoice_request.currency);
                    // We assume user specifies the value not the amount.
                    let (rate, fees) = self.get_rate_inv(invoice_request.amount, conversion_info);
                    let mut invoice_response = InvoiceResponse {
                        rate: None,
                        amount: invoice_request.amount,
                        req_id: invoice_request.req_id,
                        uid: invoice_request.uid,
                        metadata: invoice_request.metadata,
                        meta: invoice_request.meta,
                        currency: invoice_request.currency,
                        target_account_currency: invoice_request.target_account_currency,
                        payment_request: None,
                        payment_hash: None,
                        account_id: None,
                        error: None,
                        fees: None,
                    };
                    if rate.is_none() {
                        invoice_response.error = Some(InvoiceResponseError::RateNotAvailable);
                    } else {
                        invoice_response.rate = rate;
                        invoice_response.fees = fees;
                    }
                    let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                    listener(msg);
                }
                Api::PaymentRequest(mut msg) => {
                    let conversion_info = ConversionInfo::new(msg.currency, Currency::BTC);
                    // We assume user specifies the value not the amount.
                    match msg.invoice_amount {
                        Some(amount) => {
                            let (rate, fees) = self.get_rate_inv(amount, conversion_info);
                            if rate.is_none() {
                                return;
                            } else {
                                msg.rate = rate;
                                msg.fees = fees;
                            }
                            let msg = Message::Api(Api::PaymentRequest(msg));
                            listener(msg);
                        }
                        None => {
                            slog::error!(self.logger, "Discarded a payment request without amount: {:?}", msg);
                        }
                    }
                }
                Api::CreateLnurlWithdrawalRequest(mut msg) => {
                    let conversion_info = ConversionInfo::new(msg.currency, Currency::BTC);
                    // We assume user specifies the value not the amount.
                    let amount = msg.amount;
                    let (rate, fees) = self.get_rate_inv(amount, conversion_info);
                    if rate.is_none() {
                        return;
                    } else {
                        msg.rate = rate;
                        msg.fees = fees;
                    }
                    let msg = Message::Api(Api::CreateLnurlWithdrawalRequest(msg));
                    listener(msg);
                }
                _ => {}
            },
            Message::KolliderApiResponse(msg) => {
                match msg {
                    KolliderApiResponse::Disconnected(disconnection) => {
                        slog::warn!(
                            self.logger,
                            "Disconnected from the Kollider exchange at {}",
                            disconnection.timestamp
                        );
                        self.reset_state();
                    }
                    KolliderApiResponse::Reconnected(reconnection) => {
                        slog::warn!(
                            self.logger,
                            "Re-connected to the Kollider exchange at {}",
                            reconnection.timestamp
                        );
                        slog::info!(self.logger, "Re-subscribing to position states after reconnection");
                        if let Err(err) = self.ws_client.subscribe(vec![Channel::PositionStates], None) {
                            slog::error!(
                                self.logger,
                                "Failed to re-subscribe position states after reconnection, reason: {:?}",
                                err
                            );
                        }
                    }
                    KolliderApiResponse::Authenticate(auth) => {
                        if auth.success() {
                            slog::info!(self.logger, "Successful Kollider authenticated!");
                            self.is_kollider_authenticated = true;
                            self.check_has_received_initial_data();
                        }
                        slog::info!(self.logger, "Re-subscribing to position states after authentication");
                        if let Err(err) = self.ws_client.subscribe(vec![Channel::PositionStates], None) {
                            slog::error!(
                                self.logger,
                                "Failed to re-subscribe position states after authentication, reason: {:?}",
                                err
                            );
                        }
                    }
                    KolliderApiResponse::OrderInvoice(order_invoice) => {
                        // Received an order invoice. We need to send this to the bank to pay for it.
                        let msg = Message::Dealer(Dealer::PayInvoice(PayInvoice {
                            req_id: Uuid::new_v4(),
                            payment_request: order_invoice.invoice,
                        }));
                        listener(msg)
                    }
                    KolliderApiResponse::SettlementRequest(settlement_request) => {
                        slog::info!(self.logger, "Received settlement request trying to withdraw.");
                        if let Ok(amount) = settlement_request.amount.parse() {
                            let msg = Message::Dealer(Dealer::CreateInvoiceRequest(CreateInvoiceRequest {
                                req_id: Uuid::new_v4(),
                                amount,
                                memo: format!("Withdrawal upon settlement on {}", settlement_request.symbol),
                            }));
                            listener(msg);
                        } else {
                            slog::error!(
                                self.logger,
                                "Received a settlement request with incorrect amount: {:?}",
                                settlement_request
                            );
                            panic!("Received a settlement request with incorrect amount");
                        }
                    }
                    KolliderApiResponse::Positions(positions) => {
                        // positions are not stored, however, from this point we know
                        // that ws client received them too and they can be fetched
                        // from its state
                        slog::info!(self.logger, "Received positions {:?}", positions);
                        self.has_received_positions = true;
                        self.check_has_received_initial_data();
                    }
                    KolliderApiResponse::PositionStates(position) => {
                        slog::info!(self.logger, "Received position state {:?}", position);
                        self.maintain_leverage(&position);
                    }
                    KolliderApiResponse::Level2State(level2state) => {
                        self.process_orderbook_update(level2state);
                    }
                    KolliderApiResponse::TradableSymbols(tradable_symbols) => {
                        slog::info!(self.logger, "Received Symbols");
                        let mut available_symbols = vec![];
                        tradable_symbols.symbols.into_iter().for_each(|(s, _)| {
                            available_symbols.push(s);
                        });
                        self.has_received_symbols = true;
                        self.check_has_received_initial_data();
                        if let Err(err) = self.ws_client.subscribe(
                            vec![Channel::MarkPrices, Channel::OrderbookLevel2],
                            Some(available_symbols),
                        ) {
                            slog::error!(
                                self.logger,
                                "Failed to subscribe mark price and L2 order book updates, reason: {:?}",
                                err
                            );
                        }
                    }
                    KolliderApiResponse::ChangeMarginSuccess(ref change_margin_success) => {
                        if change_margin_success.amount.is_sign_negative() {
                            let amount = -change_margin_success.amount;
                            slog::info!(
                                self.logger,
                                "Reduced {} position margin by {} sats. Trying to withdraw",
                                change_margin_success.symbol,
                                amount
                            );
                            let memo = format!("Reduced {} position margin", change_margin_success.symbol);
                            if let Some(amount) = amount.to_u64() {
                                let msg =
                                    Message::Dealer(Dealer::CreateInsuranceInvoiceRequest(CreateInvoiceRequest {
                                        req_id: Uuid::new_v4(),
                                        amount,
                                        memo,
                                    }));
                                listener(msg);
                            } else {
                                panic!("Received change margin success message with incorrect amount: {msg:?}");
                            }
                        }
                    }
                    KolliderApiResponse::AddMarginRequest(add_margin_request) => {
                        slog::info!(
                            self.logger,
                            "Received add margin request of {} sats for {} position. Requesting invoice to be paid",
                            add_margin_request.amount,
                            add_margin_request.symbol,
                        );
                        let msg = Message::Dealer(Dealer::PayInsuranceInvoice(PayInvoice {
                            req_id: Uuid::new_v4(),
                            payment_request: add_margin_request.invoice,
                        }));
                        listener(msg)
                    }
                    KolliderApiResponse::FundingPayment(funding_payment) => {
                        self.update_funding_profit(funding_payment.amount);
                    }
                    KolliderApiResponse::Balances(balances) => {
                        let karma = balances.cash.get("KKP").cloned().unwrap_or_default();
                        let msg = Message::Dealer(Dealer::KarmaBalance(KarmaBalance { karma }));
                        listener(msg)
                    }
                    _ => {
                        slog::warn!(self.logger, "Handling of KolliderApiResponse {:?} not implemented", msg);
                    }
                }
            }
            Message::Dealer(Dealer::BankState(bank_state)) => {
                dbg!(&bank_state);
                self.last_bank_state_timestamp = Some(Instant::now());
                self.last_bank_state = Some(bank_state.clone());
                self.check_risk_from_bank_state(bank_state, listener);
            }

            Message::Dealer(Dealer::CreateInvoiceResponse(ref create_invoice_response)) => {
                slog::info!(self.logger, "Dealer trying to withdrawal.");
                self.ws_client
                    .make_withdrawal(
                        create_invoice_response.amount,
                        create_invoice_response.payment_request.clone(),
                    )
                    .expect("Failed to make a withdrawal");
            }
            Message::Dealer(Dealer::FiatDepositRequest(msg)) => {
                let conversion_info = ConversionInfo::new(Currency::BTC, msg.currency);
                // We assume user specifies the value not the amount.
                let (rate, fees) = self.get_rate_inv(msg.amount, conversion_info);

                let mut fiat_deposit_response = FiatDepositResponse {
                    req_id: msg.req_id,
                    uid: msg.uid,
                    amount: msg.amount,
                    currency: msg.currency,
                    payment_request: msg.payment_request,
                    rate: None,
                    error: None,
                    fees: None,
                };

                if rate.is_none() {
                    fiat_deposit_response.error = Some(FiatDepositResponseError::CurrencyNotAvailable);
                } else {
                    fiat_deposit_response.rate = rate;
                    fiat_deposit_response.fees = fees;
                }

                let msg = Message::Dealer(Dealer::FiatDepositResponse(fiat_deposit_response));
                listener(msg);
            }
            _ => {}
        }
    }

    fn process_orderbook_update(&mut self, level2_state: Level2State) {
        let symbol = level2_state.symbol.clone();
        match level2_state.update_type.as_str() {
            "snapshot" => {
                self.level2_data.insert(symbol.clone(), level2_state);
            }
            "delta" => {
                let maybe_book = self.level2_data.get_mut(&symbol);
                if let Some(book) = maybe_book {
                    level2_state
                        .bids
                        .iter()
                        .for_each(|(price, volume)| match volume.cmp(&0) {
                            Ordering::Less => {}
                            Ordering::Equal => {
                                book.bids.remove(price);
                            }
                            Ordering::Greater => {
                                book.bids.insert(*price, *volume);
                            }
                        });
                    level2_state
                        .asks
                        .iter()
                        .for_each(|(price, volume)| match volume.cmp(&0) {
                            Ordering::Less => {}
                            Ordering::Equal => {
                                book.asks.remove(price);
                            }
                            Ordering::Greater => {
                                book.asks.insert(*price, *volume);
                            }
                        });
                }
            }
            _ => panic!("Unsupported level2 update"),
        }
        self.update_quotes(&symbol);
    }

    fn update_quotes(&mut self, symbol: &Symbol) {
        const QUANTITIES: [u64; 9] = [10, 100, 1_000, 2_000, 3_000, 5_000, 10_000, 100_000, 1_000_000];

        let tradable_symbols = self.ws_client.get_tradable_symbols();
        let contract = match tradable_symbols.get(symbol) {
            Some(c) => c,
            None => return,
        };

        let dp = contract.price_dp;

        let book = match self.level2_data.get(symbol) {
            Some(l2_data) => l2_data,
            None => return,
        };

        let mut bid_quantity_at_level = 0;
        let mut bid_price_at_level = dec!(0);
        let mut bid_quantity_so_far = 0;
        let mut bid_num = dec!(0);
        let mut bid_levels = book.bids.iter().rev();

        let mut ask_quantity_at_level = 0;
        let mut ask_price_at_level = dec!(0);
        let mut ask_quantity_so_far = 0;
        let mut ask_num = dec!(0);
        let mut ask_levels = book.asks.iter();

        for qty in QUANTITIES {
            let mut bid_to_match = qty - bid_quantity_so_far;
            while bid_to_match > 0 {
                if bid_to_match <= bid_quantity_at_level {
                    bid_quantity_at_level -= bid_to_match;
                    bid_quantity_so_far += bid_to_match;
                    bid_num += bid_price_at_level * Decimal::new(bid_to_match as i64, 0);
                    bid_to_match = 0;
                } else {
                    bid_quantity_so_far += bid_quantity_at_level;
                    bid_num += bid_price_at_level * Decimal::new(bid_quantity_at_level as i64, 0);
                    bid_to_match -= bid_quantity_at_level;
                    match bid_levels.next() {
                        Some((price, volume)) => {
                            bid_price_at_level = *price;
                            bid_quantity_at_level = *volume;
                        }
                        None => {
                            bid_price_at_level = dec!(0);
                            bid_quantity_at_level = 0;
                            break;
                        }
                    }
                }
            }
            let mut ask_to_match = qty - ask_quantity_so_far;
            while ask_to_match > 0 {
                if ask_to_match <= ask_quantity_at_level {
                    ask_quantity_at_level -= ask_to_match;
                    ask_quantity_so_far += ask_to_match;
                    ask_num += ask_price_at_level * Decimal::new(ask_to_match as i64, 0);
                    ask_to_match = 0;
                } else {
                    ask_quantity_so_far += ask_quantity_at_level;
                    ask_num += ask_price_at_level * Decimal::new(ask_quantity_at_level as i64, 0);
                    ask_to_match -= ask_quantity_at_level;
                    match ask_levels.next() {
                        Some((price, volume)) => {
                            ask_price_at_level = *price;
                            ask_quantity_at_level = *volume;
                        }
                        None => {
                            ask_price_at_level = dec!(0);
                            ask_quantity_at_level = 0;
                            break;
                        }
                    }
                }
            }
            if bid_to_match == 0 {
                let mut price = (bid_num / Decimal::new(bid_quantity_so_far as i64, 0)).ceil();
                price.set_scale(dp).expect("Could not set scale");
                let quotes = self.bid_quotes.entry(symbol.clone()).or_default();
                quotes.insert(qty, price);
            }
            if ask_to_match == 0 {
                let mut price = (ask_num / Decimal::new(ask_quantity_so_far as i64, 0)).floor();
                price.set_scale(dp).expect("Could not set scale");
                let quotes = self.ask_quotes.entry(symbol.clone()).or_default();
                quotes.insert(qty, price);
            }
        }
    }

    #[inline]
    fn get_spread(&self) -> Decimal {
        self.spread
    }

    #[inline]
    fn get_half_spread(&self) -> Decimal {
        self.get_spread() / Decimal::TWO
    }

    #[inline]
    fn get_linear_modifier(&self) -> Decimal {
        Decimal::ONE - self.get_half_spread()
    }

    #[inline]
    fn get_inverse_modifier(&self) -> Decimal {
        Decimal::ONE + self.get_half_spread()
    }

    #[inline]
    fn get_linear_rate(&self, price: Decimal, charge_spread: bool) -> Decimal {
        let modifier = if charge_spread {
            self.get_linear_modifier()
        } else {
            Decimal::ONE
        };
        price * modifier
    }

    #[inline]
    fn get_inverse_rate(&self, price: Decimal, charge_spread: bool) -> Decimal {
        let modifier = if charge_spread {
            self.get_inverse_modifier()
        } else {
            Decimal::ONE
        };
        Decimal::ONE / (price * modifier)
    }

    pub fn get_rate(&self, amount: Money, destination_currency: Currency) -> (Option<Rate>, Option<Money>) {
        if amount.currency() == Currency::BTC || destination_currency == Currency::BTC {
            let conversion = ConversionInfo::new(amount.currency(), destination_currency);
            self.get_btc_cross_rate(amount, conversion, true)
        } else {
            let first_conversion = ConversionInfo::new(amount.currency(), Currency::BTC);
            let (first_rate, _first_fees) = self.get_btc_cross_rate(amount, first_conversion, false);
            match first_rate {
                Some(to_btc_rate) => {
                    let converted_money = Money::new(Currency::BTC, amount.value() * to_btc_rate.value());
                    let second_conversion = ConversionInfo::new(Currency::BTC, destination_currency);
                    let (second_rate, second_fee) = self.get_btc_cross_rate(converted_money, second_conversion, true);
                    match second_rate {
                        Some(to_target_rate) => {
                            let final_rate = Rate::new(
                                amount.currency(),
                                destination_currency,
                                to_btc_rate.value() * to_target_rate.value(),
                            );
                            (Some(final_rate), second_fee)
                        }
                        None => (None, None),
                    }
                }
                None => (None, None),
            }
        }
    }

    fn get_btc_cross_rate(
        &self,
        amount: Money,
        conversion_info: ConversionInfo,
        charge_spread: bool,
    ) -> (Option<Rate>, Option<Money>) {
        // Example 1:
        // from: BTC
        // to: USD
        // base: BTC
        // quote: USD
        // symbol: BTC/USD
        // Look Ask Side

        // Example 1:
        // from: USD
        // to: BTC
        // base: BTC
        // quote: USD
        // symbol: BTC/USD
        // Look Bid Side

        let maybe_quotes = match conversion_info.side {
            Side::Bid => self.ask_quotes.get(&conversion_info.symbol),
            Side::Ask => self.bid_quotes.get(&conversion_info.symbol),
        };

        match maybe_quotes {
            None => (None, None),
            Some(quotes) => {
                let best_price = if conversion_info.from != conversion_info.quote {
                    if let Some((_volume, price)) = quotes.range(0..u64::MAX).next() {
                        *price
                    } else {
                        return (None, None);
                    }
                } else {
                    dec!(1.0)
                };

                let value_in_fiat = amount.value() * best_price;

                if let Some(lookup_quantity) = value_in_fiat.to_u64() {
                    match quotes.range(lookup_quantity..u64::MAX).next() {
                        None => (None, None),
                        Some((_level_vol, price)) => {
                            if conversion_info.is_linear() {
                                let user_rate = self.get_linear_rate(*price, charge_spread);
                                // Fees are paid in the target currency.
                                let value = (price - user_rate) / price * value_in_fiat;
                                let currency = conversion_info.to;
                                let fees = Money::new(currency, value);
                                let rate = Rate::new(conversion_info.from, conversion_info.to, user_rate);
                                (Some(rate), Some(fees))
                            } else {
                                let no_fee_inverse_rate = Decimal::ONE / price;
                                let user_inverse_rate = self.get_inverse_rate(*price, charge_spread);
                                let rate = Rate::new(conversion_info.from, conversion_info.to, user_inverse_rate);
                                // Fees are paid in the target currency.
                                let value = (no_fee_inverse_rate - user_inverse_rate) / no_fee_inverse_rate
                                    * (value_in_fiat / price);
                                let currency = conversion_info.to;
                                let fees = Money::new(currency, value);
                                (Some(rate), Some(fees))
                            }
                        }
                    }
                } else {
                    (None, None)
                }
            }
        }
    }

    fn get_rate_inv(&self, amount: Money, conversion_info: ConversionInfo) -> (Option<Rate>, Option<Money>) {
        let maybe_quotes = match conversion_info.side {
            Side::Bid => self.ask_quotes.get(&conversion_info.symbol),
            Side::Ask => self.bid_quotes.get(&conversion_info.symbol),
        };

        match maybe_quotes {
            None => (None, None),
            Some(quotes) => {
                let value_in_fiat = amount.value().round_dp_with_strategy(0, RoundingStrategy::AwayFromZero);

                if let Some(lookup_quantity) = value_in_fiat.to_u64() {
                    match quotes.range(lookup_quantity..u64::MAX).next() {
                        None => (None, None),
                        Some((_level_vol, price)) => {
                            if conversion_info.is_linear() {
                                let user_rate = self.get_linear_rate(*price, true);
                                // Fees are paid in the target currency.
                                let value = (price - user_rate) / price * value_in_fiat;
                                let currency = conversion_info.to;
                                let fees = Money::new(currency, value);
                                let rate = Rate::new(conversion_info.from, conversion_info.to, user_rate);
                                (Some(rate), Some(fees))
                            } else {
                                let no_fee_inverse_rate = Decimal::ONE / price;
                                let user_inverse_rate = self.get_inverse_rate(*price, true);
                                let rate = Rate::new(conversion_info.from, conversion_info.to, user_inverse_rate);
                                // Fees are paid in the target currency.
                                let value = (no_fee_inverse_rate - user_inverse_rate) / no_fee_inverse_rate
                                    * (value_in_fiat / price);
                                let currency = conversion_info.to;
                                let fees = Money::new(currency, value);
                                (Some(rate), Some(fees))
                            }
                        }
                    }
                } else {
                    (None, None)
                }
            }
        }
    }

    fn reset_state(&mut self) {
        self.level2_data = HashMap::new();
        self.bid_quotes = HashMap::new();
        self.ask_quotes = HashMap::new();
        self.has_received_init_data = false;
        self.has_received_symbols = false;
        self.has_received_positions = false;
        self.is_kollider_authenticated = false;
        self.guaranteed_quotes = BTreeMap::new();
        self.hedged_qtys = HashMap::new();
        self.last_bank_state = None;
        self.last_bank_state_timestamp = None;
    }

    fn maintain_leverage(&mut self, position: &PositionState) {
        let elapsed = self.last_leverage_check_timestamp.elapsed().as_millis() as u64;
        if elapsed <= self.leverage_check_interval_ms {
            return;
        }
        self.last_leverage_check_timestamp = Instant::now();

        if self.position_min_leverage <= position.leverage && position.leverage <= self.position_max_leverage {
            return;
        }

        let current_margin = position.entry_value / position.leverage;
        let required_margin = position.entry_value / Decimal::ONE;
        let margin_delta = required_margin - current_margin;

        if !margin_delta.is_zero() {
            if let Some(amount) = margin_delta.to_i64() {
                slog::info!(
                    self.logger,
                    "Adjusting {} position margin by {}",
                    position.symbol,
                    amount
                );
                if let Err(err) = self.ws_client.change_margin(position.symbol.clone(), amount) {
                    slog::error!(self.logger, "Failed to change margin, reason: {:?}", err)
                }
            }
        }
    }

    fn update_funding_profit(&mut self, funding_amount: Decimal) {
        self.funding_profit += funding_amount;
    }

    pub fn get_pnl(&self) -> DealerPnl {
        let total_pnl = self.last_bank_state.as_ref().and_then(|bank_state| {
            let btc_cash_balance = bank_state
                .fiat_exposures
                .iter()
                .filter_map(|(_account_id, account)| {
                    if account.currency == Currency::BTC && account.account_type == AccountType::Internal {
                        Some(account.balance)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            btc_cash_balance.first().cloned()
        });
        DealerPnl {
            total_pnl,
            funding_pnl: Some(self.funding_profit),
        }
    }
}

fn validate_quote(quote: &QuoteResponse, swap_request: &SwapRequest) -> Result<(), ()> {
    if quote.from != swap_request.from
        || quote.to != swap_request.to
        || quote.amount.value() != swap_request.amount.value()
        || quote.uid != swap_request.uid
    {
        return Err(());
    }
    Ok(())
}

fn get_better_rate(rate1: Option<Rate>, rate2: Option<Rate>, is_linear: bool) -> Option<Rate> {
    match (rate1, rate2) {
        (Some(r1), Some(r2)) => {
            if is_linear {
                if r1.value() > r2.value() {
                    Some(r1)
                } else {
                    Some(r2)
                }
            } else if r1.value() < r2.value() {
                Some(r1)
            } else {
                Some(r2)
            }
        }
        (Some(r1), None) => Some(r1),
        (None, Some(r2)) => Some(r2),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use core_types::{Money, Rate};
    use msgs::kollider_client::Channel;

    struct MockWsClient {
        is_connected: bool,
        is_authenticated: bool,
        position_states: HashMap<Symbol, PositionState>,
        mark_prices: HashMap<Symbol, MarkPrice>,
        balances: RefCell<Balances>,
        tradable_symbols: HashMap<Symbol, TradableSymbol>,
    }

    impl MockWsClient {
        pub fn new() -> Self {
            Self {
                is_connected: true,
                is_authenticated: true,
                position_states: Default::default(),
                mark_prices: Default::default(),
                balances: RefCell::new(Balances {
                    cash: Default::default(),
                    isolated_margin: Default::default(),
                    order_margin: Default::default(),
                    cross_margin: Default::default(),
                }),
                tradable_symbols: [
                    (
                        Symbol::from("BTCEUR.PERP"),
                        TradableSymbol {
                            symbol: Symbol::from("BTCEUR.PERP"),
                            contract_size: dec!(1.0),
                            max_leverage: dec!(20.0),
                            base_margin: dec!(0.01),
                            liquidation_fee: dec!(0.004),
                            is_inverse_priced: true,
                            price_dp: 0,
                            underlying_symbol: Symbol::from(".BTCEUR"),
                            last_price: dec!(23058.0),
                            tick_size: dec!(1.0),
                            risk_limit: dec!(150000000.0),
                        },
                    ),
                    (
                        Symbol::from("ETHUSD.PERP"),
                        TradableSymbol {
                            symbol: Symbol::from("ETHUSD.PERP"),
                            contract_size: dec!(1.0),
                            max_leverage: dec!(10.0),
                            base_margin: dec!(0.02),
                            liquidation_fee: dec!(0.01),
                            is_inverse_priced: false,
                            price_dp: 2,
                            underlying_symbol: Symbol::from(".ETHUSD"),
                            last_price: dec!(1852.17),
                            tick_size: dec!(0.05),
                            risk_limit: dec!(150000000.0),
                        },
                    ),
                    (
                        Symbol::from("BTCUSD.PERP"),
                        TradableSymbol {
                            symbol: Symbol::from("BTCUSD.PERP"),
                            contract_size: dec!(1.0),
                            max_leverage: dec!(20.0),
                            base_margin: dec!(0.01),
                            liquidation_fee: dec!(0.004),
                            is_inverse_priced: true,
                            price_dp: 0,
                            underlying_symbol: Symbol::from(".BTCUSD"),
                            last_price: dec!(23470),
                            tick_size: dec!(1.0),
                            risk_limit: dec!(150000000.0),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            }
        }
    }

    impl WsClient for MockWsClient {
        fn is_authenticated(&self) -> bool {
            self.is_authenticated
        }

        fn get_balance(&self, currency: Currency) -> ws_client::Result<Decimal> {
            if !matches!(currency, Currency::BTC) {
                let symbol: Symbol = currency.into();
                {
                    let (side, upnl) = match self.position_states.get(&symbol) {
                        Some(position) => match position.side.as_ref() {
                            None => {
                                return Err(KolliderClientError::BalanceNotAvailable);
                            }
                            Some(side) => (*side, position.upnl),
                        },
                        None => {
                            return Err(KolliderClientError::BalanceNotAvailable);
                        }
                    };
                    let mark_price = match self.mark_prices.get(&symbol) {
                        None => {
                            return Err(KolliderClientError::BalanceNotAvailable);
                        }
                        Some(mark_price) => mark_price.price,
                    };
                    let margin = match self.balances.borrow().isolated_margin.get(&symbol) {
                        Some(isolated_margin) => *isolated_margin,
                        None => {
                            return Err(KolliderClientError::BalanceNotAvailable);
                        }
                    };
                    let fiat_value = (margin + upnl) * mark_price / SATS_IN_BITCOIN;
                    match side {
                        Side::Bid => Ok(-fiat_value),
                        Side::Ask => Ok(fiat_value),
                    }
                }
            } else {
                let symbol = Symbol::from("SAT");
                self.balances
                    .borrow()
                    .cash
                    .get(&symbol)
                    .cloned()
                    .ok_or(KolliderClientError::BalanceNotAvailable)
            }
        }

        fn subscribe(&self, _channels: Vec<Channel>, _symbols: Option<Vec<Symbol>>) -> Result<(), KolliderClientError> {
            Ok(())
        }

        fn get_all_balances(&self) -> Option<Balances> {
            Some(self.balances.borrow().clone())
        }

        fn get_position_state(&self, symbol: &Symbol) -> Result<Option<PositionState>, KolliderClientError> {
            Ok(self.position_states.get(symbol).cloned())
        }

        fn get_tradable_symbols(&self) -> HashMap<Symbol, TradableSymbol> {
            self.tradable_symbols.clone()
        }

        fn make_withdrawal(&self, amount: u64, _payment_request: String) -> ws_client::Result<()> {
            match self.balances.borrow_mut().cash.get_mut(&Symbol::from("SAT")) {
                Some(balance) => {
                    *balance -= Decimal::new(amount as i64, 0);
                    Ok(())
                }
                None => Err(KolliderClientError::BalanceNotAvailable),
            }
        }

        fn make_order(&self, _quantity: u64, _symbol: Symbol, _side: Side) -> ws_client::Result<()> {
            Ok(())
        }

        fn buy(&self, _quantity: u64, _currency: Currency) -> ws_client::Result<()> {
            Ok(())
        }

        fn sell(&self, _quantity: u64, _currency: Currency) -> ws_client::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.is_connected
        }

        fn is_ready(&self) -> bool {
            self.is_connected && self.is_authenticated
        }

        fn change_margin(&self, _symbol: Symbol, _amount: i64) -> ws_client::Result<()> {
            Ok(())
        }
    }

    use crate::dealer_engine::QUOTE_TTL_MS;
    use crate::{DealerEngine, DealerEngineSettings};
    use core_types::kollider_client::{Balances, MarkPrice, PositionState, Side};
    use core_types::{Currency, Symbol, SATS_IN_BITCOIN};
    use msgs::api::{Api, QuoteRequest, QuoteResponseError, SwapRequest, SwapResponseError};
    use msgs::kollider_client::{KolliderApiResponse, Level2State, TradableSymbol};
    use msgs::Message;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashMap, VecDeque};
    use std::time::Duration;
    use utils::xlogging::*;
    use uuid::Uuid;
    use ws_client::WsClient;
    use xerror::kollider_client::KolliderClientError;

    fn initialise_dealer_engine() -> DealerEngine {
        let settings = DealerEngineSettings {
            psql_url: "".to_string(),
            dealer_bank_pull_address: "".to_string(),
            dealer_bank_push_address: "".to_string(),
            kollider_api_key: "".to_string(),
            kollider_api_secret: "".to_string(),
            kollider_api_passphrase: "".to_string(),
            kollider_ws_url: "".to_string(),
            risk_tolerances: HashMap::new(),
            logging_settings: LoggingSettings {
                name: String::from(""),
                slack_hook: "".to_string(),
                level: String::from("debug"),
                stdout: false,
                log_path: None,
                slack_channel: "".to_string(),
            },
            influx_host: None,
            influx_org: "".to_string(),
            influx_bucket: "".to_string(),
            influx_token: "".to_string(),
            position_min_leverage: dec!(0.9999),
            position_max_leverage: dec!(1.0001),
            leverage_check_interval_ms: 1000,
            spread: dec!(0.01),
            max_single_order_quantities: HashMap::new(),
        };
        let ws_client = MockWsClient::new();
        let mut dealer = DealerEngine::new(settings, ws_client, Decimal::ZERO);

        // BTC/USD
        let mut bids = BTreeMap::new();
        bids.insert(Decimal::new(10000, 0), 5000);
        bids.insert(Decimal::new(20000, 0), 2000);
        bids.insert(Decimal::new(30000, 0), 1000);
        let mut asks = BTreeMap::new();
        asks.insert(Decimal::new(40000, 0), 1000);
        asks.insert(Decimal::new(50000, 0), 2000);
        asks.insert(Decimal::new(60000, 0), 5000);
        let level2state = Level2State {
            update_type: "snapshot".to_string(),
            seq_number: 0,
            symbol: Symbol::from("BTCUSD.PERP"),
            bids,
            asks,
        };
        dealer.process_msg(
            Message::KolliderApiResponse(KolliderApiResponse::Level2State(level2state)),
            &mut |_msg| {},
        );

        // BTC/EUR
        let mut bids = BTreeMap::new();
        bids.insert(Decimal::new(9000, 0), 5000);
        bids.insert(Decimal::new(18000, 0), 2000);
        bids.insert(Decimal::new(27000, 0), 1000);
        let mut asks = BTreeMap::new();
        asks.insert(Decimal::new(36000, 0), 1000);
        asks.insert(Decimal::new(45000, 0), 2000);
        asks.insert(Decimal::new(54000, 0), 5000);
        let level2state = Level2State {
            update_type: "snapshot".to_string(),
            seq_number: 0,
            symbol: Symbol::from("BTCEUR.PERP"),
            bids,
            asks,
        };
        dealer.process_msg(
            Message::KolliderApiResponse(KolliderApiResponse::Level2State(level2state)),
            &mut |_msg| {},
        );
        dealer
    }

    #[test]
    fn rate_first_bucket() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0001));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(29850.0)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
        let money = Money::new(Currency::USD, dec!(9.0));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::USD,
            to: Currency::BTC,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::USD);
                assert_eq!(quote_response.to, Currency::BTC);
                assert_eq!(
                    quote_response.rate.map(|rate| rate.value()),
                    Some(Rate::normalized_value(dec!(1) / dec!(40200.0)))
                );
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
    }

    #[test]
    fn rate_not_first_bucket() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
        let money = Money::new(Currency::USD, dec!(3500));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::USD,
            to: Currency::BTC,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::USD);
                assert_eq!(quote_response.to, Currency::BTC);
                assert_eq!(
                    quote_response.rate.map(|rate| rate.value()),
                    Some(Rate::normalized_value(dec!(1) / dec!(52260)))
                );
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
    }

    #[test]
    fn rate_no_bucket() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::USD, dec!(100000.0));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::USD,
            to: Currency::BTC,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert!(quote_response.rate.is_none());
                assert!(quote_response.quote_id.is_none());
                assert!(matches!(
                    quote_response.error,
                    Some(QuoteResponseError::CurrencyNotAvailable)
                ));
                break;
            }
        }
        let money = Money::new(Currency::BTC, dec!(10.0));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert!(quote_response.rate.is_none());
                assert!(quote_response.quote_id.is_none());
                assert!(matches!(
                    quote_response.error,
                    Some(QuoteResponseError::CurrencyNotAvailable)
                ));
                break;
            }
        }
    }

    #[test]
    fn rate_no_symbol() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::GBP, dec!(10.0));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::GBP,
            to: Currency::BTC,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert!(quote_response.rate.is_none());
                assert!(quote_response.quote_id.is_none());
                assert!(matches!(
                    quote_response.error,
                    Some(QuoteResponseError::CurrencyNotAvailable)
                ));
                break;
            }
        }
        let money = Money::new(Currency::BTC, dec!(0.001));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::GBP,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert!(quote_response.rate.is_none());
                assert!(quote_response.quote_id.is_none());
                assert!(matches!(
                    quote_response.error,
                    Some(QuoteResponseError::CurrencyNotAvailable)
                ));
                break;
            }
        }
    }

    #[test]
    fn swap_without_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::BTC);
                assert_eq!(swap_response.to, Currency::USD);
                assert_eq!(swap_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(swap_response.error.is_none());
                break;
            }
        }
        let money = Money::new(Currency::USD, dec!(3500.0));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::USD,
            to: Currency::BTC,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::USD);
                assert_eq!(swap_response.to, Currency::BTC);
                assert_eq!(
                    swap_response.rate.map(|rate| rate.value()),
                    Some(Rate::normalized_value(dec!(1) / dec!(52260)))
                );
                assert!(swap_response.error.is_none());
                break;
            }
        }
    }

    #[test]
    fn swap_with_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut quote = None;
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                quote = Some(quote_response);
                break;
            }
        }
        let quote = quote.expect("Expected a valid quote");
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: quote.amount,
            from: quote.from,
            to: quote.to,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert_eq!(swap_response.rate, quote.rate);
                assert!(swap_response.error.is_none());
                break;
            }
        }
    }

    #[test]
    fn swap_invalid_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
            quote_id: Some(12345),
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::BTC);
                assert_eq!(swap_response.to, Currency::USD);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
        let money = Money::new(Currency::USD, dec!(3500.0));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::USD,
            to: Currency::BTC,
            quote_id: Some(67890),
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::USD);
                assert_eq!(swap_response.to, Currency::BTC);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
    }

    #[test]
    fn swap_expired_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut quote = None;
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                quote = Some(quote_response);
                break;
            }
        }
        let quote = quote.expect("Expected a valid quote");
        std::thread::sleep(Duration::from_millis(QUOTE_TTL_MS + 50));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: quote.amount,
            from: quote.from,
            to: quote.to,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
    }

    #[test]
    fn swap_malicious_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut quote = None;
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                quote = Some(quote_response);
                break;
            }
        }
        let quote = quote.expect("Expected a valid quote");
        // modified user id
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: 728,
            amount: quote.amount,
            from: quote.from,
            to: quote.to,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, 728);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
        let money = Money::new(Currency::BTC, dec!(0.555));
        // modified amount
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: money,
            from: quote.from,
            to: quote.to,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
        // modified currencies
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: quote.amount,
            from: Currency::BTC,
            to: Currency::GBP,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, Currency::BTC);
                assert_eq!(swap_response.to, Currency::GBP);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
    }

    #[test]
    fn swap_reuse_quote() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let money = Money::new(Currency::BTC, dec!(0.0875));
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: money,
            from: Currency::BTC,
            to: Currency::USD,
        };
        dealer_engine.process_msg(Message::Api(Api::QuoteRequest(quote_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut quote = None;
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::QuoteResponse(quote_response)) = msg {
                assert_eq!(quote_response.uid, uid);
                assert_eq!(quote_response.from, Currency::BTC);
                assert_eq!(quote_response.to, Currency::USD);
                assert_eq!(quote_response.rate.map(|rate| rate.value()), Some(dec!(23217.33)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                quote = Some(quote_response);
                break;
            }
        }
        let quote = quote.expect("Expected a valid quote");
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: quote.amount,
            from: quote.from,
            to: quote.to,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request.clone())), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert_eq!(swap_response.rate, quote.rate);
                assert!(swap_response.error.is_none());
                break;
            }
        }
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, quote.from);
                assert_eq!(swap_response.to, quote.to);
                assert!(swap_response.rate.is_none());
                assert!(matches!(swap_response.error, Some(SwapResponseError::InvalidQuoteId)));
                break;
            }
        }
    }

    #[test]
    fn triangular_swap() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let initial_btc_balance = dec!(0.0001);
        let btc_balance = Money::new(Currency::BTC, initial_btc_balance);
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: btc_balance,
            from: Currency::BTC,
            to: Currency::USD,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut btc_usd_rate = dec!(0.0);
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::BTC);
                assert_eq!(swap_response.to, Currency::USD);
                let swap_rate = swap_response.rate.map(|rate| rate.value());
                assert_eq!(swap_response.rate.map(|rate| rate.value()), Some(dec!(29850.0)));
                btc_usd_rate = swap_rate.expect("Valid rate expected");
                assert!(swap_response.error.is_none());
                break;
            }
        }
        let usd_balance = Money::new(Currency::USD, btc_balance.value() * btc_usd_rate);
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: usd_balance,
            from: Currency::USD,
            to: Currency::EUR,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let expected_usd_eur_rate = dec!(27_000.0) / dec!(40_000.0) * (dec!(1.0) - dec!(0.005));
        let mut usd_eur_rate = dec!(0.0);
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::USD);
                assert_eq!(swap_response.to, Currency::EUR);
                let swap_rate = swap_response.rate.map(|rate| rate.value());
                assert_eq!(swap_rate, Some(expected_usd_eur_rate));
                usd_eur_rate = swap_rate.expect("Valid rate expected");
                assert!(swap_response.error.is_none());
                break;
            }
        }

        let eur_balance = Money::new(Currency::EUR, usd_balance.value() * usd_eur_rate);
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: eur_balance,
            from: Currency::EUR,
            to: Currency::BTC,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        let mut eur_btc_rate = dec!(0.0);
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::EUR);
                assert_eq!(swap_response.to, Currency::BTC);
                let swap_rate = swap_response.rate.map(|rate| rate.value());
                assert_eq!(swap_rate, Some(Rate::normalized_value(dec!(1.0) / dec!(36180))));
                eur_btc_rate = swap_rate.expect("Valid rate expected");
                assert!(swap_response.error.is_none());
                break;
            }
        }

        let final_btc_balance = Money::new(Currency::BTC, eur_balance.value() * eur_btc_rate);

        assert!(final_btc_balance.value() > dec!(0));
        assert!(btc_balance.value() > final_btc_balance.value());
    }

    #[test]
    fn same_currency_swap() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let btc_balance = Money::new(Currency::BTC, dec!(0.0001));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: btc_balance,
            from: Currency::BTC,
            to: Currency::BTC,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::BTC);
                assert_eq!(swap_response.to, Currency::BTC);
                assert!(swap_response.rate.is_none());
                assert!(!swap_response.success);
                assert_eq!(swap_response.error, Some(SwapResponseError::Invalid));
                break;
            }
        }
        let usd_balance = Money::new(Currency::USD, dec!(1.0));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: usd_balance,
            from: Currency::USD,
            to: Currency::USD,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });

        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::USD);
                assert_eq!(swap_response.to, Currency::USD);
                assert!(swap_response.rate.is_none());
                assert!(!swap_response.success);
                assert_eq!(swap_response.error, Some(SwapResponseError::Invalid));
                break;
            }
        }

        let eur_balance = Money::new(Currency::EUR, dec!(1.0));
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: eur_balance,
            from: Currency::EUR,
            to: Currency::EUR,
            quote_id: None,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });

        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, uid);
                assert_eq!(swap_response.from, Currency::EUR);
                assert_eq!(swap_response.to, Currency::EUR);
                assert!(swap_response.rate.is_none());
                assert!(!swap_response.success);
                assert_eq!(swap_response.error, Some(SwapResponseError::Invalid));
                break;
            }
        }
    }
}
