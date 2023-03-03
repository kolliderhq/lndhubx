use crate::{
    into_relays, request_user_profile, send_nostr_private_msg, DbPool, InternalNostrProfileRequest, NostrEngineEvent,
    NostrProfileUpdate, API_PROFILE_TIMEOUT,
};
use core_types::nostr::NostrProfile;
use diesel::QueryResult;
use models::nostr_profiles::NostrProfileRecord;
use msgs::api::{NostrResponseError, ShareableNostrProfile};
use msgs::nostr::NostrZapNote;
use msgs::Message;
use nostr_sdk::nostr::{Event, Url};
use nostr_sdk::{Client, Options};
use slog as log;
use slog::Logger;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
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
                    let sharable_profile = ShareableNostrProfile::from(cached_profile_update);
                    let resp = msgs::api::NostrProfileSearchResponse {
                        req_id: req.req_id,
                        data: vec![sharable_profile],
                        error: None,
                    };
                    let message = Message::Api(msgs::api::Api::NostrProfileSearchResponse(resp));
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
                let (data, error) = match self.search_profile_by_text(req.pubkey.clone(), req.text.clone(), req.limit) {
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
                self.send_zap_note(zap).await;
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

    fn search_profile_by_text(
        &self,
        pubkey: Option<String>,
        text: Option<String>,
        limit: Option<u64>,
    ) -> QueryResult<Vec<ShareableNostrProfile>> {
        if let Some(conn) = self.db_pool.try_get() {
            let found_profiles = NostrProfileRecord::search_by_text(&conn, pubkey, text, limit)?;
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
                                validated_lnurl_pay_req: record.lnurl_pay_req,
                            }
                        })
                })
                .collect();
            Ok(nostr_profiles)
        } else {
            Err(diesel::result::Error::NotFound)
        }
    }

    pub async fn initialize_profile_cache(&mut self) {
        let maybe_profile_records = match self.db_pool.try_get() {
            Some(conn) => NostrProfileRecord::fetch_all(&conn).ok(),
            None => {
                log::error!(
                    self.logger,
                    "Failed to initialize profile cache. Could not get DB connection"
                );
                return;
            }
        };
        if let Some(profile_records) = maybe_profile_records {
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
                                    validated_lnurl_pay_req: record.lnurl_pay_req,
                                };
                                (record.pubkey, profile_update)
                            })
                    }
                })
                .collect();
            self.nostr_profile_cache = cache;
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
                let shareable_profile = ShareableNostrProfile::from(profile_update);
                let resp = msgs::api::NostrProfileSearchResponse {
                    req_id,
                    data: vec![shareable_profile],
                    error: None,
                };
                let message = Message::Api(msgs::api::Api::NostrProfileSearchResponse(resp));
                self.send_to_bank(message).await;
            }
        }
    }

    async fn send_zap_note(&self, zap: &NostrZapNote) {
        let (zap_note, zap_note_relays) = match utils::nostr::create_zap_note(
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
                log::error!(
                    self.logger,
                    "Could not create a zap note from data: {:?}, error: {:?}",
                    zap,
                    err
                );
                return;
            }
        };
        let client_relays = self
            .nostr_client
            .relays()
            .await
            .keys()
            .map(|url| url.to_string())
            .collect::<HashSet<_>>();
        let (existing_pool_relays, unknown_relays): (Vec<_>, Vec<_>) = zap_note_relays
            .into_iter()
            .filter_map(|url| Url::parse(&url).ok().map(|url| url.to_string()))
            .partition(|url| client_relays.contains(url));
        send_to_relays(&self.nostr_client, &zap_note, &existing_pool_relays, &self.logger).await;
        let keys = self.nostr_client.keys();
        let task_logger = self.logger.clone();
        tokio::spawn(async move {
            let options = Options::new().wait_for_connection(true).wait_for_send(true);
            let nostr_client = Client::new_with_opts(&keys, options);
            nostr_client.add_relays(into_relays(unknown_relays)).await.unwrap();
            nostr_client.connect().await;
            if let Err(err) = nostr_client.send_event(zap_note.clone()).await {
                log::error!(
                    task_logger,
                    "Failed to send a zap note: {:?}, error: {:?}",
                    zap_note,
                    err
                );
            }
        });
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
        lnurl_pay_req: profile_update.validated_lnurl_pay_req.clone(),
    };
    record.upsert(conn)
}

async fn send_to_relays(nostr_client: &Client, event: &Event, relays: &[String], logger: &Logger) {
    for url in relays.iter() {
        if let Err(err) = nostr_client.send_event_to(url.clone(), event.clone()).await {
            log::error!(
                logger,
                "Failed to send an event: {:?} to {}, error: {:?}",
                event,
                url,
                err
            );
        }
    }
}
