mod nostr_engine;

use crate::nostr_engine::NostrEngine;
use core_types::nostr::NostrProfile;
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;
use lazy_static::lazy_static;
use msgs::api::ShareableNostrProfile;
use msgs::Message;
use nostr_sdk::prelude::{Event, FromPkStr, Keys, Kind, SubscriptionFilter, Timestamp};
use nostr_sdk::{Client, RelayPoolNotification};
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
    pub nostr_historical_profile_indexer: bool,
}

#[derive(Debug)]
pub enum NostrEngineEvent {
    NostrProfileUpdate(Box<NostrProfileUpdate>),
    LndhubxMessage(Message),
    InternalNostrProfileRequest(InternalNostrProfileRequest),
}

#[derive(Clone, Debug)]
pub struct NostrProfileUpdate {
    pub pubkey: String,
    pub content: String,
    pub created_at_epoch_ms: u64,
    pub received_at_epoch_ms: u64,
    pub nostr_profile: NostrProfile,
    pub validated_lnurl_pay_req: Option<String>,
}

impl From<&NostrProfileUpdate> for ShareableNostrProfile {
    fn from(profile_update: &NostrProfileUpdate) -> Self {
        Self {
            pubkey: profile_update.pubkey.clone(),
            created_at: profile_update.created_at_epoch_ms as i64 / 1000,
            profile: profile_update.nostr_profile.clone(),
            validated_lnurl_pay_req: profile_update.validated_lnurl_pay_req.clone(),
        }
    }
}

#[derive(Debug)]
pub struct InternalNostrProfileRequest {
    pub pubkey: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Deserialize, Debug)]
struct Nip05Response {
    pub names: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct LnUrlPayResponse {
    callback: String,
    #[serde(rename = "maxSendable")]
    max_sendable: u64,
    #[serde(rename = "minSendable")]
    min_sendable: u64,
    metadata: String,
    tag: String,
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

pub fn spawn_profile_indexer(
    nostr_client: Client,
    indexer_start: Option<u64>,
    events_tx: tokio::sync::mpsc::Sender<NostrEngineEvent>,
    db_pool: DbPool,
    logger: Logger,
) {
    spawn_profile_subscriber(nostr_client.clone(), events_tx.clone(), logger.clone());
    tokio::spawn(async move {
        if let Some(since_epoch_seconds) = indexer_start {
            let mut range_since = since_epoch_seconds;
            loop {
                let time_now = utils::time::time_now() / 1000;
                if range_since >= time_now {
                    log::info!(logger, "Indexer progress [100%]");
                    break;
                }
                let range_until = range_since + utils::time::SECONDS_IN_HOUR;
                let range_until_capped = range_until.min(time_now);
                let internal_request = InternalNostrProfileRequest {
                    pubkey: None,
                    since: Some(range_since),
                    until: Some(range_until_capped),
                    limit: None,
                };
                if let Err(err) = events_tx.try_send(NostrEngineEvent::InternalNostrProfileRequest(internal_request)) {
                    log::error!(
                        logger,
                        "Indexer failed to send internal profile request since: {}, until: {}, error: {:?}",
                        range_since,
                        range_until_capped,
                        err
                    );
                } else {
                    let full_range = time_now - since_epoch_seconds;
                    let progress = range_since - since_epoch_seconds;
                    let progress_pct = 100 * progress / full_range;
                    log::info!(
                        logger,
                        "Indexer progress [{}%]: since: {}, until: {}",
                        progress_pct,
                        range_since,
                        range_until_capped
                    );
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                store_last_check(&db_pool, range_since, &logger);
                range_since = range_until;
            }
        }
        // this loop moves ongoing subscription timeout every 24h
        // to avoid subscribing from the same time point when any relay disconnects
        loop {
            let since_epoch_seconds = utils::time::time_now() / 1000;
            let since = Some(since_epoch_seconds);
            let internal_request = InternalNostrProfileRequest {
                pubkey: None,
                since,
                until: None,
                limit: None,
            };
            let subscription_filter = match create_profile_filter(&internal_request) {
                Some(filter) => filter,
                None => return,
            };
            nostr_client.subscribe(vec![subscription_filter]).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(utils::time::SECONDS_IN_HOUR)).await;
            store_last_check(&db_pool, since_epoch_seconds, &logger);
        }
    });
}

fn spawn_profile_subscriber(
    nostr_client: Client,
    events_tx: tokio::sync::mpsc::Sender<NostrEngineEvent>,
    logger: Logger,
) {
    tokio::spawn(async move {
        loop {
            let mut notifications = nostr_client.notifications();
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
                    }
                }
            }
        }
    });
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
        nostr_engine.initialize_profile_cache().await;
        while let Some(event) = events_rx.recv().await {
            nostr_engine.process_event(&event).await;
        }
    });
}

async fn request_user_profile(client: &Client, request: &InternalNostrProfileRequest) {
    let subscription = match create_profile_filter(request) {
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
        if !nip05_local_part_valid(local_part) || !domain_valid(domain) {
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
        let content = event.content.clone();
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
        let lnurl_pay_req = get_caps_lnurl_pay_request(&nostr_profile).await;
        let profile_update = NostrProfileUpdate {
            pubkey,
            content,
            created_at_epoch_ms,
            received_at_epoch_ms,
            nostr_profile,
            validated_lnurl_pay_req: lnurl_pay_req,
        };
        return Some(profile_update);
    }
    None
}

fn domain_valid(domain: &str) -> bool {
    // todo more restrictive validation using a good library
    !domain.is_empty() && !domain.contains('@') && domain.len() <= 253
}

fn nip05_local_part_valid(local_part: &str) -> bool {
    // as per current nip05 spec only a-z0-9-_. are allowed in local part
    // to loosen restriction we also allow capitial letters A-Z
    lazy_static! {
        static ref NIP05_LOCAL_PART_RE: Regex = Regex::new(r"^[a-zA-Z0-9-_.]+$").unwrap();
    }
    NIP05_LOCAL_PART_RE.is_match(local_part)
}

fn lud16_local_part_valid(local_part: &str) -> bool {
    // as per current lud16 spec only a-z0-9-_. are allowed in local part
    lazy_static! {
        static ref LUD16_LOCAL_PART_RE: Regex = Regex::new(r"^[a-z0-9-_.]+$").unwrap();
    }
    LUD16_LOCAL_PART_RE.is_match(local_part)
}

fn create_profile_filter(request: &InternalNostrProfileRequest) -> Option<SubscriptionFilter> {
    let filter = SubscriptionFilter::new().kind(Kind::Metadata);

    let filter = match request.pubkey {
        Some(ref pubkey) => {
            let keys = Keys::from_pk_str(pubkey).ok()?;
            filter.author(keys.public_key())
        }
        None => filter,
    };

    let filter = match request.since {
        Some(since) => filter.since(Timestamp::from(since)),
        None => filter,
    };

    let filter = match request.until {
        Some(until) => filter.until(Timestamp::from(until)),
        None => filter,
    };

    let filter = match request.limit {
        Some(limit) => filter.limit(limit),
        None => filter,
    };

    Some(filter)
}

fn store_last_check(db_pool: &DbPool, last_check: u64, logger: &Logger) {
    if let Some(conn) = db_pool.try_get() {
        if let Err(err) =
            models::nostr_profile_indexer_times::NostrProfileIndexerTime::set_last_check(&conn, Some(last_check as i64))
        {
            log::error!(
                logger,
                "Indexer failed to store last_check time: {}, error: {:?}",
                last_check,
                err
            );
        }
    } else {
        log::error!(
            logger,
            "Indexer failed to get a DB connection to store last_check time: {}",
            last_check,
        );
    }
}

async fn get_lnurl_pay_request(profile: &NostrProfile) -> Option<String> {
    if let Some(lud16_value) = profile.lud16() {
        if let Some(lnurl) = lnurl_from_address(lud16_value) {
            if verify_lnurl(&lnurl).await {
                return utils::lnurl::encode(&lnurl, None).ok();
            }
        } else if let Ok(lnurl) = utils::lnurl::decode(lud16_value) {
            if verify_lnurl(&lnurl).await {
                // some people put lnurl pay request into lud16 field
                // so we check this and return it if it is valid
                return Some(lud16_value.clone());
            }
        }
    }

    if let Some(lud06_value) = profile.lud06() {
        if let Ok(lnurl) = utils::lnurl::decode(lud06_value) {
            if verify_lnurl(&lnurl).await {
                // some people put lnurl pay request into lud16 field
                // so we check this and return it if it is valid
                return Some(lud06_value.clone());
            }
        }
    }
    None
}

async fn get_caps_lnurl_pay_request(profile: &NostrProfile) -> Option<String> {
    get_lnurl_pay_request(profile)
        .await
        .map(|pay_request| pay_request.to_uppercase())
}

fn lnurl_from_address(lightning_address: &str) -> Option<String> {
    if let Some((local_part, domain)) = lightning_address.split_once('@') {
        if lud16_local_part_valid(local_part) && domain_valid(domain) {
            let link = format!("https://{domain}/.well-known/lnurlp/{local_part}");
            return Some(link);
        }
    }
    None
}

async fn verify_lnurl(url: &str) -> bool {
    if let Ok(response) = reqwest::get(url).await {
        if let Ok(body) = response.text().await {
            let lnurl_verification = match serde_json::from_str::<LnUrlPayResponse>(&body) {
                Ok(parsed) => parsed,
                Err(_) => return false,
            };

            if utils::Url::parse(&lnurl_verification.callback).is_err() {
                return false;
            }

            if lnurl_verification.min_sendable < 1 || lnurl_verification.max_sendable < lnurl_verification.min_sendable
            {
                return false;
            }

            if lnurl_verification.tag != "payRequest" {
                return false;
            }

            // more detailed check on metadata could be performed,
            // but that should be enough for our purposes
            return serde_json::from_str::<Vec<Vec<String>>>(&lnurl_verification.metadata).is_ok();
        }
    }
    false
}
