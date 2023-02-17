use std::collections::HashMap;
use std::thread;

use core_types::nostr::NostrProfile;
use msgs::{nostr::*, *};
use nostr_sdk::blocking::Client;
use nostr_sdk::prelude::{FromPkStr, FromSkStr, Keys, Kind, SubscriptionFilter};
use serde::{Deserialize, Serialize};
use utils::xzmq::SocketContext;

const PROFILE_REQUEST_TIMEOUT_MS: u64 = 100;

fn get_user_profile(client: &Client, pubkey: &str) -> Option<NostrProfile> {
    let keys = Keys::from_pk_str(pubkey).ok()?;

    let subscription = SubscriptionFilter::new()
        .author(keys.public_key())
        .kind(Kind::Metadata)
        .limit(1);

    let timeout = std::time::Duration::from_millis(PROFILE_REQUEST_TIMEOUT_MS);
    let events = client.get_events_of(vec![subscription], Some(timeout)).ok()?;

    events
        .first()
        .and_then(|event| serde_json::from_str::<NostrProfile>(&event.content).ok())
}

fn send_nostr_private_msg(client: Client, pubkey: &str, text: &str) {
    let keys = Keys::from_pk_str(pubkey).unwrap();
    client.send_direct_msg(keys.public_key(), text).unwrap();
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NostrEngineSettings {
    pub nostr_bank_push_address: String,
    pub nostr_bank_pull_address: String,
    pub nostr_private_key: String,
}

fn main() {
    let settings = utils::config::get_config_from_env::<NostrEngineSettings>().expect("Failed to load settings.");

    let context = SocketContext::new();
    let bank_recv = context.create_pull(&settings.nostr_bank_pull_address);
    let bank_tx = context.create_push(&settings.nostr_bank_push_address);

    let mut nostr_profile_cache: HashMap<String, NostrProfile> = HashMap::new();

    let keys = Keys::from_sk_str(&settings.nostr_private_key).unwrap();
    let nostr_client = Client::new(&keys);

    nostr_client
        .add_relays(vec![
            ("wss://relay.nostr.info", None),
            ("wss://nostr-pub.wellorder.net", None),
            ("wss://relay.damus.io", None),
            // ("wss://nostr.zebedee.cloud", None),
            // ("wss://nostr.bitcoiner.social", None),
        ])
        .unwrap();

    nostr_client.connect();

    loop {
        thread::sleep(std::time::Duration::from_micros(100));

        while let Ok(frame) = bank_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                match message {
                    Message::Api(api::Api::NostrProfileRequest(req)) => {
                        let pubkey = match req.pubkey {
                            Some(key) => key,
                            None => return,
                        };

                        let response_profile = if let Some(profile) = nostr_profile_cache.get(&pubkey) {
                            Some(profile.clone())
                        } else {
                            let maybe_profile = get_user_profile(&nostr_client, &pubkey);
                            if let Some(ref profile) = maybe_profile {
                                nostr_profile_cache.insert(pubkey, profile.clone());
                            }
                            maybe_profile
                        };

                        let resp = api::NostrProfileResponse {
                            req_id: req.req_id,
                            profile: response_profile,
                            error: None,
                        };
                        let message = Message::Api(api::Api::NostrProfileResponse(resp));
                        utils::xzmq::send_as_bincode(&bank_tx, &message);
                    }
                    Message::Nostr(Nostr::NostrPrivateMessage(req)) => {
                        send_nostr_private_msg(nostr_client.clone(), &req.pubkey, &req.text);
                    }
                    _ => {}
                }
            };
        }
    }
}
