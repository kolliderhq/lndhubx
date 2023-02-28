use crate::{
    request_user_profile, send_nostr_private_msg, DbPool, InternalNostrProfileRequest, NostrEngineEvent,
    NostrProfileUpdate, API_PROFILE_TIMEOUT,
};
use core_types::nostr::NostrProfile;
use diesel::QueryResult;
use models::nostr_profiles::NostrProfileRecord;
use msgs::api::{NostrResponseError, ShareableNostrProfile};
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
                self.store_profile(profile_update).await;
            }
            NostrEngineEvent::LndhubxMessage(message) => {
                self.process_message(message).await;
            }
            NostrEngineEvent::InternalNostrProfileRequest(request) => {
                request_user_profile(&self.nostr_client, request).await;
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
                    let internal_request = InternalNostrProfileRequest {
                        pubkey: Some(pubkey.clone()),
                        since: None,
                        until: None,
                        limit: Some(1),
                    };
                    request_user_profile(&self.nostr_client, &internal_request).await;
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
            Message::Nostr(msgs::nostr::Nostr::NostrZapNote(zap)) => {
                let (zap_note, relays) = match utils::nostr::create_zap_note(
                    &self.nostr_client.keys(),
                    zap.amount,
                    &zap.description,
                    &zap.description_hash,
                    &zap.bolt11,
                    &zap.preimage,
                    zap.settled_timestamp,
                ) {
                    Ok(success) => success,
                    Err(err) => {
                        log::error!(self.logger, "Could not create a zap note, error: {:?}", err);
                        return;
                    }
                };
                for url in relays.iter() {
                    if let Err(err) = self.nostr_client.send_event_to(url.clone(), zap_note.clone()).await {
                        log::info!(
                            self.logger,
                            "Failed to send a zap note: {:?} to {}, error: {:?}",
                            zap_note,
                            url,
                            err
                        );
                    }
                }
            }
            _ => {}
        }
    }

    async fn store_profile(&mut self, profile_update: &NostrProfileUpdate) {
        let inserted = match self.nostr_profile_cache.entry(profile_update.pubkey.clone()) {
            Entry::Occupied(mut entry) => {
                let existing_created_time_ms = entry.get().created_at_epoch_ms;
                let existing_nip05_verified_status = entry.get().nostr_profile.nip05_verified();
                if existing_created_time_ms < profile_update.created_at_epoch_ms
                    || (existing_created_time_ms == profile_update.created_at_epoch_ms
                        && existing_nip05_verified_status != profile_update.nostr_profile.nip05_verified())
                {
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
            self.reply_if_pending(profile_update).await;
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
        let time_now_ms = utils::time::time_now();
        self.nostr_profile_pending
            .retain(|_pubkey, (request_time_ms, _request_id)| {
                let elapsed = time_now_ms - *request_time_ms;
                elapsed >= 2 * API_PROFILE_TIMEOUT
            });
    }

    fn search_profile_by_text(&self, text: &str) -> QueryResult<Vec<ShareableNostrProfile>> {
        if let Some(conn) = self.db_pool.try_get() {
            let found_profiles = NostrProfileRecord::search_by_text(&conn, text)?;
            let nostr_profiles = found_profiles
                .into_iter()
                .filter_map(|record| {
                    serde_json::from_str::<NostrProfile>(&record.content)
                        .ok()
                        .map(|mut profile| {
                            profile.set_nip05_verified(record.nip05_verified);
                            ShareableNostrProfile {
                                pubkey: record.pubkey,
                                created_at: record.created_at / 1000,
                                profile,
                            }
                        })
                })
                .collect();
            Ok(nostr_profiles)
        } else {
            Err(diesel::result::Error::NotFound)
        }
    }

    pub fn initialize_profile_cache(&mut self) {
        match self.db_pool.try_get() {
            Some(conn) => {
                if let Ok(profile_records) = NostrProfileRecord::fetch_all(&conn) {
                    let cache = profile_records
                        .into_iter()
                        .filter_map(|record| {
                            if record.content.is_empty() {
                                None
                            } else {
                                serde_json::from_str::<NostrProfile>(&record.content)
                                    .ok()
                                    .map(|mut profile| {
                                        profile.set_nip05_verified(record.nip05_verified);
                                        let profile_update = NostrProfileUpdate {
                                            pubkey: record.pubkey.clone(),
                                            content: record.content,
                                            created_at_epoch_ms: record.created_at as u64,
                                            received_at_epoch_ms: record.received_at as u64,
                                            nostr_profile: profile,
                                        };
                                        (record.pubkey, profile_update)
                                    })
                            }
                        })
                        .collect();
                    self.nostr_profile_cache = cache;
                }
            }
            None => {
                log::error!(
                    self.logger,
                    "Failed to initialize profile cache. Could not get DB connection"
                );
            }
        }
    }

    async fn send_to_bank(&self, message: Message) {
        if let Err(err) = self.bank_tx_sender.send(message).await {
            log::error!(self.logger, "Failed to send message to bank_tx, error: {:?}", err);
        }
    }

    async fn reply_if_pending(&self, profile_update: &NostrProfileUpdate) {
        if let Some((pending_request_time_ms, req_id)) = self.nostr_profile_pending.get(&profile_update.pubkey).cloned()
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
}

fn insert_profile_update(conn: &diesel::PgConnection, profile_update: &NostrProfileUpdate) -> QueryResult<usize> {
    let record = NostrProfileRecord {
        pubkey: profile_update.pubkey.clone(),
        created_at: profile_update.created_at_epoch_ms as i64,
        received_at: profile_update.received_at_epoch_ms as i64,
        name: profile_update.nostr_profile.name().clone(),
        display_name: profile_update.nostr_profile.display_name().clone(),
        nip05: profile_update.nostr_profile.nip05().clone(),
        lud16: profile_update.nostr_profile.lud16().clone(),
        nip05_verified: profile_update.nostr_profile.nip05_verified(),
        content: profile_update.content.clone(),
    };
    record.upsert(conn)
}
