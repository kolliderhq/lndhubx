#![feature(drain_filter)]

use actix_cors::Cors;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer};
use diesel::{r2d2::ConnectionManager, PgConnection};
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::{mpsc, Mutex};

use actix_ratelimit::{MemoryStore, MemoryStoreActor, RateLimiter};
use core_types::{DbPool, UserId};
use utils::xzmq::SocketContext;

pub mod comms;
pub mod jwt;
pub mod routes;

use comms::*;
use utils::xlogging::slog::Logger;
use utils::xlogging::LoggingSettings;

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiSettings {
    psql_url: String,
    api_zmq_push_address: String,
    api_zmq_subscribe_address: String,
    quota_replenishment_interval_millis: u64,
    quota_size: u64,
    creation_quota: u64,
    creation_quota_interval_seconds: u64,
    api_logging_settings: LoggingSettings,
    #[serde(default)]
    admin_uids: Option<Vec<UserId>>,
    reserved_usernames: Vec<String>,
    domain: String,
    deezy_api_token: String,
}

impl ApiSettings {
    pub fn logging_settings(&self) -> &LoggingSettings {
        &self.api_logging_settings
    }
}

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct FtxSpotPrice {
    symbol: String,
    #[serde(alias = "lastPrice")]
    price: Option<Decimal>,
    #[serde(alias = "priceChangePercent")]
    change_24h: Option<Decimal>,
}

#[derive(Default)]
pub struct PriceCache {
    pub spot_prices: Vec<FtxSpotPrice>,
    pub last_updated: Option<std::time::Instant>,
}

#[derive(Clone)]
pub struct CreationLimiter {
    creation_enabled: bool,
    creation_quota: u64,
    creation_quota_interval_seconds: u64,
    created: u64,
    last_interval_start: std::time::Instant,
}

impl CreationLimiter {
    pub fn new(creation_quota: u64, creation_quota_interval_seconds: u64) -> Self {
        Self {
            creation_enabled: true,
            creation_quota,
            creation_quota_interval_seconds,
            created: 0,
            last_interval_start: std::time::Instant::now(),
        }
    }

    pub fn is_creation_enabled(&self) -> bool {
        self.creation_enabled
    }

    pub fn enable_creation(&mut self) {
        self.creation_enabled = true;
    }

    pub fn disable_creation(&mut self) {
        self.creation_enabled = false;
    }

    /// Returns number of successful create calls in the interval on Ok
    /// or seconds left to replenish quota on Err
    pub fn increase(&mut self) -> Result<u64, u64> {
        let elapsed_seconds = self.last_interval_start.elapsed().as_secs();
        if elapsed_seconds >= self.creation_quota_interval_seconds {
            self.last_interval_start = std::time::Instant::now();
            self.created = 0;
        }
        if self.created < self.creation_quota {
            self.created += 1;
            Ok(self.created)
        } else {
            let replenish_seconds_left = self.creation_quota_interval_seconds - elapsed_seconds;
            Err(replenish_seconds_left)
        }
    }

    pub fn decrease(&mut self) {
        if self.created > 0 {
            self.created -= 1;
        }
    }

    pub fn is_remaining_quota(&self) -> bool {
        let elapsed_seconds = self.last_interval_start.elapsed().as_secs();
        elapsed_seconds >= self.creation_quota_interval_seconds || self.created < self.creation_quota
    }
}

pub type WebDbPool = web::Data<DbPool>;
pub type WebSender = web::Data<mpsc::Sender<Envelope>>;

pub async fn start(settings: ApiSettings, logger: Logger) -> std::io::Result<()> {
    let endpoint = env::var("ENDPOINT").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let pool = r2d2::Pool::builder()
        .build(ConnectionManager::<PgConnection>::new(settings.psql_url.clone()))
        .expect("Failed to create pool.");

    {
        let conn = pool.get().expect("Failed to get DB connection to initialize models");
        models::init(&conn).expect("Failed to initialize models");
    }

    let (tx, rx) = mpsc::channel(1024);

    let context = SocketContext::new();
    let subscriber = context.create_subscriber(&settings.api_zmq_subscribe_address);
    let pusher = context.create_push(&settings.api_zmq_push_address);

    tokio::task::spawn(CommsActor::start(tx.clone(), rx, subscriber, pusher, settings.clone()));

    let ratelimiter_store = MemoryStore::new();

    let replenishment_interval = settings.quota_replenishment_interval_millis;
    let max_requests = settings.quota_size as usize;

    let price_cache = Arc::new(RwLock::new(PriceCache::default()));

    let creation_limiter = Arc::new(Mutex::new(CreationLimiter::new(
        settings.creation_quota,
        settings.creation_quota_interval_seconds,
    )));

    let admin_uids = settings
        .admin_uids
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<UserId>>();

    let reserved_usernames = settings
        .reserved_usernames
        .iter()
        .map(|username| username.to_lowercase())
        .collect::<HashSet<String>>();

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .wrap(
                RateLimiter::new(MemoryStoreActor::from(ratelimiter_store.clone()).start())
                    .with_interval(std::time::Duration::from_millis(replenishment_interval))
                    .with_max_requests(max_requests)
                    .with_identifier(|req| {
                        let c = req.connection_info().clone();
                        let ip_parts: Vec<&str> = c.realip_remote_addr().unwrap().split(':').collect();
                        Ok(ip_parts[0].to_string()) // should now be "127.0.0.1"
                    }),
            )
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(tx.clone()))
            .app_data(Data::new(price_cache.clone()))
            .app_data(Data::new(logger.clone()))
            .app_data(Data::new(creation_limiter.clone()))
            .app_data(Data::new(admin_uids.clone()))
            .app_data(Data::new(reserved_usernames.clone()))
            .app_data(Data::new(settings.clone()))
            .service(routes::auth::create)
            .service(routes::auth::auth)
            .service(routes::auth::whoami)
            .service(routes::user::balance)
            .service(routes::user::add_invoice)
            .service(routes::user::pay_invoice)
            .service(routes::user::get_user_invoices)
            .service(routes::user::swap)
            .service(routes::user::quote)
            .service(routes::user::get_txs)
            .service(routes::user::get_available_currencies)
            .service(routes::user::get_node_info)
            .service(routes::user::get_query_route)
            .service(routes::user::check_username_available)
            .service(routes::user::search_ln_addresses)
            .service(routes::user::check_payment)
            .service(routes::user::get_onchain_address)
            .service(routes::user::get_btc_ln_swap_state)
            .service(routes::user::make_onchain_swap)
            .service(routes::lnurl::create_lnurl_withdrawal)
            .service(routes::lnurl::get_lnurl_withdrawal)
            .service(routes::lnurl::pay_lnurl_withdrawal)
            .service(routes::lnurl::lnurl_pay_address)
            .service(routes::lnurl::pay_address)
            .service(routes::external::get_spot_prices)
            .service(routes::nostr::set_nostr_pubkey)
            .service(routes::nostr::update_nostr_pubkey)
            .service(routes::nostr::nostr_nip05)
            .service(routes::nostr::get_nostr_profile)
            .service(routes::nostr::search_nostr_profile)
            .service(routes::user_profile::get_user_profile)
            .service(routes::user_profile::user_profile)
            .service(routes::admin::disable_create)
            .service(routes::admin::enable_create)
            .service(routes::user::get_dca_settings)
            .service(routes::user::delete_dca_settings)
            .service(routes::user::set_dca_settings)
    })
    .bind(endpoint)?
    .run()
    .await
}
