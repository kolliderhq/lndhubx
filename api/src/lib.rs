#![feature(drain_filter)]

use actix_cors::Cors;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer};
use diesel::{r2d2::ConnectionManager, PgConnection};
use serde::{Deserialize, Serialize};

use tokio::sync::mpsc;

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
}

pub type WebDbPool = web::Data<DbPool>;
pub type WebSender = web::Data<mpsc::Sender<Envelope>>;

pub async fn start(settings: ApiSettings) -> std::io::Result<()> {
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

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(tx.clone()))
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
            .service(routes::user::get_btc_address)
            .service(routes::pre_signup::pre_signup)
            .service(routes::lnurl::create_lnurl_withdrawal)
            .service(routes::lnurl::get_lnurl_withdrawal)
            .service(routes::lnurl::pay_lnurl_withdrawal)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
