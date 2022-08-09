use core_types::*;
use rust_decimal::prelude::*;
use std::collections::HashMap;

use crate::api::*;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
	pub uid: UserId,
	pub currency: Currency,
	pub rate: Decimal,
	pub is_success: bool,
	pub amount: Decimal,
	pub payment_response: PaymentResponse,
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Bank {
	PaymentResult(PaymentResult),
}