use crate::ledger::Ledger;

use rust_decimal_macros::*;
use rust_decimal::prelude::*;

use core_types::{Currency, Account};
use std::collections::HashMap;

#[derive(Debug)]
pub struct ReconcilationError {
    accounts: Vec<Account>,
    net_zero: Vec<(Currency, Decimal)>,
}

pub fn reconcile_ledger(ledger: &Ledger) -> Result<(), ReconcilationError> {
    let _total_user_balances = dec!(0);
    let mut error = ReconcilationError {
        accounts: Vec::new(),
        net_zero: Vec::new(),
    };

    let mut user_accounts_by_currency = HashMap::new();

    ledger.user_accounts.iter().for_each(|(_uid, ua)| {
        ua.accounts.iter().for_each(|(_account_id, acc)| {
            if acc.balance < dec!(0) {
                error.accounts.push(acc.clone());
            }
            let mut tally = user_accounts_by_currency.entry(acc.currency).or_insert(dec!(0));
            tally += acc.balance
        });
    });

    let mut tally = user_accounts_by_currency.entry(Currency::BTC).or_insert(dec!(0));
    tally += ledger.insurance_fund_account.balance;

    if !error.accounts.is_empty() {
        return Err(error);
    }

    ledger.liabilities.accounts.iter().for_each(|(_acc_id, acc)| {
        let net_zero = acc.balance.abs() - user_accounts_by_currency.get(&acc.currency).unwrap().abs();
        if net_zero.abs() > dec!(0) {
            error.net_zero.push((acc.currency, net_zero));
        }
    });

    if !error.net_zero.is_empty() {
        return Err(error);
    }
    Ok(())
}
