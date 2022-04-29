#![feature(drain_filter)]

use actix_web::web::Data;
use actix_web::{web, App, HttpServer};
use diesel::{r2d2::ConnectionManager, PgConnection};
use serde::{Deserialize, Serialize};

use tokio::sync::mpsc;

use core_types::DbPool;
use utils::xzmq::*;

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
        let conn = pool.get().unwrap();
        models::init(&conn).expect("Failed to initialize models");
    }

    let (tx, rx) = mpsc::channel(1024);

    let subscriber = create_subscriber(&settings.api_zmq_subscribe_address);
    let pusher = create_push(&settings.api_zmq_push_address);

    tokio::task::spawn(CommsActor::start(tx.clone(), rx, subscriber, pusher, settings.clone()));

    HttpServer::new(move || {
        App::new()
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(tx.clone()))
            .service(routes::auth::register)
            .service(routes::auth::login)
            .service(routes::user::get_balances)
            .service(routes::user::create_invoice)
            .service(routes::user::pay)
            .service(routes::user::invoices)
            .service(routes::user::swap)
            .service(routes::user::quote)
            .service(routes::user::transactions)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
