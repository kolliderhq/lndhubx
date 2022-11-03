use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use bigdecimal::BigDecimal;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

use core_types::*;
use diesel::result::Error as DieselError;
use models::{accounts, invoices::Invoice, users::User};

use msgs::api::*;
use msgs::bank::*;
use msgs::dealer::*;
use msgs::*;
use std::iter::Iterator;
use utils::currencies::{SATS_DECIMALS, SATS_IN_BITCOIN};
use utils::xlogging::*;
use xerror::bank_engine::*;

use futures::stream::FuturesUnordered;
use lnd_connector::connector::{LndConnector, LndConnectorSettings};

use msgs::cli::{Cli, MakeTx, MakeTxResult};
use serde::{Deserialize, Serialize};

use crate::ledger::*;

const BANK_UID: u64 = 23193913;
const DEALER_UID: u64 = 52172712;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RateLimiterSettings {
    pub request_limit: u64,
    pub replenishment_interval: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BankEngineSettings {
    /// url to the postgres database.
    pub psql_url: String,
    pub bank_zmq_pull_address: String,
    pub bank_zmq_publish_address: String,
    pub bank_dealer_pull_address: String,
    pub bank_dealer_push_address: String,
    /// The margin users have to keep on their account to account for network fees.
    pub ln_network_fee_margin: Decimal,
    pub ln_network_max_fee: Decimal,
    pub internal_tx_fee: Decimal,
    pub external_tx_fee: Decimal,
    pub reserve_ratio: Decimal,
    pub withdrawal_only: bool,
    pub logging_settings: LoggingSettings,
    pub deposit_limits: HashMap<String, Decimal>,
    pub influx_host: String,
    pub influx_org: String,
    pub influx_bucket: String,
    pub influx_token: String,
    pub bank_cli_resp_address: String,
    pub withdrawal_request_rate_limiter_settings: RateLimiterSettings,
    pub deposit_request_rate_limiter_settings: RateLimiterSettings,
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FeeStructure {
    _internal_tx_fee: Decimal,
    _external_tx_fee: Decimal,
    _conversion_fee: Decimal,
}

impl FeeStructure {
    pub fn new() -> Self {
        Self {
            _internal_tx_fee: dec!(0),
            _external_tx_fee: dec!(0),
            _conversion_fee: dec!(0),
        }
    }
}

impl Default for FeeStructure {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BankEngine {
    pub bank_uid: UserId,
    /// Bank state.
    pub ledger: Ledger,
    /// Fees structure of bank operation.
    pub fee_structure: FeeStructure,
    /// Connection to the postgres DB.
    pub conn_pool: Option<DbPool>,
    pub lnd_connector: LndConnector,
    pub lnd_node_info: LndNodeInfo,
    pub available_currencies: Vec<Currency>,
    pub ln_network_fee_margin: Decimal,
    pub ln_network_max_fee: Decimal,
    pub internal_tx_fee: Decimal,
    pub external_tx_fee: Decimal,
    pub reserve_ratio: Decimal,
    pub withdrawal_only: bool,
    pub deposit_limits: HashMap<Currency, Decimal>,
    pub logger: slog::Logger,
    pub tx_seq: u64,
    pub lnurl_withdrawal_requests: HashMap<Uuid, (u64, PaymentRequest)>,
    pub payment_thread_sender: crossbeam_channel::Sender<Message>,
    pub lnd_connector_settings: LndConnectorSettings,
    pub payment_threads: FuturesUnordered<tokio::task::JoinHandle<()>>,
    pub withdrawal_request_rate_limiter_settings: RateLimiterSettings,
    pub deposit_request_rate_limiter_settings: RateLimiterSettings,
    pub withdrawal_request_rate_limiter: HashMap<UserId, (u64, Instant)>,
    pub deposit_request_rate_limiter: HashMap<UserId, (u64, Instant)>,
}

impl BankEngine {
    pub async fn new(
        conn_pool: Option<DbPool>,
        lnd_connector: LndConnector,
        mut settings: BankEngineSettings,
        lnd_connector_settings: LndConnectorSettings,
        payment_thread_sender: crossbeam_channel::Sender<Message>,
    ) -> Self {
        settings.logging_settings.name = String::from("Bank");
        let logger = init_log(&settings.logging_settings);

        Self {
            lnd_node_info: LndNodeInfo::default(),
            bank_uid: BANK_UID,
            ledger: Ledger::new(),
            fee_structure: FeeStructure::new(),
            conn_pool,
            lnd_connector,
            available_currencies: vec![Currency::BTC],
            internal_tx_fee: settings.internal_tx_fee,
            external_tx_fee: settings.external_tx_fee,
            ln_network_fee_margin: settings.ln_network_fee_margin,
            reserve_ratio: settings.reserve_ratio,
            ln_network_max_fee: settings.ln_network_max_fee,
            withdrawal_only: settings.withdrawal_only,
            deposit_limits: settings
                .deposit_limits
                .into_iter()
                .map(|(currency, limit)| {
                    (
                        Currency::from_str(&currency)
                            .unwrap_or_else(|_| panic!("Failed to convert {} into a valid currency", currency)),
                        limit,
                    )
                })
                .collect::<HashMap<Currency, Decimal>>(),
            logger,
            tx_seq: 0,
            lnurl_withdrawal_requests: HashMap::new(),
            payment_threads: FuturesUnordered::new(),
            withdrawal_request_rate_limiter_settings: settings.withdrawal_request_rate_limiter_settings,
            deposit_request_rate_limiter_settings: settings.deposit_request_rate_limiter_settings,
            withdrawal_request_rate_limiter: HashMap::new(),
            deposit_request_rate_limiter: HashMap::new(),
            payment_thread_sender,
            lnd_connector_settings,
        }
    }

    fn check_deposit_request_rate_limit(&mut self, user_id: UserId) -> bool {
        let (counter, last_request) = self
            .deposit_request_rate_limiter
            .entry(user_id)
            .or_insert_with(|| (0, Instant::now()));
        if (last_request.elapsed().as_millis() as u64)
            < self.deposit_request_rate_limiter_settings.replenishment_interval
        {
            *counter += 1;
            if *counter > self.deposit_request_rate_limiter_settings.request_limit {
                return false;
            }
        } else {
            *counter = 0;
            *last_request = Instant::now();
        }
        return true;
    }

    fn check_withdrawal_request_rate_limit(&mut self, user_id: UserId) -> bool {
        let (counter, last_request) = self
            .withdrawal_request_rate_limiter
            .entry(user_id)
            .or_insert_with(|| (0, Instant::now()));
        if (last_request.elapsed().as_millis() as u64)
            < self.withdrawal_request_rate_limiter_settings.replenishment_interval
        {
            *counter += 1;
            if *counter > self.withdrawal_request_rate_limiter_settings.request_limit {
                return false;
            }
        } else {
            *counter = 0;
            *last_request = Instant::now();
        }
        return true;
    }

    fn fetch_internal_user_account<F: FnMut(&diesel::PgConnection) -> Result<Vec<accounts::Account>, DieselError>>(
        &mut self,
        conn: &diesel::PgConnection,
        fetcher: &mut F,
    ) -> Account {
        let account = match fetcher(conn) {
            Ok(mut internal_accounts) => {
                let internal_accounts_count = internal_accounts.len();
                if internal_accounts_count != 1 {
                    slog::error!(
                        self.logger,
                        "Exactly one Internal user's BTC account should exist - found {}",
                        internal_accounts_count
                    );
                    panic!(
                        "Exactly one Internal user's BTC account should exist - found {}",
                        internal_accounts_count
                    );
                }
                match internal_accounts.pop() {
                    Some(account) => account,
                    None => {
                        panic!("Exactly one account expected due to previous check. Unexpected failure");
                    }
                }
            }
            Err(err) => {
                slog::error!(
                    self.logger,
                    "Could not initialise internal user account, reason {:?}",
                    err
                );
                panic!("Could not initialise internal user account, reason {:?}", err);
            }
        };

        let currency = match Currency::from_str(&account.currency) {
            Ok(converted) => converted,
            Err(err) => {
                panic!(
                    "Failed to convert {} to a valid currency, error: {:?}",
                    account.currency, err
                );
            }
        };
        let balance = match Decimal::from_str(&account.balance.to_string()) {
            Ok(converted) => converted,
            Err(err) => {
                panic!(
                    "Failed to convert {} to a valid balance, error: {:?}",
                    account.balance, err
                );
            }
        };
        let account_type = match AccountType::from_str(&account.account_type) {
            Ok(converted) => converted,
            Err(err) => {
                panic!(
                    "Failed to convert {} to a valid account type, error: {:?}",
                    account.account_type, err
                );
            }
        };
        let account_id = account.account_id;
        Account {
            account_id,
            balance,
            currency,
            account_type,
        }
    }

    fn fetch_external_account(&mut self, conn: &diesel::PgConnection) -> Account {
        self.fetch_internal_user_account(conn, &mut accounts::Account::get_bank_btc_accounts)
    }

    fn fetch_insurance_account(&mut self, conn: &diesel::PgConnection) -> Account {
        self.fetch_internal_user_account(conn, &mut accounts::Account::get_dealer_btc_accounts)
    }

    fn is_insurance_fund_depleted(&self) -> bool {
        self.ledger.insurance_fund_account.balance < Decimal::new(10, SATS_DECIMALS)
    }

    pub fn init_accounts(&mut self) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                slog::error!(self.logger, "No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                slog::error!(self.logger, "Couldn't get psql connection.");
                return;
            }
        };

        let insurance_account = self.fetch_insurance_account(&c);
        self.ledger.insurance_fund_account = insurance_account;

        let external_account = self.fetch_external_account(&c);
        self.ledger.external_account = external_account;

        let accounts = match accounts::Account::get_non_internal_users_accounts(&c) {
            Ok(accs) => accs,
            Err(_) => return,
        };

        for account in accounts {
            let user_account = self
                .ledger
                .user_accounts
                .entry(account.uid as u64)
                .or_insert_with(|| UserAccount::new(account.uid as u64));
            let currency = match Currency::from_str(&account.currency) {
                Ok(converted) => converted,
                Err(err) => {
                    panic!(
                        "Failed to convert {} to a valid currency, error: {:?}",
                        account.currency, err
                    );
                }
            };
            let balance = match Decimal::from_str(&account.balance.to_string()) {
                Ok(converted) => converted,
                Err(err) => {
                    panic!(
                        "Failed to convert {} to a valid balance, error: {:?}",
                        account.balance, err
                    );
                }
            };
            let account_type = match AccountType::from_str(&account.account_type) {
                Ok(converted) => converted,
                Err(err) => {
                    panic!(
                        "Failed to convert {} to a valid account type, error: {:?}",
                        account.account_type, err
                    );
                }
            };
            let account_id = account.account_id;
            let acc = Account {
                currency,
                balance,
                account_id,
                account_type,
            };

            user_account.accounts.insert(account.account_id, acc);
        }
    }

    pub fn get_bank_state(&self) -> BankState {
        let mut total_exposures = HashMap::new();

        for (_, user_account) in self.ledger.user_accounts.clone().into_iter() {
            for (_, account) in user_account.accounts.into_iter() {
                let mut currency_exposure = total_exposures.entry(account.currency).or_insert(dec!(0));
                currency_exposure += account.balance;
            }
        }

        BankState {
            total_exposures,
            insurance_fund_account: self.ledger.insurance_fund_account.clone(),
            external_account: self.ledger.external_account.clone(),
        }
    }

    fn insert_into_ledger(&mut self, uid: &UserId, account_id: AccountId, account: Account) {
        if let Some(user_account) = self.ledger.user_accounts.get_mut(uid) {
            user_account.accounts.insert(account_id, account);
        } else {
            panic!(
                "Failed to find user account, uid: {} while inserting account state: account_id: {}, account: {:?}",
                uid, account_id, account
            );
        }
    }

    pub fn update_account(&mut self, account: &Account, uid: UserId) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                slog::error!(self.logger, "No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                slog::error!(self.logger, "Couldn't get psql connection.");
                return;
            }
        };

        // Oh lord forgive me for this.
        let balance_str = account.balance.to_string();
        let big_decimal = match BigDecimal::from_str(&balance_str) {
            Ok(d) => d,
            Err(_) => {
                dbg!("couldn't parse big int");
                return;
            }
        };
        let update_account = accounts::UpdateAccount {
            account_id: account.account_id,
            balance: Some(big_decimal.clone()),
            currency: account.currency.to_string(),
            account_type: None,
            uid: None,
        };
        if let Ok(res) = update_account.update(&c, account.account_id) {
            if res == 0 {
                let insertable_account = accounts::InsertableAccount {
                    account_id: account.account_id,
                    balance: Some(big_decimal),
                    currency: account.currency.to_string(),
                    uid: uid as i32,
                    account_type: account.account_type.to_string(),
                };
                if insertable_account.insert(&c).is_err() {
                    dbg!("Error inserting!");
                }
            }
        }
    }

    /// Double entry transaction logic.
    pub fn make_tx(
        &mut self,
        outbound_account: &mut Account,
        outbound_uid: u64,
        inbound_account: &mut Account,
        inbound_uid: u64,
        amount: Decimal,
        rate: Decimal,
    ) -> Result<(), BankError> {
        if amount <= dec!(0) {
            return Err(BankError::FailedTransaction);
        }

        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                slog::error!(self.logger, "No database provided.");
                return Err(BankError::FailedTransaction);
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                slog::error!(self.logger, "Couldn't get psql connection.");
                return Err(BankError::FailedTransaction);
            }
        };

        let outbound_amount = amount;
        let inbound_amount = amount * rate;

        outbound_account.balance -= outbound_amount;
        inbound_account.balance += inbound_amount;

        let outbound_amount_str = outbound_amount.to_string();
        let inbound_amount_str = inbound_amount.to_string();
        let rate_str = rate.to_string();

        let outbound_amount_bigdec = match BigDecimal::from_str(&outbound_amount_str) {
            Ok(d) => d,
            Err(_) => {
                dbg!("couldn't parse big decimal");
                return Err(BankError::FailedTransaction);
            }
        };

        let inbound_amount_bigdec = match BigDecimal::from_str(&inbound_amount_str) {
            Ok(d) => d,
            Err(_) => {
                dbg!("couldn't parse big decimal");
                return Err(BankError::FailedTransaction);
            }
        };

        let rate_bigdec = match BigDecimal::from_str(&rate_str) {
            Ok(d) => d,
            Err(_) => {
                dbg!("couldn't parse big decimal");
                return Err(BankError::FailedTransaction);
            }
        };

        let tx_type = if outbound_account.account_type != inbound_account.account_type {
            String::from("External")
        } else {
            String::from("Internal")
        };

        let t = utils::time::time_now();
        self.tx_seq += 1;
        let txid = format!("{}-{}", t, self.tx_seq);

        let tx = models::transactions::Transaction {
            txid,
            outbound_uid: outbound_uid as i32,
            inbound_uid: inbound_uid as i32,
            created_at: t as i64,
            outbound_amount: outbound_amount_bigdec,
            inbound_amount: inbound_amount_bigdec,
            outbound_account_id: outbound_account.account_id,
            inbound_account_id: inbound_account.account_id,
            outbound_currency: outbound_account.currency.to_string(),
            inbound_currency: inbound_account.currency.to_string(),
            exchange_rate: rate_bigdec,
            tx_type,
        };

        if tx.insert(&c).is_err() {
            return Err(BankError::FailedTransaction);
        }

        Ok(())
    }

    pub fn make_internal_tx<F: FnMut(Message, ServiceIdentity)>(
        &mut self,
        payment_request: PaymentRequest,
        listener: &mut F,
    ) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                slog::error!(self.logger, "No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                slog::error!(self.logger, "Couldn't get psql connection.");
                return;
            }
        };

        let username = payment_request
            .receipient
            .as_ref()
            .unwrap_or_else(|| panic!("Recipient's username not specified: {:?}", payment_request))
            .clone();
        let outbound_uid = payment_request.uid;
        let amount = *payment_request
            .amount
            .as_ref()
            .unwrap_or_else(|| panic!("Amount not specified: {:?}", payment_request));
        let rate = dec!(1);

        let mut payment_response = PaymentResponse {
            amount,
            payment_hash: Uuid::new_v4().to_string(),
            req_id: payment_request.req_id,
            uid: outbound_uid,
            success: false,
            payment_request: None,
            currency: payment_request.currency,
            fees: dec!(0),
            error: None,
            rate,
        };

        let inbound_user = match User::get_by_username(&c, username) {
            Ok(u) => u,
            Err(_) => {
                payment_response.error = Some(PaymentResponseError::UserDoesNotExist);
                let msg = Message::Api(Api::PaymentResponse(payment_response));
                listener(msg, ServiceIdentity::Api);
                return;
            }
        };

        let inbound_uid = inbound_user.uid as u64;
        if inbound_uid == outbound_uid {
            slog::error!(self.logger, "User tried to send to self via username.");
            return;
        }

        let mut outbound_account = {
            let user_account = match self.ledger.user_accounts.get_mut(&outbound_uid) {
                Some(ua) => ua,
                None => return,
            };

            user_account.get_default_account(payment_request.currency)
        };

        let mut inbound_account = {
            let user_account = self
                .ledger
                .user_accounts
                .entry(inbound_uid)
                .or_insert_with(|| UserAccount::new(inbound_uid));
            user_account.get_default_account(payment_request.currency)
        };

        if outbound_account.balance < amount {
            payment_response.error = Some(PaymentResponseError::InsufficientFunds);
            let msg = Message::Api(Api::PaymentResponse(payment_response));
            listener(msg, ServiceIdentity::Api);
            return;
        }

        if self
            .make_tx(
                &mut outbound_account,
                outbound_uid,
                &mut inbound_account,
                inbound_uid,
                amount,
                rate,
            )
            .is_err()
        {
            return;
        }

        self.insert_into_ledger(&inbound_uid, inbound_account.account_id, inbound_account.clone());
        self.insert_into_ledger(&outbound_uid, outbound_account.account_id, outbound_account.clone());

        // Update DB.
        self.update_account(&outbound_account, outbound_uid);
        self.update_account(&inbound_account, inbound_uid);

        payment_response.success = true;
        let msg = Message::Api(Api::PaymentResponse(payment_response));
        listener(msg, ServiceIdentity::Api);
    }

    pub async fn process_msg<F: FnMut(Message, ServiceIdentity)>(&mut self, msg: Message, listener: &mut F) {
        match msg {
            Message::Dealer(msg) => match msg {
                Dealer::Health(dealer_health) => {
                    self.available_currencies = dealer_health.available_currencies;
                    if dealer_health.status == HealthStatus::Down || self.is_insurance_fund_depleted() {
                        if dealer_health.status == HealthStatus::Down {
                            slog::warn!(self.logger, "Dealer is disconnected from the exchange!");
                        }
                        self.available_currencies = Vec::new();
                    }
                }
                Dealer::BankStateRequest(_) => {
                    let bank_state = self.get_bank_state();
                    let msg = Message::Dealer(Dealer::BankState(bank_state));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Dealer::PayInvoice(pay_invoice) => {
                    self.process_pay_invoice(pay_invoice, false).await;
                }
                Dealer::PayInsuranceInvoice(pay_invoice) => {
                    self.process_pay_invoice(pay_invoice, true).await;
                }
                Dealer::CreateInvoiceRequest(req) => {
                    slog::info!(self.logger, "Dealer requested payment: {:?}", req);
                    self.process_create_invoice_request(req, BANK_UID, listener).await;
                }
                Dealer::CreateInsuranceInvoiceRequest(req) => {
                    slog::info!(self.logger, "Dealer requested insurance payment: {:?}", req);
                    self.process_create_invoice_request(req, DEALER_UID, listener).await;
                }
                Dealer::FiatDepositResponse(msg) => {
                    slog::info!(self.logger, "Received fiat deposit response: {:?}", msg);
                    if msg.error.is_some() {
                        return;
                    }
                    let rate = match msg.rate {
                        Some(r) => r,
                        None => return,
                    };

                    let (mut inbound_account, inbound_uid) = {
                        let user_account = self
                            .ledger
                            .user_accounts
                            .entry(msg.uid as u64)
                            .or_insert_with(|| UserAccount::new(msg.uid as u64));

                        let account = user_account.get_default_account(msg.currency);

                        (account, user_account.owner)
                    };

                    let mut external_account = self.ledger.external_account.clone();
                    let value = msg.amount;

                    // Making the transaction and inserting it into the DB.
                    if self
                        .make_tx(
                            &mut external_account,
                            BANK_UID,
                            &mut inbound_account,
                            inbound_uid,
                            value,
                            rate,
                        )
                        .is_err()
                    {
                        return;
                    }

                    self.insert_into_ledger(&inbound_uid, inbound_account.account_id, inbound_account.clone());

                    // Updating cache of external account.
                    self.ledger.external_account = external_account.clone();

                    // Updating db of internal account.
                    self.update_account(&inbound_account, inbound_uid);

                    // Updating db of internal account.
                    self.update_account(&external_account, BANK_UID);

                    let bank_state = self.get_bank_state();
                    let msg = Message::Dealer(Dealer::BankState(bank_state));
                    listener(msg, ServiceIdentity::Dealer);
                }
                _ => {}
            },
            Message::Deposit(msg) => {
                slog::warn!(self.logger, "Received deposit: {:?}", msg);
                // Deposit can only be triggered if someone external has payed an invoice generated by someone internal.
                let conn = match &self.conn_pool {
                    Some(conn) => conn,
                    None => {
                        slog::error!(self.logger, "No database provided.");
                        return;
                    }
                };

                let c = match conn.get() {
                    Ok(psql_connection) => psql_connection,
                    Err(_) => {
                        slog::error!(self.logger, "Couldn't get psql connection.");
                        return;
                    }
                };

                // Check whether we know about this invoice.
                if let Ok(invoice) = Invoice::get_by_invoice_hash(&c, msg.payment_request) {
                    // Value of the depoist.
                    let value = Decimal::new(invoice.value as i64, SATS_DECIMALS);
                    // Exchange rate of the deposit.
                    let rate = dec!(1);

                    let currency = match invoice.currency {
                        Some(c) => match Currency::from_str(&c) {
                            Ok(converted) => converted,
                            Err(err) => {
                                panic!("Failed to convert {} into a valid currency, reason: {:?}", c, err);
                            }
                        },
                        None => Currency::BTC,
                    };

                    // If user wants to deposit into a fiat account.
                    let target_account_currency = match invoice.target_account_currency {
                        Some(c) => match Currency::from_str(&c) {
                            Ok(converted) => converted,
                            Err(err) => {
                                panic!("Failed to convert {} into a valid currency, reason: {:?}", c, err);
                            }
                        },
                        None => Currency::BTC,
                    };

                    // If its not a fiat deposit we need to get the current rate.
                    // Note a user could deposit with a sat specified invoice and then deposit into a fiat account.
                    if currency != Currency::BTC
                        || (currency == Currency::BTC && target_account_currency != Currency::BTC)
                    {
                        let c = if currency == Currency::BTC {
                            target_account_currency
                        } else {
                            currency
                        };
                        let fiat_deposit_request = FiatDepositRequest {
                            uid: invoice.uid as u64,
                            currency: c,
                            req_id: Uuid::new_v4(),
                            amount: value,
                        };
                        let msg = Message::Dealer(Dealer::FiatDepositRequest(fiat_deposit_request));
                        listener(msg, ServiceIdentity::Dealer);
                        return;
                    }

                    let is_insurance_invoice = invoice.uid as UserId == DEALER_UID;
                    let (mut inbound_account, inbound_uid) = if is_insurance_invoice {
                        slog::info!(self.logger, "Transferring deposit into insurance fund");
                        let insurance_account = self.ledger.insurance_fund_account.clone();
                        (insurance_account, DEALER_UID)
                    } else {
                        let user_account = self
                            .ledger
                            .user_accounts
                            .entry(invoice.uid as u64)
                            .or_insert_with(|| UserAccount::new(invoice.uid as u64));

                        let account = user_account.get_default_account(currency);

                        (account, user_account.owner)
                    };

                    let mut external_account = self.ledger.external_account.clone();

                    // Making the transaction and inserting it into the DB.
                    if self
                        .make_tx(
                            &mut external_account,
                            BANK_UID,
                            &mut inbound_account,
                            inbound_uid,
                            value,
                            rate,
                        )
                        .is_err()
                    {
                        return;
                    }

                    if is_insurance_invoice {
                        self.ledger.insurance_fund_account = inbound_account.clone();
                    } else {
                        // Safe to unwrap as we created this account above.
                        self.insert_into_ledger(&inbound_uid, inbound_account.account_id, inbound_account.clone());
                    }
                    // Updating cache of external account.
                    self.ledger.external_account = external_account.clone();

                    // Updating db of internal account.
                    self.update_account(&inbound_account, inbound_uid);

                    // Updating db of internal account.
                    self.update_account(&external_account, BANK_UID);
                }
            }
            Message::Api(msg) => match msg {
                Api::InvoiceRequest(msg) => {
                    slog::warn!(self.logger, "Received invoice request: {:?}", msg);

                    if !self.check_deposit_request_rate_limit(msg.uid) {
                        let invoice_response = InvoiceResponse {
                            amount: msg.amount,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            meta: msg.meta,
                            rate: None,
                            payment_request: None,
                            currency: msg.currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: None,
                            error: Some(InvoiceResponseError::RequestLimitExceeded),
                            fees: None,
                        };
                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    if self.is_insurance_fund_depleted() {
                        slog::warn!(self.logger, "Insurance is depleted Deposit request Failed!");
                        return;
                    }

                    let user_account = self
                        .ledger
                        .user_accounts
                        .entry(msg.uid)
                        .or_insert_with(|| UserAccount::new(msg.uid));

                    if self.withdrawal_only {
                        slog::info!(self.logger, "Bank is in withdrawal only mode");
                        let invoice_response = InvoiceResponse {
                            amount: msg.amount,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            meta: msg.meta,
                            rate: None,
                            payment_request: None,
                            currency: msg.currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: None,
                            error: Some(InvoiceResponseError::WithdrawalOnly),
                            fees: None,
                        };

                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            slog::error!(self.logger, "No database provided.");
                            return;
                        }
                    };

                    let c = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            slog::error!(self.logger, "Couldn't get psql connection.");
                            return;
                        }
                    };

                    let amount = msg.amount;
                    let currency = msg.currency;

                    let mut target_account = Account::new(msg.currency, AccountType::Internal);

                    if let Some(account_id) = msg.account_id {
                        if let Some(acc) = user_account.accounts.get(&account_id) {
                            target_account = acc.clone();
                        } else {
                            let invoice_response = InvoiceResponse {
                                amount,
                                req_id: msg.req_id,
                                uid: msg.uid,
                                rate: None,
                                meta: msg.meta.clone(),
                                payment_request: None,
                                currency: msg.currency,
                                target_account_currency: msg.target_account_currency,
                                account_id: Some(target_account.account_id),
                                error: Some(InvoiceResponseError::AccountDoesNotExist),
                                fees: None,
                            };
                            let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    } else {
                        // If user does not specify an account_id we select or create one for him.
                        let account = user_account.get_default_account(msg.currency);
                        target_account = account;
                    }

                    let deposit_limit = self
                        .deposit_limits
                        .get(&currency)
                        .unwrap_or_else(|| panic!("Failed to get deposit limits for {}", currency));
                    // Check whether deposit limit is exceeded.
                    if target_account.balance + amount > *deposit_limit {
                        let invoice_response = InvoiceResponse {
                            amount,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            rate: None,
                            meta: msg.meta.clone(),
                            payment_request: None,
                            currency: msg.currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: Some(target_account.account_id),
                            error: Some(InvoiceResponseError::DepositLimitExceeded),
                            fees: None,
                        };
                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    // If user wants to deposit another currency we have to go through the dealer.
                    if currency != Currency::BTC {
                        let msg = Message::Api(Api::InvoiceRequest(msg));
                        listener(msg, ServiceIdentity::Dealer);
                        return;
                    }

                    let amount_in_sats = (amount * Decimal::new(SATS_IN_BITCOIN as i64, 0))
                        .to_u64()
                        .unwrap_or_else(|| {
                            panic!(
                                "Failed to convert  decimal amount in BTC: {} to u64 amount in SATs",
                                amount
                            )
                        });

                    if let Ok(mut invoice) = self
                        .lnd_connector
                        .create_invoice(amount_in_sats, msg.meta.clone(), msg.uid, target_account.account_id)
                        .await
                    {
                        invoice.currency = Some(msg.currency.to_string());
                        if let Some(target_account_currency) = msg.target_account_currency {
                            invoice.target_account_currency = Some(target_account_currency.to_string());
                        } else {
                            invoice.target_account_currency = None
                        }
                        if let Err(_err) = invoice.insert(&c) {
                            slog::error!(self.logger, "Error inserting invoice.");
                            return;
                        }

                        let invoice_response = InvoiceResponse {
                            amount,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            meta: msg.meta,
                            rate: None,
                            payment_request: Some(invoice.payment_request),
                            currency: msg.currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: Some(target_account.account_id),
                            error: None,
                            fees: None,
                        };

                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api)
                    }
                }
                Api::InvoiceResponse(ref msg) => {
                    let rate = match msg.rate {
                        Some(r) => r,
                        None => {
                            let mut m = msg.clone();
                            m.error = Some(InvoiceResponseError::RateNotAvailable);
                            let msg = Message::Api(Api::InvoiceResponse(m));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    };
                    let amount_in_btc = msg.amount / rate;
                    let amount_in_sats = (amount_in_btc * Decimal::new(SATS_IN_BITCOIN as i64, 0))
                        .round_dp_with_strategy(0, RoundingStrategy::AwayFromZero)
                        .to_u64()
                        .unwrap_or_else(|| panic!());

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            slog::error!(self.logger, "No database provided.");
                            return;
                        }
                    };

                    let c = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            slog::error!(self.logger, "Couldn't get psql connection.");
                            return;
                        }
                    };

                    let user_account = self
                        .ledger
                        .user_accounts
                        .entry(msg.uid)
                        .or_insert_with(|| UserAccount::new(msg.uid));

                    let mut target_account = Account::new(Currency::BTC, AccountType::Internal);

                    if let Some(account_id) = msg.account_id {
                        if let Some(acc) = user_account.accounts.get(&account_id) {
                            target_account = acc.clone();
                        } else {
                            let invoice_response = InvoiceResponse {
                                amount: msg.amount,
                                req_id: msg.req_id,
                                uid: msg.uid,
                                rate: None,
                                meta: msg.meta.clone(),
                                payment_request: None,
                                currency: msg.currency,
                                target_account_currency: msg.target_account_currency,
                                account_id: Some(target_account.account_id),
                                error: Some(InvoiceResponseError::AccountDoesNotExist),
                                fees: None,
                            };
                            let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    } else {
                        // If user does not specify an account_id we select or create one for him.
                        let account = user_account.get_default_account(Currency::BTC);
                        target_account = account;
                    }

                    let currency = msg.currency;
                    let deposit_limit = self
                        .deposit_limits
                        .get(&currency)
                        .unwrap_or_else(|| panic!("Failed to get deposit limit for {}", currency));

                    // Check whether deposit limit is exceeded.
                    if target_account.balance + msg.amount > *deposit_limit {
                        let invoice_response = InvoiceResponse {
                            amount: amount_in_btc,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            rate: None,
                            meta: msg.meta.clone(),
                            payment_request: None,
                            currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: Some(target_account.account_id),
                            error: Some(InvoiceResponseError::DepositLimitExceeded),
                            fees: None,
                        };
                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    if let Ok(mut invoice) = self
                        .lnd_connector
                        .create_invoice(amount_in_sats, msg.meta.clone(), msg.uid, target_account.account_id)
                        .await
                    {
                        invoice.currency = Some(msg.currency.to_string());
                        if let Err(_err) = invoice.insert(&c) {
                            slog::error!(self.logger, "Error inserting invoice.");
                            return;
                        }

                        let invoice_response = InvoiceResponse {
                            amount: msg.amount,
                            req_id: msg.req_id,
                            uid: msg.uid,
                            meta: msg.meta.clone(),
                            rate: msg.rate,
                            payment_request: Some(invoice.payment_request),
                            currency: msg.currency,
                            target_account_currency: msg.target_account_currency,
                            account_id: Some(target_account.account_id),
                            error: None,
                            fees: msg.fees,
                        };

                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api)
                    }
                }
                Api::PaymentRequest(mut msg) => {
                    slog::warn!(self.logger, "Received withdrawal request: {:?}", msg);

                    let uid = msg.uid;

                    if !self.check_withdrawal_request_rate_limit(uid) {
                        let payment_response = PaymentResponse::error(
                            PaymentResponseError::RequestLimitExceeded,
                            msg.req_id,
                            uid,
                            msg.payment_request,
                            msg.currency,
                            msg.rate,
                        );
                        let msg = Message::Api(Api::PaymentResponse(payment_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let mut outbound_account = {
                        let user_account = match self.ledger.user_accounts.get_mut(&uid) {
                            Some(ua) => ua,
                            None => {
                                let payment_response = PaymentResponse::error(
                                    PaymentResponseError::UserAccountNotFound,
                                    msg.req_id,
                                    uid,
                                    msg.payment_request,
                                    msg.currency,
                                    msg.rate,
                                );
                                let msg = Message::Api(Api::PaymentResponse(payment_response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                        };
                        user_account.get_default_account(msg.currency)
                    };

                    if self.is_insurance_fund_depleted() {
                        slog::warn!(
                            self.logger,
                            "Insurance fund is depleted. Rejecting Lnurl Withdrawal Request."
                        );
                    }

                    if let Some(amount) = msg.amount {
                        if amount <= dec!(0) {
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::InvalidAmount,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    }

                    // If user specified a username then we attempt to make an internal transaction.
                    if msg.receipient.is_some() {
                        self.make_internal_tx(msg, listener);
                        return;
                    }

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            slog::error!(self.logger, "No database provided.");
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::DatabaseConnectionFailed,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    };

                    let psql_connection = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            slog::error!(self.logger, "Couldn't get psql connection.");
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::DatabaseConnectionFailed,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    };

                    let payment_request = msg
                        .clone()
                        .payment_request
                        .unwrap_or_else(|| panic!("Payment request not specified in the message: {:?}", msg));

                    let decoded = match payment_request.parse::<lightning_invoice::Invoice>() {
                        Ok(d) => d,
                        Err(_) => {
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::InvalidInvoice,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    };

                    // If the user supplied a zero-amount invoice, return an error
                    let (invoice_amount_millisats, invoice_amount_sats) =
                        if let Some(millisats) = decoded.amount_milli_satoshis() {
                            (millisats, millisats / 1000)
                        } else {
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::ZeroAmountInvoice,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        };

                    // Amount in sats that we're paying.
                    let amount_in_sats = Decimal::new(invoice_amount_sats as i64, 0);
                    // Amount in btc that we're paying.
                    let amount_in_btc = amount_in_sats / Decimal::new(SATS_IN_BITCOIN as i64, 0);

                    msg.amount = Some(amount_in_btc);

                    if msg.currency != Currency::BTC && msg.rate.is_none() {
                        let msg = Message::Api(Api::PaymentRequest(msg));
                        listener(msg, ServiceIdentity::Dealer);
                        return;
                    }

                    if msg.currency == Currency::BTC {
                        msg.rate = Some(dec!(1));
                    }

                    let rate = msg
                        .rate
                        .unwrap_or_else(|| panic!("Rate has not been specified in the message: {:?}", msg));
                    let invoice = if let Ok(invoice) =
                        models::invoices::Invoice::get_by_invoice_hash(&psql_connection, payment_request.clone())
                    {
                        Some(invoice)
                    } else {
                        let invoice = models::invoices::Invoice {
                            payment_request: payment_request.clone(),
                            rhash: decoded.payment_hash().to_string(),
                            payment_hash: decoded.payment_hash().to_string(),
                            created_at: utils::time::time_now() as i64,
                            value: invoice_amount_sats as i64,
                            value_msat: invoice_amount_millisats as i64,
                            expiry: decoded.expiry_time().as_secs() as i64,
                            settled: false,
                            add_index: -1,
                            settled_date: 0,
                            uid: uid as i32,
                            account_id: Uuid::default().to_string(),
                            owner: None,
                            fees: None,
                            incoming: false,
                            currency: Some(msg.currency.to_string()),
                            target_account_currency: None,
                        };
                        invoice
                            .insert(&psql_connection)
                            .expect("Failed to insert psql connection");
                        Some(invoice)
                    };

                    // If we couldn't create an invoice we cannot proceed with the payment.
                    let mut invoice = match invoice {
                        None => {
                            slog::error!(self.logger, "Couldn't create invoice. Can't make payment.");
                            let payment_response = PaymentResponse::error(
                                PaymentResponseError::RequestLimitExceeded,
                                msg.req_id,
                                uid,
                                msg.payment_request,
                                msg.currency,
                                msg.rate,
                            );
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                        Some(inv) => inv,
                    };

                    // Preparing a generic response.
                    let mut payment_response = PaymentResponse {
                        amount: amount_in_btc,
                        payment_hash: Uuid::new_v4().to_string(),
                        req_id: msg.req_id,
                        uid,
                        success: false,
                        payment_request: Some(payment_request.clone()),
                        currency: msg.currency,
                        fees: dec!(0),
                        rate,
                        error: None,
                    };

                    if let Some(owner) = invoice.owner {
                        if uid == owner as u64 {
                            slog::info!(self.logger, "User tried to make self payment. Not allowed.");
                            payment_response.error = Some(PaymentResponseError::SelfPayment);
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    }

                    // If invoice was already paid we reject this the payment request.
                    if invoice.settled {
                        slog::info!(self.logger, "Invoice is already settled.");
                        payment_response.error = Some(PaymentResponseError::InvoiceAlreadyPaid);
                        let msg = Message::Api(Api::PaymentResponse(payment_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    // We could be dealing with an internal transaction in which case we cannot borrow two accounts
                    // as mutable. Hence we have to work with local scoping. We first deal with the payer.

                    let outbound_balance = outbound_account.balance;

                    // Worst case amount user will have to pay for this transaction in Bitcoin.
                    let max_fee_in_btc = (amount_in_btc * self.ln_network_fee_margin)
                        .round_dp_with_strategy(SATS_DECIMALS, RoundingStrategy::AwayFromZero);

                    let settings = self.lnd_connector_settings.clone();
                    let mut lnd_connector = LndConnector::new(settings).await;

                    let estimated_fee = if let Ok(res) = lnd_connector
                        .probe(payment_request.clone(), self.ln_network_fee_margin)
                        .await
                    {
                        if !res.is_empty() {
                            let best_route = res[0].clone();
                            Decimal::new(best_route.total_fees, 0) / Decimal::new(SATS_IN_BITCOIN as i64, 0)
                        } else {
                            max_fee_in_btc
                        }
                    } else {
                        max_fee_in_btc
                    };

                    let outbound_amount_in_btc_plus_max_fees = amount_in_btc + estimated_fee;

                    // Worst case amount user will have to pay for this transaction in outbound Currency.
                    let outbound_amount_in_outbound_currency_plus_max_fee = outbound_amount_in_btc_plus_max_fees / rate;

                    // Checking whether user has enough funds on their outbound currency account.
                    if outbound_balance < outbound_amount_in_outbound_currency_plus_max_fee {
                        payment_response.error = Some(PaymentResponseError::InsufficientFundsForFees);
                        let msg = Message::Api(Api::PaymentResponse(payment_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    if invoice.owner.is_none() {
                        // We need to debit amount a user is trying to send before sending the payment so he cannot
                        // double spend.
                        let mut external_account = self.ledger.external_account.clone();

                        if self
                            .make_tx(
                                &mut outbound_account,
                                uid,
                                &mut external_account,
                                BANK_UID,
                                outbound_amount_in_outbound_currency_plus_max_fee,
                                rate,
                            )
                            .is_err()
                        {
                            payment_response.error = Some(PaymentResponseError::TransactionFailed);
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }

                        self.ledger.external_account = external_account.clone();

                        self.insert_into_ledger(&uid, outbound_account.account_id, outbound_account.clone());

                        self.update_account(&outbound_account, msg.uid);
                        self.update_account(&external_account, BANK_UID);

                        payment_response.success = false;
                        payment_response.fees = estimated_fee;

                        let payment_task_sender = self.payment_thread_sender.clone();

                        let settings = self.lnd_connector_settings.clone();
                        let req_id = msg.req_id;
                        let payment_req = payment_request;
                        let aib = amount_in_btc;
                        let currency = msg.currency;

                        let estimated_fee_in_sats = estimated_fee * Decimal::new(SATS_IN_BITCOIN as i64, 0);

                        let payment_task = tokio::task::spawn(async move {
                            let mut lnd_connector = LndConnector::new(settings).await;
                            match lnd_connector
                                .pay_invoice(payment_req.clone(), amount_in_sats, None, Some(estimated_fee_in_sats))
                                .await
                            {
                                Ok(result) => {
                                    let payment_response = PaymentResponse {
                                        uid,
                                        req_id,
                                        currency,
                                        payment_hash: result.payment_hash,
                                        success: true,
                                        payment_request: Some(payment_req.clone()),
                                        amount: aib,
                                        fees: Decimal::new(result.fee as i64, 0),
                                        rate,
                                        error: None,
                                    };
                                    let msg = Message::Bank(Bank::PaymentResult(PaymentResult {
                                        uid,
                                        currency,
                                        rate,
                                        is_success: true,
                                        amount: outbound_amount_in_btc_plus_max_fees,
                                        payment_response,
                                        error: None,
                                    }));
                                    if let Err(err) = payment_task_sender.send(msg) {
                                        panic!("Failed to send a payment task: {:?}", err);
                                    }
                                }
                                Err(e) => {
                                    let payment_response = PaymentResponse {
                                        uid,
                                        req_id,
                                        currency,
                                        payment_hash: String::from(""),
                                        success: false,
                                        payment_request: Some(payment_req.clone()),
                                        amount: aib,
                                        fees: dec!(0),
                                        rate,
                                        error: Some(PaymentResponseError::InsufficientFundsForFees),
                                    };
                                    let msg = Message::Bank(Bank::PaymentResult(PaymentResult {
                                        uid,
                                        currency,
                                        rate,
                                        is_success: false,
                                        amount: outbound_amount_in_btc_plus_max_fees,
                                        payment_response,
                                        error: Some(e.to_string()),
                                    }));
                                    if let Err(err) = payment_task_sender.send(msg) {
                                        panic!("Failed to send a payment task: {:?}", err);
                                    }
                                }
                            }
                        });
                        self.payment_threads.push(payment_task);
                        return;
                    }

                    if let Some(invoice_currency) = invoice.currency.clone() {
                        if let Ok(currency) = Currency::from_str(&invoice_currency) {
                            if currency != msg.currency {
                                slog::info!(
                                    self.logger,
                                    "Cannot send internal tx between two different currency accounts"
                                );
                                payment_response.success = false;
                                let msg = Message::Api(Api::PaymentResponse(payment_response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                        } else {
                            panic!("Failed to convert {} to a valid currency", invoice_currency);
                        }
                    } else {
                        panic!("Invoice currency not specified: {:?}", invoice);
                    }

                    if msg.currency != Currency::BTC && msg.rate.is_none() {
                        let msg = Message::Api(Api::PaymentRequest(msg));
                        listener(msg, ServiceIdentity::Dealer);
                        return;
                    }

                    let owner = invoice
                        .owner
                        .unwrap_or_else(|| panic!("Invoice owner has not been specified: {:?}", invoice))
                        as u64;

                    let mut invoice_owner_account = {
                        let user_account = self
                            .ledger
                            .user_accounts
                            .entry(owner as u64)
                            .or_insert_with(|| UserAccount::new(owner as u64));
                        user_account.get_default_account(msg.currency)
                    };

                    let mut invoice_payer_account = {
                        match self.ledger.user_accounts.get_mut(&uid) {
                            Some(user_account) => user_account.get_default_account(msg.currency),
                            None => {
                                slog::error!(
                                    self.logger,
                                    "Invoice payer's user account does not exist: uid: {}, invoice: {:?}",
                                    uid,
                                    invoice
                                );
                                return;
                            }
                        }
                    };

                    let mut rate = rate;
                    let amount = amount_in_btc / rate;

                    // we are resetting the rate because we made the conversions
                    rate = dec!(1);

                    let internal_tx_fee = amount * self.internal_tx_fee;

                    if internal_tx_fee + amount > invoice_payer_account.balance {
                        slog::info!(
                            self.logger,
                            "User: {} doesn't have enough funds to cover tx fee of {}",
                            uid,
                            internal_tx_fee
                        );
                        payment_response.success = false;
                        let msg = Message::Api(Api::PaymentResponse(payment_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    if self
                        .make_tx(
                            &mut invoice_payer_account,
                            uid,
                            &mut invoice_owner_account,
                            owner,
                            amount,
                            rate,
                        )
                        .is_err()
                    {
                        return;
                    }

                    let mut fee_account = self.ledger.fee_account.clone();

                    // Deducting internal fees
                    // TODO: This is wrong./
                    if internal_tx_fee > dec!(0)
                        && self
                            .make_tx(
                                &mut invoice_payer_account,
                                uid,
                                &mut fee_account,
                                BANK_UID,
                                amount_in_btc,
                                rate,
                            )
                            .is_err()
                    {
                        return;
                    }

                    self.ledger.fee_account = fee_account;

                    self.insert_into_ledger(&owner, invoice_owner_account.account_id, invoice_owner_account.clone());
                    self.insert_into_ledger(&uid, invoice_payer_account.account_id, invoice_payer_account.clone());

                    // Update DB.
                    self.update_account(&invoice_payer_account, uid as u64);
                    self.update_account(&invoice_owner_account, owner as u64);

                    // Update Invoice
                    invoice.settled = true;
                    if invoice.update(&psql_connection).is_err() {
                        slog::info!(self.logger, "Couldn't update invoice.");
                        return;
                    }

                    payment_response.success = true;
                    payment_response.fees = internal_tx_fee;

                    let msg = Message::Api(Api::PaymentResponse(payment_response));
                    listener(msg, ServiceIdentity::Api);
                }

                Api::SwapRequest(msg) => {
                    if self.is_insurance_fund_depleted() {
                        slog::warn!(self.logger, "Insurance is depleted Deposit request Failed!");
                        return;
                    }
                    slog::warn!(self.logger, "Received swap request: {:?}", msg);
                    let msg = Message::Api(Api::SwapRequest(msg));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Api::SwapResponse(mut msg) => {
                    slog::warn!(self.logger, "Received swap response: {:?}", msg);
                    if msg.error.is_some() || !msg.success {
                        let msg = Message::Api(Api::SwapResponse(msg));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let mut swap_response = msg.clone();

                    dbg!(&self.available_currencies);

                    // Checking whether we can convert into the target curreny.
                    if !self.available_currencies.contains(&msg.to) {
                        swap_response.success = false;
                        swap_response.error = Some(SwapResponseError::CurrencyNotAvailable);
                        let msg = Message::Api(Api::SwapResponse(swap_response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            slog::error!(self.logger, "No database provided.");
                            return;
                        }
                    };

                    let _psql_connection = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            slog::error!(self.logger, "Couldn't get psql connection.");
                            return;
                        }
                    };

                    let uid = msg.uid;
                    let swap_amount = msg.amount;

                    let rate = match msg.rate {
                        Some(rate) => rate,
                        None => {
                            swap_response.success = false;
                            swap_response.error = Some(SwapResponseError::CurrencyNotAvailable);
                            let msg = Message::Api(Api::SwapResponse(swap_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }
                    };

                    let (mut outbound_account, mut inbound_account) = {
                        let user_account = match self.ledger.user_accounts.get_mut(&msg.uid) {
                            Some(ua) => ua,
                            None => return,
                        };

                        let outbound_account = user_account.get_default_account(msg.from);
                        let inbound_account = user_account.get_default_account(msg.to);

                        (outbound_account, inbound_account)
                    };

                    if outbound_account.balance < swap_amount {
                        slog::info!(
                            self.logger,
                            "User: {} has not enough available balance. Available: {}",
                            uid,
                            outbound_account.balance
                        );
                        msg.success = false;
                        msg.error = Some(SwapResponseError::NotEnoughAvailableBalance);
                        let msg = Message::Api(Api::SwapResponse(msg));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    if self
                        .make_tx(&mut outbound_account, uid, &mut inbound_account, uid, swap_amount, rate)
                        .is_err()
                    {
                        slog::info!(self.logger, "Tx failed to go through.");
                        return;
                    }

                    self.insert_into_ledger(&uid, outbound_account.account_id, outbound_account.clone());
                    self.insert_into_ledger(&uid, inbound_account.account_id, inbound_account.clone());

                    self.update_account(&outbound_account, uid);
                    self.update_account(&inbound_account, uid);

                    let msg = Message::Api(Api::SwapResponse(swap_response));
                    listener(msg, ServiceIdentity::Api);

                    // Updating the dealer of the new state of the bank.
                    let bank_state = self.get_bank_state();
                    let msg = Message::Dealer(Dealer::BankState(bank_state));
                    listener(msg, ServiceIdentity::Dealer);
                }

                Api::GetBalances(msg) => {
                    let user_account = self
                        .ledger
                        .user_accounts
                        .entry(msg.uid)
                        .or_insert_with(|| UserAccount::new(msg.uid));
                    let balances = Balances {
                        req_id: msg.req_id,
                        uid: msg.uid,
                        accounts: user_account.accounts.clone(),
                    };
                    let msg = Message::Api(Api::Balances(balances));
                    listener(msg.clone(), ServiceIdentity::Api);
                    listener(msg, ServiceIdentity::Dealer);
                }
                Api::QuoteRequest(msg) => {
                    let msg = Message::Api(Api::QuoteRequest(msg));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Api::QuoteResponse(msg) => {
                    let msg = Message::Api(Api::QuoteResponse(msg));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::AvailableCurrenciesRequest(msg) => {
                    let msg = Message::Api(Api::AvailableCurrenciesRequest(msg));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Api::AvailableCurrenciesResponse(msg) => {
                    let msg = Message::Api(Api::AvailableCurrenciesResponse(msg));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::GetNodeInfoRequest(msg) => {
                    let lnd_node_info = match self.lnd_connector.get_node_info().await {
                        Ok(ni) => ni,
                        Err(_) => LndNodeInfo::default(),
                    };
                    let response = GetNodeInfoResponse {
                        req_id: msg.req_id,
                        lnd_node_info,
                        ln_network_fee_margin: self.ln_network_fee_margin,
                        ln_network_max_fee: self.ln_network_max_fee,
                        internal_tx_fee: self.internal_tx_fee,
                        external_tx_fee: self.external_tx_fee,
                        reserve_ratio: self.reserve_ratio,
                    };
                    let msg = Message::Api(Api::GetNodeInfoResponse(response));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::CreateLnurlWithdrawalRequest(msg) => {
                    if self.is_insurance_fund_depleted() {
                        slog::warn!(
                            self.logger,
                            "Insurance fund is depleted. Rejecting Lnurl Withdrawal Request."
                        );
                    }

                    slog::warn!(self.logger, "Received LNURL withdrawal request: {:?}", msg);
                    let uid = msg.uid;

                    let mut response = CreateLnurlWithdrawalResponse {
                        req_id: msg.req_id,
                        lnurl: None,
                        error: None,
                    };

                    if msg.amount <= dec!(0) {
                        response.error = Some(CreateLnurlWithdrawalError::InvalidAmount);
                        let msg = Message::Api(Api::CreateLnurlWithdrawalResponse(response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let outbound_account = {
                        let user_account = match self.ledger.user_accounts.get_mut(&uid) {
                            Some(ua) => ua,
                            None => {
                                response.error = Some(CreateLnurlWithdrawalError::UserAccountNotFound);
                                let msg = Message::Api(Api::CreateLnurlWithdrawalResponse(response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                        };

                        user_account.get_default_account(msg.currency)
                    };

                    if msg.currency != Currency::BTC && msg.rate.is_none() {
                        let msg = Message::Api(Api::CreateLnurlWithdrawalRequest(msg));
                        listener(msg, ServiceIdentity::Dealer);
                        return;
                    }

                    if outbound_account.balance < msg.amount {
                        response.error = Some(CreateLnurlWithdrawalError::InsufficientFunds);
                        let msg = Message::Api(Api::CreateLnurlWithdrawalResponse(response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }
                    let now = utils::time::time_now();

                    let amount = msg.amount;

                    let payment_request = PaymentRequest {
                        uid: msg.uid,
                        req_id: msg.req_id,
                        amount: Some(amount),
                        currency: msg.currency,
                        rate: msg.rate,
                        payment_request: Some(String::from("")),
                        receipient: None,
                        fees: msg.fees,
                    };

                    let lnurl_path = String::from("https://lndhubx.kollider.xyz/api/lnurl_withdrawal/request");
                    let q = msg.req_id;
                    let lnurl = if let Ok(encoded) = utils::lnurl::encode(lnurl_path, Some(q.to_string())) {
                        encoded
                    } else {
                        response.error = Some(CreateLnurlWithdrawalError::FailedToCreateLnUrl);
                        let msg = Message::Api(Api::CreateLnurlWithdrawalResponse(response));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    };
                    response.lnurl = Some(lnurl);
                    self.lnurl_withdrawal_requests
                        .insert(payment_request.req_id, (now, payment_request));

                    let msg = Message::Api(Api::CreateLnurlWithdrawalResponse(response));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::GetLnurlWithdrawalRequest(msg) => {
                    let callback = String::from("https://lndhubx.kollider.xyz/api/lnurl_withdrawal/pay");
                    let mut response = GetLnurlWithdrawalResponse {
                        callback,
                        req_id: msg.req_id,
                        max_withdrawable: 0,
                        default_description: String::from("Lndhubx Withdrawal"),
                        min_withdrawable: 1,
                        tag: String::from("withdrawalRequest"),
                        error: None,
                    };
                    if let Some((_, payment_request)) = self.lnurl_withdrawal_requests.get(&msg.req_id) {
                        if let Some(a) = payment_request.amount {
                            let a = match payment_request.rate {
                                Some(r) => a * r,
                                None => a,
                            };
                            let a = a * dec!(100000000);
                            if let Some(ma) = a.to_u64() {
                                response.max_withdrawable = ma;
                                let msg = Message::Api(Api::GetLnurlWithdrawalResponse(response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                        }
                    }
                    response.error = Some(GetLnurlWithdrawalError::RequestNotFound);
                    let msg = Message::Api(Api::GetLnurlWithdrawalResponse(response));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::PayLnurlWithdrawalRequest(msg) => {
                    if let Some((_, payment_request)) = self.lnurl_withdrawal_requests.get_mut(&msg.req_id) {
                        payment_request.payment_request = Some(msg.payment_request);
                        let msg = Message::Api(Api::PaymentRequest(payment_request.clone()));
                        listener(msg, ServiceIdentity::Loopback);
                        return;
                    }
                    let response = PayLnurlWithdrawalResponse {
                        req_id: msg.req_id,
                        error: Some(PayLnurlWithdrawalError::RequestNotFound),
                    };
                    let msg = Message::Api(Api::PayLnurlWithdrawalResponse(response));
                    listener(msg, ServiceIdentity::Api);
                }
                Api::QueryRouteRequest(msg) => {
                    let settings = self.lnd_connector_settings.clone();
                    let mut lnd_connector = LndConnector::new(settings).await;

                    if let Ok(res) = lnd_connector.probe(msg.payment_request, dec!(0.0005)).await {
                        if !res.is_empty() {
                            let best_route = res[0].clone();
                            let msg = Message::Api(Api::QueryRouteResponse(QueryRouteResponse {
                                req_id: msg.req_id,
                                total_fee: Decimal::new(best_route.total_fees, 0),
                                error: None,
                            }));
                            listener(msg, ServiceIdentity::Api);
                        } else {
                            let msg = Message::Api(Api::QueryRouteResponse(QueryRouteResponse {
                                req_id: msg.req_id,
                                total_fee: dec!(0),
                                error: Some(QueryRouteError::NoRouteFound),
                            }));
                            listener(msg, ServiceIdentity::Api);
                        }
                    }
                }

                _ => {}
            },
            Message::Bank(msg) => match msg {
                Bank::PaymentResult(res) => {
                    slog::warn!(self.logger, "Received payment result: {:?}", res);

                    if res.amount <= dec!(0) {
                        panic!("Amount is smaller than zero.");
                        return;
                    }

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            slog::error!(self.logger, "No database provided.");
                            return;
                        }
                    };

                    let psql_connection = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            slog::error!(self.logger, "Couldn't get psql connection.");
                            return;
                        }
                    };

                    let uid = res.uid;

                    let mut external_account = self.ledger.external_account.clone();

                    let mut inbound_account = {
                        let user_account = match self.ledger.user_accounts.get_mut(&uid) {
                            Some(ua) => ua,
                            None => return,
                        };

                        user_account.get_default_account(res.currency)
                    };

                    let inv_rate = dec!(1) / res.rate;

                    let mut payment_response = res.payment_response;

                    let mut bank_fee_account = self.ledger.fee_account.clone();

                    if res.is_success {
                        // If successful and there are excess fees we send it to the bank fee account.
                        let fees_payed_in_btc = payment_response.fees / Decimal::new(SATS_IN_BITCOIN as i64, 0);

                        let excess_fees_in_btc = res.amount - (payment_response.amount + fees_payed_in_btc);

                        assert!(excess_fees_in_btc >= dec!(0));

                        if excess_fees_in_btc > dec!(0) {
                            if self
                                .make_tx(
                                    &mut external_account,
                                    BANK_UID,
                                    &mut bank_fee_account,
                                    BANK_UID,
                                    excess_fees_in_btc,
                                    inv_rate,
                                )
                                .is_err()
                            {
                                return;
                            }

                            self.ledger.external_account = external_account.clone();
                            self.ledger.fee_account = bank_fee_account.clone();

                            self.update_account(&bank_fee_account, BANK_UID);
                            self.update_account(&external_account, BANK_UID);
                        }

                        payment_response.success = true;

                        let pr = payment_response.clone().payment_request.unwrap_or_else(|| {
                            panic!(
                                "Payment request has not been specified in the payment response: {:?}",
                                payment_response
                            )
                        });

                        let mut invoice =
                            if let Ok(invoice) = models::invoices::Invoice::get_by_invoice_hash(&psql_connection, pr) {
                                invoice
                            } else {
                                return;
                            };

                        invoice.settled = true;

                        if invoice.update(&psql_connection).is_err() {
                            slog::error!(self.logger, "Error updating updating invoices!");
                        }
                    } else {
                        let refund = res.amount;

                        if self
                            .make_tx(
                                &mut external_account,
                                BANK_UID,
                                &mut inbound_account,
                                uid,
                                refund,
                                inv_rate,
                            )
                            .is_err()
                        {
                            return;
                        }

                        self.ledger.external_account = external_account.clone();

                        self.insert_into_ledger(&uid, inbound_account.account_id, inbound_account.clone());

                        self.update_account(&inbound_account, res.uid);
                        self.update_account(&external_account, BANK_UID);
                    }

                    let msg = Message::Api(Api::PaymentResponse(payment_response));
                    listener(msg, ServiceIdentity::Api);
                }
            },
            Message::Cli(Cli::MakeTx(make_tx)) => {
                let tx = make_tx.clone();
                let result = match self.process_make_tx(make_tx).await {
                    Ok(_) => "Successful".to_string(),
                    Err(err) => err.to_string(),
                };
                let msg = Message::Cli(Cli::MakeTxResult(MakeTxResult { tx, result }));
                // the identity is ignored by cli listener, so we are using ServiceIdentity::Api here
                // just to pass some argument
                listener(msg, ServiceIdentity::Api);
            }
            _ => {}
        }
    }

    async fn process_pay_invoice(&mut self, pay_invoice: PayInvoice, is_insurance_invoice: bool) {
        let decoded = match pay_invoice
            .payment_request
            .clone()
            .parse::<lightning_invoice::Invoice>()
        {
            Ok(d) => d,
            Err(_) => return,
        };

        let amount_in_milli_satoshi = decoded
            .amount_milli_satoshis()
            .unwrap_or_else(|| panic!("Amount in millisatoshi is not specified: {:?}", decoded));
        // scale 3, which corresponds to dividing by 10^3 = 1000
        let amount_in_sats = Decimal::new(amount_in_milli_satoshi as i64, 3);

        let invoice_type_text = if is_insurance_invoice { "insurance " } else { "" };

        slog::debug!(
            self.logger,
            "Dealer requests to pay {}invoice: {} of amount: {}",
            invoice_type_text,
            pay_invoice.payment_request,
            amount_in_sats
        );

        match self
            .lnd_connector
            .pay_invoice(
                pay_invoice.payment_request.clone(),
                amount_in_sats,
                Some(self.ln_network_max_fee),
                None,
            )
            .await
        {
            Ok(result) => {
                slog::debug!(self.logger, "{:?}", result);
                if is_insurance_invoice {
                    let mut external_account = self.ledger.external_account.clone();
                    let mut insurance_fund_account = self.ledger.insurance_fund_account.clone();
                    if self
                        .make_tx(
                            &mut insurance_fund_account,
                            DEALER_UID,
                            &mut external_account,
                            BANK_UID,
                            amount_in_sats * Decimal::new(1, SATS_DECIMALS),
                            Decimal::ONE,
                        )
                        .is_err()
                    {
                        return;
                    }

                    self.update_account(&external_account, BANK_UID);
                    self.update_account(&insurance_fund_account, DEALER_UID);
                    self.ledger.external_account = external_account;
                    self.ledger.insurance_fund_account = insurance_fund_account;
                }
            }
            Err(err) => {
                slog::error!(
                    self.logger,
                    "Failed to pay {}invoice {:?}, reason: {:?}",
                    invoice_type_text,
                    pay_invoice,
                    err
                );
            }
        }
    }

    async fn process_create_invoice_request<F: FnMut(Message, ServiceIdentity)>(
        &mut self,
        req: CreateInvoiceRequest,
        invoice_owner: UserId,
        listener: &mut F,
    ) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                slog::error!(self.logger, "No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                slog::error!(self.logger, "Couldn't get psql connection.");
                return;
            }
        };

        let account_id = match invoice_owner {
            BANK_UID => self.ledger.external_account.account_id,
            DEALER_UID => self.ledger.insurance_fund_account.account_id,
            _ => panic!("Unexpected invoice owner. It can be Bank or Dealer only"),
        };

        if let Ok(invoice) = self
            .lnd_connector
            .create_invoice(req.amount, req.memo, invoice_owner, account_id)
            .await
        {
            slog::info!(self.logger, "Inserting invoice into db: {:?}", invoice);
            if let Err(_err) = invoice.insert(&c) {
                slog::error!(self.logger, "Couldn't insert invoice: {:?}", invoice);
                return;
            }

            let create_invoice_response = CreateInvoiceResponse {
                req_id: req.req_id,
                payment_request: invoice.payment_request,
                amount: req.amount,
            };

            let msg = Message::Dealer(Dealer::CreateInvoiceResponse(create_invoice_response));
            listener(msg, ServiceIdentity::Dealer)
        }
    }

    async fn process_make_tx(&mut self, make_tx: MakeTx) -> Result<(), BankError> {
        let MakeTx {
            outbound_uid,
            outbound_account_id,
            inbound_uid,
            inbound_account_id,
            amount,
            currency,
        } = make_tx;

        if amount.is_sign_negative() {
            return Err(BankError::FailedTransaction);
        }

        if outbound_uid == inbound_uid && outbound_account_id == inbound_account_id {
            return Err(BankError::FailedTransaction);
        }

        let is_inbound_insurance_account =
            inbound_uid == DEALER_UID && inbound_account_id == self.ledger.insurance_fund_account.account_id;
        let is_outbound_insurance_account =
            outbound_uid == DEALER_UID && make_tx.outbound_account_id == self.ledger.insurance_fund_account.account_id;
        let is_inbound_external_account =
            inbound_uid == BANK_UID && inbound_account_id == self.ledger.external_account.account_id;
        let is_outbound_external_account =
            outbound_uid == BANK_UID && outbound_account_id == self.ledger.external_account.account_id;

        if (is_inbound_insurance_account && !is_outbound_external_account)
            || (is_outbound_insurance_account && !is_inbound_external_account)
        {
            return Err(BankError::FailedTransaction);
        }

        let mut outbound_account = if is_outbound_external_account {
            self.ledger.external_account.clone()
        } else if is_outbound_insurance_account {
            self.ledger.insurance_fund_account.clone()
        } else {
            self.ledger
                .user_accounts
                .get(&outbound_uid)
                .ok_or(BankError::UserAccountNotFound)?
                .accounts
                .get(&outbound_account_id)
                .cloned()
                .ok_or(BankError::AccountNotFound)?
        };

        let mut inbound_account = if is_inbound_external_account {
            self.ledger.external_account.clone()
        } else if is_inbound_insurance_account {
            self.ledger.insurance_fund_account.clone()
        } else {
            self.ledger
                .user_accounts
                .get(&inbound_uid)
                .ok_or(BankError::UserAccountNotFound)?
                .accounts
                .get(&inbound_account_id)
                .cloned()
                .ok_or(BankError::AccountNotFound)?
        };

        if outbound_account.currency != currency || outbound_account.currency != inbound_account.currency {
            return Err(BankError::FailedTransaction);
        }

        self.make_tx(
            &mut outbound_account,
            outbound_uid,
            &mut inbound_account,
            inbound_uid,
            amount,
            Decimal::ONE,
        )?;

        self.update_account(&outbound_account, outbound_uid);
        self.update_account(&inbound_account, inbound_uid);

        if is_outbound_external_account {
            self.ledger.external_account = outbound_account;
        } else if is_outbound_insurance_account {
            self.ledger.insurance_fund_account = outbound_account;
        } else {
            self.insert_into_ledger(&outbound_uid, outbound_account_id, outbound_account);
        };

        if is_inbound_external_account {
            self.ledger.external_account = inbound_account
        } else if is_inbound_insurance_account {
            self.ledger.insurance_fund_account = inbound_account
        } else {
            self.insert_into_ledger(&inbound_uid, inbound_account_id, inbound_account);
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_create_bank_manager() {}
}
