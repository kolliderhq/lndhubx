use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use msgs::*;
use nostr_engine::{get_relays, spawn_events_handler, spawn_profile_subscriber, NostrEngineEvent, NostrEngineSettings};
use nostr_sdk::prelude::{FromSkStr, Keys};
use slog as log;
use utils::xzmq::SocketContext;

#[tokio::main]
async fn main() {
    let settings = utils::config::get_config_from_env::<NostrEngineSettings>().expect("Failed to load settings.");
    let logger = utils::xlogging::init_log(&settings.nostr_engine_logging_settings);

    let context = SocketContext::new();
    let bank_recv = context.create_pull(&settings.nostr_bank_pull_address);
    let bank_tx = context.create_push(&settings.nostr_bank_push_address);

    let nostr_engine_keys = Keys::from_sk_str(&settings.nostr_private_key).unwrap();

    let relays = get_relays(&settings);

    let (events_tx, events_rx) = tokio::sync::mpsc::channel(2048);

    let db_pool = Pool::builder()
        .build(ConnectionManager::<PgConnection>::new(settings.psql_url))
        .expect("Failed to create pool.");

    let now_seconds = utils::time::time_now() / 1000;

    // for rebuilding index
    spawn_profile_subscriber(
        nostr_engine_keys.clone(),
        relays.subscribed_from_beginning,
        None,
        None,
        events_tx.clone(),
        logger.clone(),
    );

    // for ongoing subscription
    spawn_profile_subscriber(
        nostr_engine_keys.clone(),
        relays.subscribed_from_now,
        Some(now_seconds),
        None,
        events_tx.clone(),
        logger.clone(),
    );

    spawn_events_handler(
        nostr_engine_keys,
        relays.all,
        events_rx,
        bank_tx,
        db_pool,
        logger.clone(),
    );

    while let Ok(frame) = bank_recv.recv_msg(0) {
        if let Ok(message) = bincode::deserialize::<Message>(&frame) {
            if let Err(err) = events_tx.send(NostrEngineEvent::LndhubxMessage(message)).await {
                log::error!(
                    logger,
                    "Failed to send lndhubx message to events channel, error: {:?}",
                    err
                );
            }
        };
    }
}
