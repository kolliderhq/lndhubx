use crate::{get_user_profile, send_nostr_private_msg, DbPool, NostrEngineEvent, NostrProfileUpdate};
use core_types::nostr::NostrProfile;
use diesel::QueryResult;
use models::nostr_profiles::NostrProfileRecord;
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
                if profile_update.nostr_profile.lud06().is_some() || profile_update.nostr_profile.lud16().is_some() {
                    self.nostr_profile_cache
                        .insert(profile_update.pubkey.clone(), profile_update.nostr_profile.clone());
                    if let Some(conn) = self.db_pool.try_get() {
                        match insert_profile_update(&conn, profile_update) {
                            Ok(_) => {
                                if let Some(nip05) = profile_update.nostr_profile.nip05().as_ref() {
                                    let verification_result = self.verify_nip05(&profile_update.pubkey, nip05);
                                    if let Err(err) = NostrProfileRecord::update_nip05_verified(
                                        &conn,
                                        &profile_update.pubkey,
                                        nip05,
                                        verification_result,
                                    ) {
                                        log::error!(
                                    self.logger,
                                    "Failed to set nip05 verification result: {:?} for profile pubkey: {}, nip05: {}, err: {:?}",
                                    verification_result, profile_update.pubkey, nip05, err
                                );
                                    }
                                }
                            }
                            Err(err) => {
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
                } else {
                    let maybe_profile = get_user_profile(&self.nostr_client, pubkey).await;
                    if let Some(ref profile) = maybe_profile {
                        self.nostr_profile_cache.insert(pubkey.clone(), profile.clone());
                    }
                    maybe_profile
                };

                let resp = msgs::api::NostrProfileResponse {
                    req_id: req.req_id,
                    profile: response_profile,
                    error: None,
                };
                let message = Message::Api(msgs::api::Api::NostrProfileResponse(resp));
                utils::xzmq::send_as_bincode(&self.response_socket, &message);
            }
            Message::Nostr(msgs::nostr::Nostr::NostrPrivateMessage(req)) => {
                send_nostr_private_msg(&self.nostr_client, &req.pubkey, &req.text).await;
            }
            _ => {}
        }
    }

    fn verify_nip05(&self, pubkey: &str, nip05: &str) -> Option<bool> {
        // todo run verification if nip05 present
        None
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
        nip05_verified: None,
    };
    record.upsert(conn)
}
