use crate::{get_user_profile, send_nostr_private_msg, DbPool, NostrEngineEvent, NostrProfileUpdate};
use core_types::nostr::NostrProfile;
use diesel::QueryResult;
use models::nostr_profiles::NostrProfileRecord;
use msgs::api::{NostrResponseError, PayableNostrProfile};
use msgs::Message;
use nostr_sdk::prelude::Keys;
use nostr_sdk::Client;
use slog as log;
use slog::Logger;
use std::collections::HashMap;
use std::net::SocketAddr;
use utils::xzmq::ZmqSocket;

pub struct NostrEngine {
    nostr_profile_cache: HashMap<String, NostrProfile>,
    response_socket: ZmqSocket,
    nostr_client: Client,
    db_pool: DbPool,
    logger: Logger,
}

impl NostrEngine {
    pub async fn new(
        nostr_engine_keys: Keys,
        relays: Vec<(String, Option<SocketAddr>)>,
        response_socket: ZmqSocket,
        db_pool: DbPool,
        logger: Logger,
    ) -> Self {
        let nostr_client = Client::new(&nostr_engine_keys);
        nostr_client.add_relays(relays).await.unwrap();
        nostr_client.connect().await;

        Self {
            nostr_profile_cache: HashMap::new(),
            response_socket,
            nostr_client,
            db_pool,
            logger,
        }
    }

    pub async fn process_event(&mut self, event: &NostrEngineEvent) {
        log::trace!(self.logger, "Processing event: {:?}", event);
        match event {
            NostrEngineEvent::NostrProfileUpdate(profile_update) => {
                self.store_payable_profile(profile_update);
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

                let response_profile = if let Some(profile) = self.nostr_profile_cache.get(pubkey) {
                    Some(profile.clone())
                } else if let Some(profile_update) = get_user_profile(&self.nostr_client, pubkey).await {
                    self.store_payable_profile(&profile_update);
                    Some(profile_update.nostr_profile)
                } else {
                    None
                };

                let resp = msgs::api::NostrProfileResponse {
                    req_id: req.req_id,
                    profile: response_profile,
                    error: None,
                };
                let message = Message::Api(msgs::api::Api::NostrProfileResponse(resp));
                utils::xzmq::send_as_bincode(&self.response_socket, &message);
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
                utils::xzmq::send_as_bincode(&self.response_socket, &message);
            }
            Message::Nostr(msgs::nostr::Nostr::NostrPrivateMessage(req)) => {
                send_nostr_private_msg(&self.nostr_client, &req.pubkey, &req.text).await;
            }
            _ => {}
        }
    }

    fn store_payable_profile(&mut self, profile_update: &NostrProfileUpdate) {
        let lud06 = profile_update.nostr_profile.lud06().clone().unwrap_or_default();
        let lud16 = profile_update.nostr_profile.lud16().clone().unwrap_or_default();
        if !lud06.is_empty() || !lud16.is_empty() {
            self.nostr_profile_cache
                .insert(profile_update.pubkey.clone(), profile_update.nostr_profile.clone());
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
