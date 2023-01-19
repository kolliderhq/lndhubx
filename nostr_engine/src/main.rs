use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
};

use nostr_rust::{nostr_client::Client, req::ReqFilter, Identity as NostrIdentity, Message as NostrMessage, events::extract_events_ws, utils::parse_content_tags};

use utils::xzmq::SocketContext;

use core_types::{*, nostr::NostrProfile};
use crossbeam_channel::bounded;
use msgs::*;

use serde::{Serialize, Deserialize};

fn get_user_profile(client: Arc<Mutex<Client>>, pubkey:String) -> Option<NostrProfile> {
    let subscription_id = client
    .lock()
    .unwrap()
    .subscribe(vec![ReqFilter {
        ids: None,
        authors: Some(vec![
            pubkey,
        ]),
        kinds: Some(vec![0]),
        e: None,
        p: None,
        since: None,
        until: None,
        limit: Some(1),
    }])
    .unwrap();

    client
        .lock()
        .unwrap()
        .unsubscribe(&subscription_id)
        .unwrap();

    let events = client.lock().unwrap().next_data().unwrap();

    for (_relay_url, message) in events.iter() {
        let events = extract_events_ws(message);
        for event in events {
            let content_str = event.content.clone();
            let nostr_profile = serde_json::from_str::<NostrProfile>(&content_str).unwrap();
            return Some(nostr_profile)
        }
    }
    return None
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NostrEngineSettings {
    pub nostr_bank_push_address: String,
    pub nostr_bank_pull_address: String,
}

fn main() {
    
    let settings = utils::config::get_config_from_env::<NostrEngineSettings>().expect("Failed to load settings.");

    let context = SocketContext::new();
    let bank_recv = context.create_pull(&settings.nostr_bank_pull_address);
    let bank_tx = context.create_publisher(&settings.nostr_bank_push_address);
    //nsec1pxx6xfhfmjnyn8c55j62n79q8zy8fcqkv4d5t0nh7kr4smfwhnzs7ucnpj

    //nsec1wud2pmpy5cjzv5qm4dgjfl5lmlecz6cut0fm0twtrw47p0t8jq7sfj6yk8

    let my_identity =
        NostrIdentity::from_str("nsec1wud2pmpy5cjzv5qm4dgjfl5lmlecz6cut0fm0twtrw47p0t8jq7sfj6yk8")
            .unwrap();

    let nostr_client = Arc::new(Mutex::new(
        Client::new(vec!["wss://relay.nostr.info"]).unwrap(),
    ));

    // Run a new thread to handle messages
    // let nostr_clone = nostr_client.clone();
    // let handle_thread = thread::spawn(move || {
    //     println!("Listening...");
    //     let events = nostr_clone.lock().unwrap().next_data().unwrap();

    //     for (relay_url, message) in events.iter() {
    //         handle_message(relay_url, message).unwrap();
    //     }
    // });

    let profile = get_user_profile(nostr_client.clone(), String::from("884704bd421721e292edbff42eb77547fe115c6ff9825b08fc366be4cd69e9f6"));
    dbg!(&profile);

    loop {
        thread::sleep(std::time::Duration::from_secs(1));

        if let Ok(frame) = bank_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                match message {
                    Message::Api(api::Api::NostrProfileRequest(req)) => {
                        let profile = get_user_profile(nostr_client.clone(), req.pubkey.unwrap());
                        let response = Message::Api(api::Api::NostrProfileResponse( api::NostrProfileResponse {
                            req_id: req.req_id,
                            profile: profile,
                            error: None
                        }));
                        utils::xzmq::send_as_bincode(&bank_tx, &response);
                    }
                    _ => {}
                }
            };
        }

        dbg!("cycle");
    }

    println!("Hello, world!");
}
