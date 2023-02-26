mod nostr_engine;

use crate::nostr_engine::NostrEngine;
use core_types::nostr::NostrProfile;
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;
use lazy_static::lazy_static;
use msgs::Message;
use nostr_sdk::prelude::{Event, FromPkStr, Keys, Kind, SubscriptionFilter, Timestamp};
use nostr_sdk::{Client, Options, RelayPoolNotification};
use regex::Regex;
use serde::{Deserialize, Serialize};
use slog as log;
use slog::Logger;
use std::collections::HashMap;
use std::net::SocketAddr;
use utils::xlogging::LoggingSettings;

const REQUEST_USER_PROFILE_TIMEOUT: u64 = 30_000;
const API_PROFILE_TIMEOUT: u64 = 5_000;

type DbPool = diesel::r2d2::Pool<ConnectionManager<PgConnection>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NostrEngineSettings {
    pub psql_url: String,
    pub nostr_bank_push_address: String,
    pub nostr_bank_pull_address: String,
    pub nostr_private_key: String,
    pub nostr_engine_logging_settings: LoggingSettings,
    pub nostr_relays_urls: Vec<String>,
    pub rebuild_nostr_profile_index: bool,
}

#[derive(Debug)]
pub enum NostrEngineEvent {
    NostrProfileUpdate(Box<NostrProfileUpdate>),
    LndhubxMessage(Message),
}

#[derive(Clone, Debug)]
pub struct NostrProfileUpdate {
    pub pubkey: String,
    pub created_at_epoch_ms: u64,
    pub received_at_epoch_ms: u64,
    pub nostr_profile: NostrProfile,
}

#[derive(Deserialize, Debug)]
pub struct Nip05Response {
    pub names: HashMap<String, String>,
}

/// Returns relays for normal subscription and relays for which index should be rebuilt
pub fn get_relays(settings: &NostrEngineSettings) -> Vec<(String, Option<SocketAddr>)> {
    into_relays(settings.nostr_relays_urls.clone())
}

fn into_relays<T: IntoIterator>(urls: T) -> Vec<(String, Option<SocketAddr>)>
where
    <T as IntoIterator>::Item: Into<String>,
{
    urls.into_iter().map(|url| (url.into(), None)).collect()
}

pub async fn spawn_profile_subscriber(
    nostr_engine_keys: Keys,
    relays: Vec<(String, Option<SocketAddr>)>,
    subscribe_since_epoch_seconds: Option<u64>,
    subscribe_until_epoch_seconds: Option<u64>,
    events_tx: tokio::sync::mpsc::Sender<NostrEngineEvent>,
    logger: Logger,
) -> Client {
    log::info!(
        logger,
        "Waiting to connect with relays: {:?}, since: {:?}, until: {:?}",
        relays,
        subscribe_since_epoch_seconds,
        subscribe_until_epoch_seconds
    );
    let options = Options::new().wait_for_connection(true);
    let nostr_client = Client::new_with_opts(&nostr_engine_keys, options);
    nostr_client.add_relays(relays).await.unwrap();
    nostr_client.connect().await;

    log::info!(logger, "Connected");

    let subscription = {
        let filter = SubscriptionFilter::new();
        let filter = if let Some(since_epoch_seconds) = subscribe_since_epoch_seconds {
            let since_timestamp = Timestamp::from(since_epoch_seconds);
            filter.since(since_timestamp)
        } else {
            filter
        };
        if let Some(until_epoch_seconds) = subscribe_until_epoch_seconds {
            let until_timestamp = Timestamp::from(until_epoch_seconds);
            filter.until(until_timestamp)
        } else {
            filter
        }
    };
    nostr_client.subscribe(vec![subscription]).await;
    let task_nostr_client = nostr_client.clone();
    tokio::spawn(async move {
        loop {
            let mut notifications = task_nostr_client.notifications();
            while let Ok(notification) = notifications.recv().await {
                if let RelayPoolNotification::Event(_url, event) = notification {
                    if event.kind == Kind::Metadata {
                        if let Some(profile_update) = try_profile_update_from_event(&event).await {
                            let msg = NostrEngineEvent::NostrProfileUpdate(Box::new(profile_update));
                            if let Err(err) = events_tx.try_send(msg) {
                                log::error!(
                                    logger,
                                    "Failed to send nostr profile update to events channel, error: {:?}",
                                    err
                                );
                            }
                        }
                    } else {
                        let pubkey = event.pubkey.to_string();
                        request_user_profile(&task_nostr_client, &pubkey).await;
                    }
                }
            }
        }
    });
    nostr_client
}

pub fn spawn_events_handler(
    nostr_client: Client,
    mut events_rx: tokio::sync::mpsc::Receiver<NostrEngineEvent>,
    bank_tx_sender: tokio::sync::mpsc::Sender<Message>,
    db_pool: DbPool,
    logger: Logger,
) {
    tokio::spawn(async move {
        let mut nostr_engine = NostrEngine::new(nostr_client, bank_tx_sender, db_pool, logger).await;
        while let Some(event) = events_rx.recv().await {
            nostr_engine.process_event(&event).await;
        }
    });
}

async fn request_user_profile(client: &Client, pubkey: &str) {
    let subscription = match create_profile_filter(pubkey) {
        Some(filter) => filter,
        None => return,
    };

    let timeout = std::time::Duration::from_millis(REQUEST_USER_PROFILE_TIMEOUT);
    client.req_events_of(vec![subscription], Some(timeout)).await;
}

async fn send_nostr_private_msg(client: &Client, pubkey: &str, text: &str) {
    let keys = Keys::from_pk_str(pubkey).unwrap();
    client.send_direct_msg(keys.public_key(), text).await.unwrap();
}

async fn verify_nip05(pubkey: String, nip05: String) -> Option<bool> {
    if let Some((local_part, domain)) = nip05.split_once('@') {
        if !local_part_valid(local_part) || !domain_valid(domain) {
            return None;
        }
        let url = format!("https://{domain}/.well-known/nostr.json?name={local_part}");
        let body = reqwest::get(&url).await.ok()?.text().await.ok()?;
        let nip_verification = serde_json::from_str::<Nip05Response>(&body).ok()?;
        return nip_verification
            .names
            .get(local_part)
            .map(|response_pubkey| response_pubkey == &pubkey);
    }
    None
}

async fn try_profile_update_from_event(event: &Event) -> Option<NostrProfileUpdate> {
    if event.kind == Kind::Metadata {
        let mut nostr_profile = serde_json::from_str::<NostrProfile>(&event.content).ok()?;
        let pubkey = event.pubkey.to_string();
        let created_at_epoch_ms = 1000 * event.created_at.as_u64();
        let received_at_epoch_ms = utils::time::time_now();
        let verified = if let Some(nip05) = nostr_profile.nip05().as_ref() {
            if !nip05.is_empty() {
                verify_nip05(pubkey.clone(), nip05.clone()).await
            } else {
                None
            }
        } else {
            None
        };
        nostr_profile.set_nip05_verified(verified);
        let profile_update = NostrProfileUpdate {
            pubkey,
            created_at_epoch_ms,
            received_at_epoch_ms,
            nostr_profile,
        };
        return Some(profile_update);
    }
    None
}

fn domain_valid(domain: &str) -> bool {
    // todo more restrictive validation using a good library
    !domain.is_empty() && !domain.contains('@') && domain.len() <= 253
}

fn local_part_valid(local_part: &str) -> bool {
    // as per current nip05 spec only a-z0-9-_. are allowed in local part
    lazy_static! {
        static ref LOCAL_PART_RE: Regex = Regex::new(r"^[a-z0-9-_.]+$").unwrap();
    }
    LOCAL_PART_RE.is_match(local_part)
}

fn create_profile_filter(pubkey: &str) -> Option<SubscriptionFilter> {
    let keys = Keys::from_pk_str(pubkey).ok()?;
    let subscription = SubscriptionFilter::new()
        .author(keys.public_key())
        .kind(Kind::Metadata)
        .limit(1);
    Some(subscription)
}
