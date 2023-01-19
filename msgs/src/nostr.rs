use core_types::*;
use rust_decimal::prelude::Decimal;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrPrivateMessage {
	pub pubkey: String,
	pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Nostr {
	NostrPrivateMessage(NostrPrivateMessage)
}