use core_types::*;

use crate::api::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub uid: UserId,
    pub currency: Currency,
    pub rate: Rate,
    pub is_success: bool,
    pub amount: Money,
    pub payment_response: PaymentResponse,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Bank {
    PaymentResult(PaymentResult),
}
