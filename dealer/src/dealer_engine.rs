use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

use msgs::api::{Api, QuoteResponse, QuoteResponseError, SwapRequest, SwapResponse, SwapResponseError, AvailableCurrenciesResponse, InvoiceResponse, InvoiceResponseError, PaymentResponse, PaymentResponseError};
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
use utils::time::time_now;
use utils::currencies::get_base_currency_from_symbol;
use uuid::Uuid;
use utils::xlogging::{LoggingSettings, init_log};

const QUOTE_TTL_MS: u64 = 5000;

pub struct HedgeSettings {
    // The amount of unhedged value to tolerate before a an adjustment.
    pub max_exposure: Option<u64>,
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
    is_kollider_authenticated: bool,
    logger: slog::Logger,
    pub last_bank_state: Option<BankState>,
    pub last_bank_state_update: Option<Instant>,
}

impl DealerEngine {
    pub fn new(settings: DealerEngineSettings, ws_client: impl WsClient + 'static) -> Self {
        let mut settings = settings.clone();

        let risk_tolerances = settings.risk_tolerances.into_iter().map(|(c, r)| {
            let currency = Currency::from_str(&c).unwrap();
            (currency, r)
        }).collect::<HashMap<Currency, u64>>();
        
        settings.logging_settings.name = String::from("Dealer");
        let logger = init_log(&settings.logging_settings);

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
            is_kollider_authenticated: false,
            guaranteed_quotes: BTreeMap::new(),
            last_bank_state_update: None,
            logger,
        }
    }

    pub fn check_has_received_initial_data(&mut self) {
        if self.has_received_symbols && self.is_kollider_authenticated {
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
            let sat_balance = balances.cash.get(&Symbol::from("SAT")).unwrap();
            if *sat_balance > dec!(1000) {
                let msg = Message::Dealer(Dealer::CreateInvoiceRequest(CreateInvoiceRequest {
                    req_id: Uuid::new_v4(),
                    amount: sat_balance.to_i64().unwrap() as u64,
                }));
                listener(msg);
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
                let quote = Currency::from_str(&symbol[3..6]).unwrap();
                if base == "BTC" {
                    Some(quote)
                } else {
                    None
                }
            })
            .collect::<HashSet<Currency>>();

        let mut available_currencies = available_currencies.into_iter().collect::<Vec<_>>();
        available_currencies.push(Currency::from_str("BTC").unwrap());

        let status = if is_authenticated {
            HealthStatus::Running
        } else {
            HealthStatus::Down
        };

        let dealer_health = DealerHealth {
            status,
            available_currencies,
            timestamp: time_now(),
        };

        let msg = Message::Dealer(Dealer::Health(dealer_health));
        listener(msg);
    }

    pub fn check_risk<F: FnMut(Message)>(&mut self, bank_state: BankState, listener: &mut F) {
        slog::info!(self.logger, "Checking Risk.");

        if !self.has_received_init_data {
            slog::info!(self.logger, "Not received all data. Skip checking risk.");
            return
        }

        for (currency, exposure) in bank_state.total_exposures.clone().into_iter() {
            if currency == Currency::BTC {
                continue;
            }

            slog::info!(self.logger, "{:?}", bank_state);

            let symbol = Symbol::from(currency);
            let denom = Denom::from_currency(currency);

            let qty_contracts_required =
                match self.calc_num_contracts_for_value(exposure, symbol.clone(), denom) {
                    Ok(q) => dec!(-1) * q,
                    Err(_) => continue,
                };

            slog::info!(self.logger, "Target number of cotracts: {}", qty_contracts_required);

            let currently_hedged_qty = match self.ws_client.get_position_state(&symbol) {
                Some(p) => match p.side {
                    None => dec!(0),
                    Some(side) => {
                        let side_sign = Decimal::new(side.to_sign(), 0);
                        side_sign * p.quantity
                    }
                },
                None => dec!(0),
            };

            slog::info!(self.logger, "Current number of cotracts: {}", currently_hedged_qty);

            // If negative we need to sell more and if positive we need to buy more.
            // This works under the assumption that qty_contracts_required is <= 0.
            let delta_qty = qty_contracts_required - currently_hedged_qty;

            let risk_tolerance = match  self.risk_tolerances.get(&currency) {
                Some(t) => t,
                None => continue
            };

            if delta_qty.abs() < Decimal::new(*risk_tolerance as i64, 0) {
                slog::info!(self.logger, "Deltaa qty of {} within risk tolerance of {}. NO ACTION.", delta_qty, risk_tolerance);
                continue;
            }

            let trade_side = Side::from_sign(delta_qty.to_i64().unwrap());

            slog::info!(self.logger, "Placing trade on side: {:?} of qty: {} for symbol: {}", trade_side, delta_qty, symbol);

            self.ws_client
                .make_order(delta_qty.abs().to_i64().unwrap() as u64, symbol, trade_side)
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
                    };
                    let time_now = SystemTime::now();
                    let invalidated_quotes = time_now
                        .sub(Duration::from_millis(QUOTE_TTL_MS))
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_micros();
                    self.guaranteed_quotes = self.guaranteed_quotes.split_off(&invalidated_quotes);

                    match swap_request.quote_id {
                        None => {
                            let conversion_info = ConversionInfo::new(swap_request.from, swap_request.to);
                            let rate = self.get_rate(Some(swap_request.amount), None, conversion_info);
                            if rate.is_some() {
                                swap_response.rate = rate;
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
                                Ok(_) => swap_response.rate = quote.rate,
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
                    };
                    let conversion_info = ConversionInfo::new(quote_request.from, quote_request.to);
                    let rate = self.get_rate(Some(quote_request.amount), None, conversion_info);
                    if rate.is_some() {
                        let time_now = SystemTime::now();
                        let quote_id = time_now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_micros();
                        let valid_until = time_now
                            .add(Duration::from_millis(QUOTE_TTL_MS))
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        quote_response.quote_id = Some(quote_id);
                        quote_response.rate = rate;
                        quote_response.valid_until = valid_until;
                        self.guaranteed_quotes.insert(quote_id, quote_response.clone());
                    } else {
                        quote_response.error = Some(QuoteResponseError::CurrencyNotAvailable);
                    }
                    let msg = Message::Api(Api::QuoteResponse(quote_response));
                    listener(msg);
                }
                Api::AvailableCurrenciesRequest(available_currencies_request) => {

                    let tradable_symbols = self.ws_client.get_tradable_symbols();
                    let mut currencies = tradable_symbols.into_iter().map(|(s, _)| {
                        let base_currency = get_base_currency_from_symbol(s).unwrap();
                        base_currency
                    }).collect::<HashSet<Currency>>().into_iter().map(|c| {
                        c
                    }).collect::<Vec<Currency>>();
                    currencies.push(Currency::from_str("BTC").unwrap());

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
                    let rate = self.get_rate(None, Some(invoice_request.amount), conversion_info);
                    let mut invoice_response = InvoiceResponse {
                        rate: None,
                        amount: invoice_request.amount,
                        req_id: invoice_request.req_id,
                        uid: invoice_request.uid,
                        meta: invoice_request.meta,
                        currency: invoice_request.currency,
                        payment_request: None,
                        account_id: None,
                        error: None,
                    };
                    if rate.is_none() {
                        invoice_response.error = Some(InvoiceResponseError::RateNotAvailable);
                    } else {
                        invoice_response.rate = rate
                    }
                    let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                    listener(msg);
                }
                Api::PaymentRequest(mut msg) => {
                    let conversion_info = ConversionInfo::new(msg.currency, Currency::BTC);
                    // We assume user specifies the value not the amount.
                    let amount = msg.amount.unwrap();
                    let rate = self.get_rate(None, Some(amount), conversion_info);
                    if rate.is_none() {
                        return
                    } else {
                        msg.rate = rate;
                    }
                    let msg = Message::Api(Api::PaymentRequest(msg));
                    listener(msg);
                }
                Api::CreateLnurlWithdrawalRequest(mut msg) => {
                    let conversion_info = ConversionInfo::new(msg.currency, Currency::BTC);
                    // We assume user specifies the value not the amount.
                    let amount = msg.amount;
                    let rate = self.get_rate(None, Some(amount), conversion_info);
                    if rate.is_none() {
                        return
                    } else {
                        msg.rate = rate;
                    }
                    let msg = Message::Api(Api::CreateLnurlWithdrawalRequest(msg));
                    listener(msg);
                }
                _ => {}
            },
            Message::KolliderApiResponse(msg) => {
                match msg {
                    KolliderApiResponse::Authenticate(auth) => {
                        if auth.success() {
                            slog::info!(self.logger, "Successful Kollider authenticated!");
                            self.is_kollider_authenticated = true;
                            self.check_has_received_initial_data();
                        }
                        self.ws_client.subscribe(
                            vec![Channel::PositionStates],
                            None,
                        );
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
                        let msg = Message::Dealer(Dealer::CreateInvoiceRequest(CreateInvoiceRequest {
                            req_id: Uuid::new_v4(),
                            amount: settlement_request.amount.parse().unwrap(),
                        }));
                        listener(msg);
                    }
                    KolliderApiResponse::PositionStates(position_states) => {
                        slog::info!(self.logger, "Position states: {:?}", position_states);
                        if let Some(_last_bank_state) = self.last_bank_state.clone() {
                            // self.check_risk(last_bank_state, listener);
                        }
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
                        self.ws_client.subscribe(
                            vec![Channel::MarkPrices, Channel::OrderbookLevel2],
                            Some(available_symbols)
                        );
                    }
                    _ => {}
                }
            }
            Message::Dealer(Dealer::BankState(bank_state)) => {
                self.last_bank_state_update = Some(Instant::now());
                self.last_bank_state = Some(bank_state.clone());
                self.check_risk(bank_state, listener);
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
                let rate = self.get_rate(None, Some(msg.amount), conversion_info);

                let mut fiat_deposit_response = FiatDepositResponse {
                    req_id: msg.req_id,
                    uid: msg.uid,
                    amount: msg.amount,
                    currency: msg.currency,
                    rate: None,
                    error: None,
                };

                if rate.is_none() {
                    fiat_deposit_response.error = Some(FiatDepositResponseError::CurrencyNotAvailable);
                } else {
                    fiat_deposit_response.rate = rate;
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
            None => return
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

    fn get_rate(&self, amount: Option<Decimal>, value: Option<Decimal>, conversion_info: ConversionInfo) -> Option<Decimal> {
        let maybe_quotes = match conversion_info.side {
            Side::Bid => self.bid_quotes.get(&conversion_info.symbol),
            Side::Ask => self.ask_quotes.get(&conversion_info.symbol),
        };
        // Either vlaue or amount needs to be some.
        if amount.is_none() && value.is_none() {
            return None
        }
        match maybe_quotes {
            None => None,
            Some(quotes) => {
                let best_price = if conversion_info.from != conversion_info.quote {
                    *quotes.range(0..u64::MAX).next().unwrap().1
                } else {
                    dec!(1.0)
                };
                let value = if let Some(v) = value {
                    v.round_dp_with_strategy(0, RoundingStrategy::AwayFromZero)
                } else {
                    let value = amount.unwrap() * best_price;
                    value.round_dp_with_strategy(0, RoundingStrategy::AwayFromZero)
                };
                let lookup_quantity = value.to_i64().unwrap() as u64;
                match quotes.range(lookup_quantity..u64::MAX).next() {
                    None => None,
                    Some((_level_vol, price)) => {
                        if conversion_info.base == conversion_info.from {
                            Some(*price * dec!(0.995))
                        } else {
                            Some(dec!(1) / (price * dec!(1.005)))
                        }
                    }
                }
            }
        }
    }
}

fn validate_quote(quote: &QuoteResponse, swap_request: &SwapRequest) -> Result<(), ()> {
    if quote.from != swap_request.from
        || quote.to != swap_request.to
        || quote.amount != swap_request.amount
        || quote.uid != swap_request.uid
    {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use msgs::kollider_client::Channel;

    struct MockWsClient {
        is_authenticated: bool,
        position_states: HashMap<Symbol, PositionState>,
        mark_prices: HashMap<Symbol, MarkPrice>,
        balances: RefCell<Balances>,
        tradable_symbols: HashMap<Symbol, TradableSymbol>,
    }

    impl MockWsClient {
        pub fn new() -> Self {
            Self {
                is_authenticated: true,
                position_states: Default::default(),
                mark_prices: Default::default(),
                balances: RefCell::new(Balances {
                    cash: Default::default(),
                    isolated_margin: Default::default(),
                    order_margin: Default::default(),
                    cross_margin: Default::default(),
                }),
                tradable_symbols: Default::default(),
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
                Ok(self.balances.borrow().cash)
            }
        }

        fn subscribe(&self, channels: Vec<Channel>, symbols: Option<Vec<Symbol>>) {
        }

        fn get_all_balances(&self) -> Option<Balances> {
            Some(self.balances.borrow().clone())
        }

        fn get_position_state(&self, symbol: &Symbol) -> Option<PositionState> {
            self.position_states.get(symbol).cloned()
        }

        fn get_tradable_symbols(&self) -> HashMap<Symbol, TradableSymbol> {
            self.tradable_symbols.clone()
        }

        fn make_withdrawal(&self, amount: u64, _payment_request: String) -> ws_client::Result<()> {
            self.balances.borrow_mut().cash -= Decimal::new(amount as i64, 0);
            Ok(())
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
    use uuid::Uuid;
    use ws_client::WsClient;
    use xerror::kollider_client::KolliderClientError;
    use utils::xlogging::*;

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
                level: String::from("debug"),
                stdout: false,
                log_path: None
            }
        };
        let ws_client = MockWsClient::new();
        let mut dealer = DealerEngine::new(settings, ws_client);
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
        dealer
    }

    #[test]
    fn rate_first_bucket() {
        let mut dealer_engine = initialise_dealer_engine();
        let mut out_msg = VecDeque::new();
        let uid = 1003;
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.001),
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
                assert_eq!(quote_response.rate, Some(dec!(4000.0)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(10.0),
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
                assert_eq!(quote_response.rate, Some(dec!(1) / dec!(3000.0)));
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(quote_response.rate, Some(dec!(5200.0)));
                assert!(quote_response.quote_id.is_some());
                assert!(quote_response.error.is_none());
                break;
            }
        }
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: Decimal::new(3500, 0),
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
                assert_eq!(quote_response.rate, Some(dec!(1) / dec!(1800.0)));
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(100000.0),
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(10.0),
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(10.0),
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.001),
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
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(swap_response.rate, Some(dec!(5200.0)));
                assert!(swap_response.error.is_none());
                break;
            }
        }
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: Decimal::new(3500, 0),
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
                assert_eq!(swap_response.rate, Some(dec!(1) / dec!(1800.0)));
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(quote_response.rate, Some(dec!(5200.0)));
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
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: Decimal::new(3500, 0),
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(quote_response.rate, Some(dec!(5200.0)));
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(quote_response.rate, Some(dec!(5200.0)));
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
        // modified amount
        let swap_request = SwapRequest {
            req_id: Uuid::new_v4(),
            uid: quote.uid,
            amount: dec!(0.555),
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
            from: Currency::GBP,
            to: Currency::BTC,
            quote_id: quote.quote_id,
        };
        dealer_engine.process_msg(Message::Api(Api::SwapRequest(swap_request)), &mut |msg| {
            out_msg.push_back(msg);
        });
        while let Some(msg) = out_msg.pop_front() {
            if let Message::Api(Api::SwapResponse(swap_response)) = msg {
                assert_eq!(swap_response.uid, quote.uid);
                assert_eq!(swap_response.from, Currency::GBP);
                assert_eq!(swap_response.to, Currency::BTC);
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
        let quote_request = QuoteRequest {
            req_id: Uuid::new_v4(),
            uid,
            amount: dec!(0.875),
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
                assert_eq!(quote_response.rate, Some(dec!(5200.0)));
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
}
