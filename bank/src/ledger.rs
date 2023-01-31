use core_types::{Account, AccountClass, AccountId, AccountType, Currency, UserId};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserAccount {
    pub owner: UserId,
    pub accounts: HashMap<AccountId, Account>,
    pub invoices: Vec<String>,
    pub last_withdrawal_request: std::time::SystemTime,
    pub last_deposit_request: std::time::SystemTime,
}

impl UserAccount {
    pub fn new(owner: UserId) -> Self {
        Self {
            owner,
            accounts: HashMap::new(),
            invoices: Vec::new(),
            last_withdrawal_request: std::time::SystemTime::UNIX_EPOCH,
            last_deposit_request: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    /// Since users can have multiple accounts of the same currency we need
    /// a getter that returns the first best account if the user does not specify one.
    pub fn get_default_account(&mut self, currency: Currency, account_type: Option<AccountType>) -> Account {
        let accounts = self
            .accounts
            .clone()
            .into_iter()
            .filter(|(_key, value)| {
                if let Some(at) = account_type {
                    value.currency == currency && value.account_type == at
                } else {
                    value.currency == currency
                }
            })
            .collect::<Vec<(Uuid, Account)>>();

        if !accounts.is_empty() {
            return accounts[0].1.clone();
        }

        let account_type = if let Some(at) = account_type {
            at
        } else {
            AccountType::Internal
        };

        let new_account = Account::new(currency, account_type, AccountClass::Cash);
        self.accounts.insert(new_account.account_id, new_account.clone());
        new_account
    }
}

#[derive(Debug)]
pub struct Ledger {
    /// These are the assets.
    pub user_accounts: HashMap<UserId, UserAccount>,
    /// The insurance fund is own by the bank and makes sure that liabilities can be met at any time.
    /// An insurance fund exists for each currency.
    pub insurance_fund_account: Account,
    /// Holds all fees collected by the Bank.
    pub fee_account: UserAccount,
    // These are the liabilities.
    pub bank_liabilities: UserAccount,
    // The account of the dealer.
    pub dealer_accounts: UserAccount,
    /// The external account is the counterparty for every deposit from an unknown external user.
    pub external_fee_account: Account,
}

impl Ledger {
    pub fn new(owner: UserId, dealer: UserId) -> Self {
        Self {
            user_accounts: HashMap::new(),
            insurance_fund_account: Account::new(Currency::BTC, AccountType::Internal, AccountClass::Cash),
            fee_account: UserAccount::new(owner),
            bank_liabilities: UserAccount::new(owner),
            dealer_accounts: UserAccount::new(dealer),
            external_fee_account: Account::new(Currency::BTC, AccountType::External, AccountClass::Cash),
        }
    }
}
