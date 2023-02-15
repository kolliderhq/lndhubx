use crate::ledger::Ledger;

use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use core_types::{Account, Currency};
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

    if !error.accounts.is_empty() {
        return Err(error);
    }

    ledger.bank_liabilities.accounts.iter().for_each(|(_acc_id, acc)| {
        let mut tally = user_accounts_by_currency.entry(acc.currency).or_insert(dec!(0));
        tally += acc.balance
    });

    ledger.dealer_accounts.accounts.iter().for_each(|(_acc_id, acc)| {
        let mut tally = user_accounts_by_currency.entry(acc.currency).or_insert(dec!(0));
        tally += acc.balance
    });

    user_accounts_by_currency.iter().for_each(|(curr, balance)| {
        if *balance != dec!(0) {
            error.net_zero.push((*curr, *balance));
        }
    });

    if !error.net_zero.is_empty() {
        return Err(error);
    }
    Ok(())
}
