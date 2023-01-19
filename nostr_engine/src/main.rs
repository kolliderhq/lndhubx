use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
};

use nostr_rust::{
    events::extract_events_ws, nostr_client::Client, req::ReqFilter, utils::parse_content_tags, bech32::{ToBech32Kind, from_hb_to_hex}, 
    Identity as NostrIdentity, Message as NostrMessage,
};

use utils::xzmq::SocketContext;

use core_types::{nostr::NostrProfile, *};
use crossbeam_channel::bounded;
use msgs::{*, nostr::{*}};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

fn get_user_profile(client: Arc<Mutex<Client>>, pubkey: String) -> Option<NostrProfile> {
    let hex = from_hb_to_hex(ToBech32Kind::PublicKey, &pubkey).unwrap();
    let subscription_id = client
        .lock()
        .unwrap()
        .subscribe(vec![ReqFilter {
            ids: None,
            authors: Some(vec![hex]),
            kinds: Some(vec![0]),
            e: None,
            p: None,
            since: None,
            until: None,
            limit: Some(1),
        }])
        .unwrap();

    client.lock().unwrap().unsubscribe(&subscription_id).unwrap();
    let events = client.lock().unwrap().next_data().unwrap();

    for (_relay_url, message) in events.iter() {
        let events = extract_events_ws(message);
        for event in events {
            let content_str = event.content.clone();
            let nostr_profile = serde_json::from_str::<NostrProfile>(&content_str).unwrap();
            return Some(nostr_profile);
        }
    }
    return None;
}

fn send_nostr_private_msg(client: Arc<Mutex<Client>>, identity: &NostrIdentity, pubkey: &String, text: &String) {
    let hex = from_hb_to_hex(ToBech32Kind::PublicKey, &pubkey).unwrap();
    client.lock().unwrap().send_private_message(identity, pubkey, text, 0).unwrap();
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

    let my_identity =
        NostrIdentity::from_str(settings.nostr_private_key).unwrap();

    let nostr_client = Arc::new(Mutex::new(
        Client::new(vec![
            "wss://relay.nostr.info",
            // "wss://nostr-pub.wellorder.net",
            // "wss://relay.damus.io",
            // "wss://nostr.zebedee.cloud",
            // "wss://nostr.bitcoiner.social",
        ])
        .unwrap(),
    ));

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
                        send_nostr_private_msg(nostr_client.clone(), &my_identity, &req.pubkey, &req.text);
                    }
                    _ => {}
                }
            };
        }

        dbg!("cycle");
    }

    println!("Hello, world!");
}
