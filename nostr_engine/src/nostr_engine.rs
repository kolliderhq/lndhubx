use crate::{get_user_profile, send_nostr_private_msg, NostrEngineEvent};
use core_types::nostr::NostrProfile;
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
    logger: Logger,
}

impl NostrEngine {
    pub async fn new(
        nostr_engine_keys: Keys,
        relays: Vec<(String, Option<SocketAddr>)>,
        response_socket: ZmqSocket,
        logger: Logger,
    ) -> Self {
        let nostr_client = Client::new(&nostr_engine_keys);
        nostr_client.add_relays(relays).await.unwrap();
        nostr_client.connect().await;

        Self {
            nostr_profile_cache: HashMap::new(),
            response_socket,
            nostr_client,
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
                }
            }
            NostrEngineEvent::LndhubxMessage(message) => {
                self.process_message(message).await;
            }
        }
    }

    pub async fn process_message(&mut self, message: &Message) {
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
}
