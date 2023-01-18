#![feature(drain_filter)]

use actix_cors::Cors;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer};
use diesel::{r2d2::ConnectionManager, PgConnection};
use serde::{Deserialize, Serialize};
use std::env;
use rust_decimal::prelude::*;

use tokio::sync::mpsc;
use std::sync::Arc;
use tokio::sync::RwLock;


use actix_ratelimit::{MemoryStore, MemoryStoreActor, RateLimiter};
use core_types::DbPool;
use utils::xzmq::SocketContext;

pub mod comms;
pub mod jwt;
pub mod routes;

use comms::*;

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiSettings {
    psql_url: String,
    api_zmq_push_address: String,
    api_zmq_subscribe_address: String,
    quota_replenishment_interval_millis: u64,
    quota_size: u64,
}

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct FtxSpotPrice {
    symbol: String,
    #[serde(alias = "lastPrice")]
    price: Option<Decimal>,
    #[serde(alias = "priceChangePercent")]
    change_24h: Option<Decimal>,
}

pub struct PriceCache {
    pub spot_prices: Vec<FtxSpotPrice>,
    pub last_updated: std::time::Instant,
}

impl Default for PriceCache {
    fn default() -> Self {
        Self {
            spot_prices: Vec::new(),
            last_updated: std::time::Instant::now(),
        }
    }
}

pub type WebDbPool = web::Data<DbPool>;
pub type WebSender = web::Data<mpsc::Sender<Envelope>>;

pub async fn start(settings: ApiSettings) -> std::io::Result<()> {
    let endpoint = env::var("ENDPOINT").unwrap_or("127.0.0.1:8080".to_string());
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
            .service(routes::user_profile::get_user_profile)
    })
    .bind(endpoint)?
    .run()
    .await
}
