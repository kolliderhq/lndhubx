use std::collections::HashMap;
use std::thread;

use core_types::nostr::NostrProfile;
use msgs::{nostr::*, *};
use nostr_sdk::blocking::Client;
use nostr_sdk::prelude::{FromPkStr, FromSkStr, Keys, Kind, SubscriptionFilter};
use serde::{Deserialize, Serialize};
use utils::xzmq::SocketContext;

fn get_user_profile(client: Client, pubkey: String) -> Option<NostrProfile> {
    let keys = Keys::from_pk_str(&pubkey).unwrap();

    let subscription = SubscriptionFilter::new()
        .author(keys.public_key())
        .kind(Kind::Metadata)
        .limit(1);
    let events = client.get_events_of(vec![subscription]).unwrap();

    if let Some(event) = events.first() {
        let nostr_profile = serde_json::from_str::<NostrProfile>(&event.content).unwrap();
        Some(nostr_profile)
    } else {
        None
    }
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
            // "wss://nostr-pub.wellorder.net",
            // "wss://relay.damus.io",
            // "wss://nostr.zebedee.cloud",
            // "wss://nostr.bitcoiner.social",
        ])
        .unwrap();

    nostr_client.connect();

    loop {
        thread::sleep(std::time::Duration::from_secs(1));

        if let Ok(frame) = bank_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                match message {
                    Message::Api(api::Api::NostrProfileRequest(req)) => {
                        let pubkey = req.pubkey.unwrap();

                        let mut resp = api::NostrProfileResponse {
                            req_id: req.req_id,
                            profile: None,
                            error: None,
                        };

                        if let Some(p) = nostr_profile_cache.get(&pubkey) {
                            resp.profile = Some(p.clone());
                            let message = Message::Api(api::Api::NostrProfileResponse(resp));
                            utils::xzmq::send_as_bincode(&bank_tx, &message);
                        } else {
                            let profile = get_user_profile(nostr_client.clone(), pubkey.clone());
                            if let Some(p) = profile.clone() {
                                nostr_profile_cache.insert(pubkey.clone(), p);
                            }
                            resp.profile = profile;
                            let message = Message::Api(api::Api::NostrProfileResponse(resp));
                            utils::xzmq::send_as_bincode(&bank_tx, &message);
                        }
                    }
                    Message::Nostr(Nostr::NostrPrivateMessage(req)) => {
                        send_nostr_private_msg(nostr_client.clone(), &req.pubkey, &req.text);
                    }
                    _ => {}
                }
            };
        }

        dbg!("cycle");
    }
}
