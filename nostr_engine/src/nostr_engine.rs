use crate::{
    request_user_profile, send_nostr_private_msg, DbPool, NostrEngineEvent, NostrProfileUpdate, API_PROFILE_TIMEOUT,
};
use diesel::QueryResult;
use models::nostr_profiles::NostrProfileRecord;
use msgs::api::{NostrResponseError, PayableNostrProfile};
use msgs::Message;
use nostr_sdk::Client;
use slog as log;
use slog::Logger;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use uuid::Uuid;

pub struct NostrEngine {
    nostr_profile_cache: HashMap<String, NostrProfileUpdate>,
    nostr_profile_pending: HashMap<String, (u64, Uuid)>,
    bank_tx_sender: tokio::sync::mpsc::Sender<Message>,
    nostr_client: Client,
    db_pool: DbPool,
    logger: Logger,
}

impl NostrEngine {
    pub async fn new(
        nostr_client: Client,
        bank_tx_sender: tokio::sync::mpsc::Sender<Message>,
        db_pool: DbPool,
        logger: Logger,
    ) -> Self {
        Self {
            nostr_profile_cache: HashMap::new(),
            nostr_profile_pending: HashMap::new(),
            bank_tx_sender,
            nostr_client,
            db_pool,
            logger,
        }
    }

    pub async fn process_event(&mut self, event: &NostrEngineEvent) {
        log::trace!(self.logger, "Processing event: {:?}", event);
        match event {
            NostrEngineEvent::NostrProfileUpdate(profile_update) => {
                self.store_payable_profile(profile_update).await;
            }
            NostrEngineEvent::LndhubxMessage(message) => {
                self.process_message(message).await;
            }
        }
    }

    async fn process_message(&mut self, message: &Message) {
        match message {
            Message::Api(msgs::api::Api::NostrProfileRequest(req)) => {
                let pubkey = match req.pubkey {
                    Some(ref key) => key,
                    None => return,
                };

                if let Some(cached_profile_update) = self.nostr_profile_cache.get(pubkey) {
                    let resp = msgs::api::NostrProfileResponse {
                        req_id: req.req_id,
                        profile: Some(cached_profile_update.nostr_profile.clone()),
                        error: None,
                    };
                    let message = Message::Api(msgs::api::Api::NostrProfileResponse(resp));
                    self.send_to_bank(message).await;
                } else {
                    self.nostr_profile_pending
                        .insert(pubkey.clone(), (utils::time::time_now(), req.req_id));
                    request_user_profile(&self.nostr_client, pubkey).await;
                }
            }
            Message::Api(msgs::api::Api::NostrProfileSearchRequest(req)) => {
                let (data, error) = match self.search_profile_by_text(&req.text) {
                    Ok(profiles) => (profiles, None),
                    Err(_) => (Vec::new(), Some(NostrResponseError::ProfileNotFound)),
                };
                let resp = msgs::api::NostrProfileSearchResponse {
                    req_id: req.req_id,
                    data,
                    error,
                };
                let message = Message::Api(msgs::api::Api::NostrProfileSearchResponse(resp));
                self.send_to_bank(message).await;
            }
            Message::Nostr(msgs::nostr::Nostr::NostrPrivateMessage(req)) => {
                send_nostr_private_msg(&self.nostr_client, &req.pubkey, &req.text).await;
            }
            _ => {}
        }
    }

    async fn store_payable_profile(&mut self, profile_update: &NostrProfileUpdate) {
        let lud06 = profile_update.nostr_profile.lud06().clone().unwrap_or_default();
        let lud16 = profile_update.nostr_profile.lud16().clone().unwrap_or_default();
        if !lud06.is_empty() || !lud16.is_empty() {
            let inserted = match self.nostr_profile_cache.entry(profile_update.pubkey.clone()) {
                Entry::Occupied(mut entry) => {
                    let existing_created_time_ms = entry.get().created_at_epoch_ms;
                    if existing_created_time_ms < profile_update.created_at_epoch_ms {
                        entry.insert(profile_update.clone());
                        true
                    } else {
                        false
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(profile_update.clone());
                    true
                }
            };
            if inserted {
                if let Some((pending_request_time_ms, req_id)) =
                    self.nostr_profile_pending.get(&profile_update.pubkey).cloned()
                {
                    let time_now_ms = utils::time::time_now();
                    let elapsed = time_now_ms - pending_request_time_ms;
                    if elapsed < API_PROFILE_TIMEOUT {
                        let resp = msgs::api::NostrProfileResponse {
                            req_id,
                            profile: Some(profile_update.nostr_profile.clone()),
                            error: None,
                        };
                        let message = Message::Api(msgs::api::Api::NostrProfileResponse(resp));
                        self.send_to_bank(message).await;
                    }
                }
            }
            if let Some(conn) = self.db_pool.try_get() {
                if let Err(err) = insert_profile_update(&conn, profile_update) {
                    log::error!(
                        self.logger,
                        "Failed to upsert nostr profile update: {:?}, err: {:?}",
                        profile_update,
                        err
                    );
                }
            }
        }
        let time_now_ms = utils::time::time_now();
        self.nostr_profile_pending
            .retain(|_pubkey, (request_time_ms, _request_id)| {
                let elapsed = time_now_ms - *request_time_ms;
                elapsed >= 2 * API_PROFILE_TIMEOUT
            });
    }

    fn search_profile_by_text(&self, text: &str) -> QueryResult<Vec<PayableNostrProfile>> {
        if let Some(conn) = self.db_pool.try_get() {
            let found_profiles = NostrProfileRecord::search_by_text(&conn, text)?;
            let payable_profiles = found_profiles
                .into_iter()
                .map(|record| PayableNostrProfile {
                    pubkey: record.pubkey,
                    created_at: record.created_at,
                    received_at: record.received_at,
                    name: record.name,
                    display_name: record.display_name,
                    nip05: record.nip05,
                    lud06: record.lud06,
                    lud16: record.lud16,
                    nip05_verified: record.nip05_verified,
                })
                .collect();
            Ok(payable_profiles)
        } else {
            Err(diesel::result::Error::NotFound)
        }
    }

    async fn send_to_bank(&self, message: Message) {
        if let Err(err) = self.bank_tx_sender.send(message).await {
            log::error!(self.logger, "Failed to send message to bank_tx, error: {:?}", err);
        }
    }
}

fn insert_profile_update(conn: &diesel::PgConnection, profile_update: &NostrProfileUpdate) -> QueryResult<usize> {
    let record = NostrProfileRecord {
        pubkey: profile_update.pubkey.clone(),
        created_at: profile_update.created_at_epoch_ms as i64,
        received_at: profile_update.received_at_epoch_ms as i64,
        name: profile_update.nostr_profile.name().clone(),
        display_name: profile_update.nostr_profile.display_name().clone(),
        nip05: profile_update.nostr_profile.nip05().clone(),
        lud06: profile_update.nostr_profile.lud06().clone(),
        lud16: profile_update.nostr_profile.lud16().clone(),
        nip05_verified: profile_update.nostr_profile.nip05_verified(),
    };
    record.upsert(conn)
}
