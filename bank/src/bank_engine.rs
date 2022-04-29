use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use bigdecimal::BigDecimal;
use std::collections::HashMap;
use uuid::Uuid;

use core_types::*;
use models::{accounts, internal_user_mappings, invoices::Invoice};

use msgs::api::*;
use msgs::dealer::*;
use msgs::*;
use xerror::bank_engine::*;

use lnd_connector::connector::LndConnector;

use serde::{Deserialize, Serialize};

use log::{error, info};

const BANK_UID: u64 = 23193913;

#[derive(Serialize, Deserialize)]
pub struct BankEngineSettings {
    pub psql_url: String,
    pub bank_zmq_pull_address: String,
    pub bank_zmq_publish_address: String,
    pub bank_dealer_pull_address: String,
    pub bank_dealer_push_address: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserAccount {
    owner: UserId,
    accounts: HashMap<AccountId, Account>,
    invoices: Vec<String>,
}

impl UserAccount {
    pub fn new(owner: UserId) -> Self {
        Self {
            owner,
            accounts: HashMap::new(),
            invoices: Vec::new(),
        }
    }

    /// Since users can have multiple accounts of the same currency we need
    /// a getter that returns the first best account if the user does not specify one.
    pub fn get_default_account(&mut self, currency: Currency) -> Account {
        let accounts = self
            .accounts
            .clone()
            .into_iter()
            .filter(|(_key, value)| value.currency == currency)
            .collect::<Vec<(Uuid, Account)>>();

        if !accounts.is_empty() {
            return accounts[0].1.clone();
        }
        let new_account = Account::new(currency, AccountType::Internal);
        self.accounts.insert(new_account.account_id, new_account.clone());
        new_account
    }
}

#[derive(Debug)]
pub struct Ledger {
    /// All user accounts holding all value assigned to user.
    pub user_accounts: HashMap<UserId, UserAccount>,
    /// The insurance fund is own by the bank and makes sure that liabilities can be met at any time.
    /// An insurance fund exists for each currency.
    pub insurance_fund_account: Account,
    /// Holds all fees collected by the Bank.
    pub fee_account: Account,
    /// The external account is the counterparty for every deposit from an unknown external user.
    pub external_account: Account,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            user_accounts: HashMap::new(),
            insurance_fund_account: Account::new(Currency::BTC, AccountType::Internal),
            fee_account: Account::new(Currency::BTC, AccountType::Internal),
            external_account: Account::new(Currency::BTC, AccountType::External),
        }
    }
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
    pub tx_seq: u64,
}

impl BankEngine {
    pub async fn new(conn_pool: Option<DbPool>, lnd_connector: LndConnector) -> Self {
        Self {
            lnd_node_info: LndNodeInfo::default(),
            bank_uid: BANK_UID,
            ledger: Ledger::new(),
            fee_structure: FeeStructure::new(),
            conn_pool,
            lnd_connector,
            available_currencies: Vec::new(),
            tx_seq: 0,
        }
    }

    pub fn init_accounts(&mut self) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                error!("No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                error!("Couldn't get psql connection.");
                return;
            }
        };

        let accounts = match accounts::Account::get_all_not_in(&c, &[]) {
            Ok(accs) => accs,
            Err(_) => return,
        };

        let _internal_users = match internal_user_mappings::InternalUserMapping::get_all_not_in(&c, &[]) {
            Ok(iu) => iu,
            Err(_) => return,
        };

        for account in accounts {
            let user_account = self
                .ledger
                .user_accounts
                .entry(account.uid as u64)
                .or_insert_with(|| UserAccount::new(account.uid as u64));
            let balance_str = account.balance.to_string();
            let balance_dec = Decimal::from_str(&balance_str).unwrap();
            let currency = Currency::from_str(&account.currency).unwrap();
            let acc = Account {
                currency,
                balance: balance_dec,
                account_id: account.account_id,
                account_type: AccountType::from_str(&account.account_type).unwrap(),
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

    pub fn update_account(&mut self, account: &Account, uid: UserId) {
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                error!("No database provided.");
                return;
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                error!("Couldn't get psql connection.");
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
        let conn = match &self.conn_pool {
            Some(conn) => conn,
            None => {
                error!("No database provided.");
                return Err(BankError::FailedTransaction);
            }
        };

        let c = match conn.get() {
            Ok(psql_connection) => psql_connection,
            Err(_) => {
                error!("Couldn't get psql connection.");
                return Err(BankError::FailedTransaction);
            }
        };

        let outbound_amount = amount;
        let inbound_amount = amount * rate;

        outbound_account.balance -= outbound_amount;
        inbound_account.balance += inbound_amount;

        let outbound_amount_str = amount.to_string();
        let inbound_amount_str = amount.to_string();
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

    pub async fn process_msg<F: FnMut(Message, ServiceIdentity)>(&mut self, msg: Message, listener: &mut F) {
        match msg {
            Message::Dealer(msg) => match msg {
                Dealer::Health(dealer_health) => {
                    self.available_currencies = dealer_health.available_currencies;
                    if dealer_health.status == HealthStatus::Down {
                        self.available_currencies = Vec::new();
                    }
                }
                Dealer::BankStateRequest(_) => {
                    let bank_state = self.get_bank_state();
                    let msg = Message::Dealer(Dealer::BankState(bank_state));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Dealer::PayInvoice(pay_invoice) => {
                    if let Err(err) = self
                        .lnd_connector
                        .pay_invoice(pay_invoice.payment_request.clone())
                        .await
                    {
                        dbg!(err);
                    }
                }
                Dealer::CreateInvoiceRequest(req) => {
                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            error!("No database provided.");
                            return;
                        }
                    };

                    let c = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            error!("Couldn't get psql connection.");
                            return;
                        }
                    };

                    let meta = "dealer withdrawal".to_string();
                    if let Ok(invoice) = self
                        .lnd_connector
                        .create_invoice(req.amount, meta, BANK_UID, self.ledger.external_account.account_id)
                        .await
                    {
                        if let Err(_err) = invoice.insert(&c) {
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
                _ => {}
            },
            Message::Deposit(msg) => {
                info!("Received deposit: {:?}", msg);
                // Deposit can only be triggered if someone external has payed an invoice generated by someone internal.
                let conn = match &self.conn_pool {
                    Some(conn) => conn,
                    None => {
                        error!("No database provided.");
                        return;
                    }
                };

                let c = match conn.get() {
                    Ok(psql_connection) => psql_connection,
                    Err(_) => {
                        error!("Couldn't get psql connection.");
                        return;
                    }
                };

                // Check whether we know about this invoice.
                if let Ok(invoice) = Invoice::get_by_invoice_hash(&c, msg.payment_request) {
                    // Value of the depoist.
                    let value = Decimal::new(invoice.value as i64, 0);
                    // Exchange rate of the deposit.
                    let rate = dec!(1);

                    let (mut inbound_account, inbound_uid) = {
                        let user_account = self
                            .ledger
                            .user_accounts
                            .entry(invoice.uid as u64)
                            .or_insert_with(|| UserAccount::new(invoice.uid as u64));

                        let account = user_account.get_default_account(Currency::BTC);

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

                    // Safe to unwrap as we created this account above.
                    let user_account = self.ledger.user_accounts.get_mut(&inbound_uid).unwrap();

                    // Updating cache.
                    user_account
                        .accounts
                        .insert(inbound_account.account_id, inbound_account.clone());

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
                    info!("Received invoice request: {:?}", msg);
                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            error!("No database provided.");
                            return;
                        }
                    };

                    let c = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            error!("Couldn't get psql connection.");
                            return;
                        }
                    };

                    let amount = msg.amount.unwrap_or(0);

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
                                req_id: msg.req_id,
                                uid: msg.uid,
                                payment_request: None,
                                currency: msg.currency,
                                account_id: target_account.account_id,
                                error: Some(InvoiceResponseError::AccountDoesNotExist),
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

                    if let Ok(invoice) = self
                        .lnd_connector
                        .create_invoice(amount, msg.meta, msg.uid, target_account.account_id)
                        .await
                    {
                        if let Err(_err) = invoice.insert(&c) {
                            return;
                        }

                        let invoice_response = InvoiceResponse {
                            req_id: msg.req_id,
                            uid: msg.uid,
                            payment_request: Some(invoice.payment_request),
                            currency: msg.currency,
                            account_id: target_account.account_id,
                            error: None,
                        };

                        let msg = Message::Api(Api::InvoiceResponse(invoice_response));
                        listener(msg, ServiceIdentity::Api)
                    }
                }
                Api::PaymentRequest(msg) => {
                    info!("Received payment request: {:?}", msg);

                    let conn = match &self.conn_pool {
                        Some(conn) => conn,
                        None => {
                            error!("No database provided.");
                            return;
                        }
                    };

                    let psql_connection = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            error!("Couldn't get psql connection.");
                            return;
                        }
                    };

                    let uid = msg.uid;

                    // First we decode the payment request to create a new invoice in our database.
                    if let Ok(decoded) = msg.payment_request.clone().parse::<lightning_invoice::Invoice>() {
                        let invoice = if let Ok(invoice) = models::invoices::Invoice::get_by_invoice_hash(
                            &psql_connection,
                            msg.payment_request.clone(),
                        ) {
                            Some(invoice)
                        } else {
                            let invoice = models::invoices::Invoice {
                                payment_request: msg.payment_request.clone(),
                                rhash: decoded.payment_hash().to_string(),
                                payment_hash: decoded.payment_hash().to_string(),
                                created_at: utils::time::time_now() as i64,
                                value: Decimal::new((decoded.amount_milli_satoshis().unwrap() / 1000) as i64, 0)
                                    .to_i64()
                                    .unwrap(),
                                value_msat: decoded.amount_milli_satoshis().unwrap() as i64,
                                expiry: decoded.expiry_time().as_secs() as i64,
                                settled: false,
                                add_index: -1,
                                settled_date: 0,
                                uid: uid as i32,
                                account_id: Uuid::default().to_string(),
                                owner: None,
                                fees: None,
                                incoming: false,
                            };
                            invoice
                                .insert(&psql_connection)
                                .expect("Failed to insert psql connection");
                            Some(invoice)
                        };

                        // If we couldn't create an invoice we cannot proceed with the payment.
                        let mut invoice = match invoice {
                            None => return,
                            Some(inv) => inv,
                        };

                        // Preparing a generic response.
                        let mut payment_response = PaymentResponse {
                            req_id: msg.req_id,
                            uid,
                            success: false,
                            payment_request: msg.payment_request.clone(),
                            currency: Currency::BTC,
                            error: None,
                        };

                        if let Some(owner) = invoice.owner {
                            if uid == owner as u64 {
                                payment_response.error = Some(PaymentResponseError::SelfPayment);
                                let msg = Message::Api(Api::PaymentResponse(payment_response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                        }

                        // If invoice was already paid we reject this the payment request.
                        if invoice.settled {
                            payment_response.error = Some(PaymentResponseError::InvoiceAlreadyPaid);
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }

                        let amount = Decimal::new((decoded.amount_milli_satoshis().unwrap() / 1000) as i64, 0);
                        let rate = dec!(1);

                        // We could be dealing with an internal transaction in which case we cannot borrow two accounts
                        // as mutable. Hence we have to work with local scoping. We first deal with the payer.

                        let mut outbound_account = {
                            let user_account = match self.ledger.user_accounts.get_mut(&uid) {
                                Some(ua) => ua,
                                None => return,
                            };

                            user_account.get_default_account(Currency::BTC).clone()
                        };

                        if outbound_account.balance < amount {
                            payment_response.error = Some(PaymentResponseError::InsufficientFunds);
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }

                        // If the invoice has a known owner that means that the invoice was generated by us and
                        // we are dealing with an internal transaction. If not we are doing an external transaction.
                        if invoice.owner.is_none() {
                            if self
                                .lnd_connector
                                .pay_invoice(msg.payment_request.clone())
                                .await
                                .is_ok()
                            {
                                let mut external_account = self.ledger.external_account.clone();

                                if self
                                    .make_tx(
                                        &mut outbound_account,
                                        uid,
                                        &mut external_account,
                                        BANK_UID,
                                        amount,
                                        rate,
                                    )
                                    .is_err()
                                {
                                    return;
                                }

                                self.ledger.external_account = external_account.clone();

                                let user_account = self.ledger.user_accounts.get_mut(&uid).unwrap();

                                user_account
                                    .accounts
                                    .insert(outbound_account.account_id, outbound_account.clone());

                                self.update_account(&outbound_account, msg.uid);
                                self.update_account(&external_account, BANK_UID);

                                payment_response.success = true;
                                invoice.settled = true;

                                if invoice.update(&psql_connection).is_err() {
                                    dbg!("Error updating updating invoices!");
                                }

                                let msg = Message::Api(Api::PaymentResponse(payment_response));
                                listener(msg, ServiceIdentity::Api);
                                return;
                            }
                            payment_response.success = false;
                            let msg = Message::Api(Api::PaymentResponse(payment_response));
                            listener(msg, ServiceIdentity::Api);
                            return;
                        }

                        let owner = invoice.owner.unwrap() as u64;

                        let mut invoice_owner_account = {
                            let user_account = self
                                .ledger
                                .user_accounts
                                .entry(owner as u64)
                                .or_insert_with(|| UserAccount::new(owner as u64));
                            user_account.get_default_account(Currency::BTC)
                        };

                        let mut invoice_payer_account = {
                            let user_account = self.ledger.user_accounts.get_mut(&uid).unwrap();
                            user_account.get_default_account(Currency::BTC)
                        };

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

                        {
                            let owner_user_account = self.ledger.user_accounts.get_mut(&owner).unwrap();
                            owner_user_account
                                .accounts
                                .insert(invoice_owner_account.account_id, invoice_owner_account.clone());
                        }

                        {
                            let payer_user_account = self.ledger.user_accounts.get_mut(&uid).unwrap();
                            payer_user_account
                                .accounts
                                .insert(invoice_payer_account.account_id, invoice_payer_account.clone());
                        }

                        // Update DB.
                        self.update_account(&invoice_payer_account, uid as u64);
                        self.update_account(&invoice_owner_account, owner as u64);

                        // Update Invoice
                        invoice.settled = true;
                        if invoice.update(&psql_connection).is_err() {
                            dbg!("Error updating invoices!");
                            return;
                        }

                        payment_response.success = true;

                        let msg = Message::Api(Api::PaymentResponse(payment_response));
                        listener(msg, ServiceIdentity::Api);
                    }
                }

                Api::SwapRequest(msg) => {
                    let msg = Message::Api(Api::SwapRequest(msg));
                    listener(msg, ServiceIdentity::Dealer);
                }
                Api::SwapResponse(msg) => {
                    if msg.error.is_some() || !msg.success {
                        let msg = Message::Api(Api::SwapResponse(msg));
                        listener(msg, ServiceIdentity::Api);
                        return;
                    }

                    let mut swap_response = msg.clone();

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
                            error!("No database provided.");
                            return;
                        }
                    };

                    let _psql_connection = match conn.get() {
                        Ok(psql_connection) => psql_connection,
                        Err(_) => {
                            error!("Couldn't get psql connection.");
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
                        dbg!("User has not enough funds");
                        return;
                    }

                    if self
                        .make_tx(&mut outbound_account, uid, &mut inbound_account, uid, swap_amount, rate)
                        .is_err()
                    {
                        dbg!("Transaction failed");
                        return;
                    }

                    let user_account = self.ledger.user_accounts.get_mut(&msg.uid).unwrap();

                    user_account
                        .accounts
                        .insert(outbound_account.account_id, outbound_account.clone());
                    user_account
                        .accounts
                        .insert(inbound_account.account_id, inbound_account.clone());

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
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lnd_connector::connector::{LndConnector, LndConnectorSettings};

    #[tokio::test]
    async fn test_create_bank_manager() {
        let lnd_connector_settings = LndConnectorSettings {
            node_url: "".to_string(),
            macaroon_path: "".to_string(),
            tls_path: "".to_string(),
        };
        let lnd_connector = LndConnector::new(lnd_connector_settings).await;
        let _bank_manager = BankEngine::new(None, lnd_connector).await;
        panic!()
    }
}
