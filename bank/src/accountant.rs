use crate::ledger::Ledger;

use rust_decimal_macros::*;

use core_types::Account;

#[derive(Debug)]
pub struct ReconcilationError {
	accounts: Vec<Account>,
	net_zero: bool,
}

pub fn reconcile_ledger(ledger: &Ledger) -> Result<(), ReconcilationError> {
	let total_user_balances = dec!(0);
	let mut error = ReconcilationError {
		accounts: Vec::new(),
		net_zero: true,
	};
	ledger.user_accounts.iter().for_each(|(uid, ua)| {
		ua.accounts.iter().for_each(|(account_id, acc)| {
			if acc.balance < dec!(0) {
				error.accounts.push(acc.clone());
			}
		});
	});

	if error.accounts.len() > 0 {
		return Err(error)
	}
	if !error.net_zero {
		return Err(error)
	}
	Ok(())
}