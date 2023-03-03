use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrPrivateMessage {
    pub pubkey: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrZapNote {
    pub amount: u64,
    pub description: String,
    pub description_hash: String,
    pub bolt11: String,
    pub preimage: Option<String>,
    pub settled_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrProfilesRefetchRequest {
    pub pubkey: Option<String>,
    pub since_epoch_ms: Option<u64>,
    pub until_epoch_ms: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Nostr {
    NostrPrivateMessage(NostrPrivateMessage),
    NostrZapNote(NostrZapNote),
    NostrProfilesRefetchRequest(NostrProfilesRefetchRequest),
}
