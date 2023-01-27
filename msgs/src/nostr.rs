use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrPrivateMessage {
    pub pubkey: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Nostr {
    NostrPrivateMessage(NostrPrivateMessage),
}
