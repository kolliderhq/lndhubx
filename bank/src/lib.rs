extern crate core;

pub mod accountant;
pub mod bank_engine;
pub mod ledger;
pub mod dca;

use bank_engine::*;
use futures::prelude::*;
use std::time::Instant;

use dca::dca_task;

use diesel::{r2d2::ConnectionManager, PgConnection};
use zmq::Socket as ZmqSocket;

use core_types::*;
use crossbeam_channel::bounded;
use msgs::*;
use msgs::journal::{Transaction, Journal};

use lnd_connector::connector::*;
use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use futures::stream::FuturesUnordered;
use influxdb2::Client;

use accountant::*;

pub async fn insert_bank_state(bank: &BankEngine, client: &Client, bucket: &str) {
    let mut btc_balance = dec!(0);
    let mut usd_balance = dec!(0);
    let mut eur_balance = dec!(0);

    for (_, user_account) in bank.ledger.user_accounts.clone().into_iter() {
        for (_, account) in user_account.accounts.into_iter() {
            if account.currency == Currency::BTC {
                btc_balance += account.balance;
            } else if account.currency == Currency::EUR {
                eur_balance += account.balance;
            } else if account.currency == Currency::USD {
                usd_balance += account.balance;
            }
        }
    }

    let fields = vec![
        ("btc_user_balance", btc_balance),
        ("eur_user_balance", eur_balance),
        ("usd_user_balance", usd_balance),
        // ("fee_balance", bank.ledger.fee_account.balance),
        ("fee_balance", dec!(0)),
        ("insurance_fund_balance", bank.ledger.insurance_fund_account.balance),
        ("ln_network_max_fee", bank.ln_network_max_fee),
        ("ln_network_fee_margin", bank.ln_network_fee_margin),
        ("internal_tx_fee", bank.internal_tx_fee),
        ("external_tx_fee", bank.external_tx_fee),
        ("external_tx_fee", bank.external_tx_fee),
    ];

    let builder = fields.into_iter().fold(
        influxdb2::models::DataPoint::builder("bank_states"),
        |builder, (field_name, value)| match value.to_f64() {
            Some(converted) => builder.field(field_name, converted),
            None => builder,
        },
    );

    if let Ok(data_point) = builder.build() {
        let points = vec![data_point];
        if let Err(err) = client.write(bucket, stream::iter(points)).await {
            eprintln!("Failed to write point to Influx. Err: {err}");
        }
    }
}

pub async fn insert_transaction(client: &Client, tx: Transaction, bucket: &str) -> Result<(), ()> {
    let fields = vec![
        ("outbound_amount", tx.outbound_amount),
		("inbound_amount", tx.inbound_amount),
        ("fees", tx.fees),
        ("exchange_rate", tx.exchange_rate),
    ];

    let tags = vec![
        ("outbound_currency", tx.outbound_currency),
        ("inbound_currency", tx.inbound_currency),
        ("outbound_uid", tx.outbound_uid.to_string()),
        ("inbound_uid", tx.inbound_uid.to_string()),
        ("tx_type", tx.tx_type),
    ];

    let mut builder = fields.into_iter().fold(
        influxdb2::models::DataPoint::builder("summary_transactions"),
        |builder, (field_name, value)| match value.to_f64() {
            Some(converted) => builder.field(field_name, converted),
            None => builder,
        },
    );

    tags.into_iter().fold(
        influxdb2::models::DataPoint::builder("summary_transactions"),
        |builder, (tag_name, value)| {
            builder.tag(tag_name, value)
        },
    );

    if let Ok(data_point) = builder.build() {
        let points = vec![data_point];
        if let Err(err) = client.write(bucket, stream::iter(points)).await {
            eprintln!("Failed to write point to Influx. Err: {err}");
        }
    }
    Ok(())
}

pub async fn start(
    settings: BankEngineSettings,
    lnd_connector_settings: LndConnectorSettings,
    api_recv: ZmqSocket,
    api_sender: ZmqSocket,
    dealer_sender: ZmqSocket,
    dealer_recv: ZmqSocket,
    nostr_sender: ZmqSocket,
    nostr_recv: ZmqSocket,
    cli_socket: ZmqSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = r2d2::Pool::builder()
        .build(ConnectionManager::<PgConnection>::new(settings.psql_url.clone()))
        .expect("Failed to create pool.");

    let lnd_connector = LndConnector::new(lnd_connector_settings.clone()).await;
    let mut lnd_connector_invoices = LndConnector::new(lnd_connector_settings.clone()).await;

    let influx_client = Client::new(
        settings.influx_host.clone(),
        settings.influx_org.clone(),
        settings.influx_token.clone(),
    );

    let (invoice_tx, invoice_rx) = bounded(1024);
    let (dca_tx, dca_rx) = bounded(1024);
    let (priority_tx, priority_rx) = bounded(1024);
    let (journal_tx, journal_rx) = bounded(1024);

    let invoice_task = {
        async move {
            lnd_connector_invoices.sub_invoices(invoice_tx).await;
        }
    };

    tokio::spawn(invoice_task);

    let dca_task = {
        async move {
            dca_task(dca_tx).await;
        }
    };

    tokio::spawn(dca_task);

    let (payment_thread_tx, payment_thread_rx) = crossbeam_channel::bounded(2024);

    let mut bank_engine = BankEngine::new(
        Some(pool),
        lnd_connector,
        settings.clone(),
        lnd_connector_settings,
        payment_thread_tx,
    )
    .await;
    bank_engine.init_accounts();

    if settings.normalize_account_balances {
        slog::warn!(&bank_engine.logger, "Performing balances normalization");
        bank_engine.normalize_accounts();
        bank_engine.store_accounts();
    }

    if let Err(error) = reconcile_ledger(&bank_engine.ledger) {
        slog::warn!(&bank_engine.logger, "Reconciliation error at start-up: {:?}", error);
        // Giving logger to send out log.
        std::thread::sleep(std::time::Duration::from_millis(500));
        panic!("Reconciliation error at start up! Shutting down.");
    }
    slog::info!(&bank_engine.logger, "Accounts start-up reconciliation successful");

    let mut state_insertion_interval = Instant::now();
    let mut reconciliation_interval = Instant::now();

    insert_bank_state(&bank_engine, &influx_client, &settings.influx_bucket.clone()).await;

    let mut listener = |msg: Message, destination: ServiceIdentity| match destination {
        ServiceIdentity::Api => {
            utils::xzmq::send_multipart_as_bincode(&api_sender, &msg);
        }
        ServiceIdentity::Dealer => {
            utils::xzmq::send_as_bincode(&dealer_sender, &msg);
        }
        ServiceIdentity::Loopback => {
            if let Err(err) = priority_tx.send(msg) {
                panic!("Failed to send priority message: {err:?}");
            }
        }
        ServiceIdentity::Journal => {
            if let Err(err) = journal_tx.send(msg) {
                panic!("Failed to send priority message: {err:?}");
            }
        }
        ServiceIdentity::Nostr => {
            utils::xzmq::send_as_bincode(&nostr_sender, &msg);
        }
        _ => {}
    };

    let mut cli_listener = |msg: Message, _destination: ServiceIdentity| match msg {
        Message::Nostr(nostr_msg) => {
            utils::xzmq::send_as_bincode(&nostr_sender, &Message::Nostr(nostr_msg.clone()));
            utils::xzmq::send_as_json(&cli_socket, &Message::Nostr(nostr_msg))
        }
        _ => utils::xzmq::send_as_json(&cli_socket, &msg),
    };

    loop {
        if let Ok(msg) = payment_thread_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }
        // Receiving msgs from the api.
        if let Ok(frame) = api_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut listener).await;
            };
        }

        // Receiving msgs from the invoice subscribtion.
        if let Ok(msg) = invoice_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        // Receiving msgs from dca rask.
        if let Ok(msg) = dca_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        // Receiving msgs from dealer.
        if let Ok(frame) = dealer_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut listener).await;
            };
        }

        // Receiving msgs from nostr engine.
        if let Ok(frame) = nostr_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut listener).await;
            };
        }

        if let Ok(msg) = priority_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        if let Ok(msg) = journal_rx.try_recv() {
            match msg {
                Message::Journal(Journal::Transaction(msg))=> {
                    insert_transaction(&influx_client, msg, &settings.influx_bucket.clone()).await;
                }
                _ => {}
            }
        }

        if let Ok(frame) = cli_socket.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut cli_listener).await;
            };
        }

        if state_insertion_interval.elapsed().as_secs() > 5 {
            insert_bank_state(&bank_engine, &influx_client, &settings.influx_bucket.clone()).await;

            state_insertion_interval = Instant::now();
            // Cleaning up the payment threads.
            bank_engine.payment_threads = bank_engine
                .payment_threads
                .into_iter()
                .filter(|t| t.is_finished())
                .collect::<FuturesUnordered<tokio::task::JoinHandle<()>>>();
        }

        if reconciliation_interval.elapsed().as_secs() > 3 {
            reconciliation_interval = Instant::now();
            if let Err(error) = reconcile_ledger(&bank_engine.ledger) {
                slog::warn!(&bank_engine.logger, "{:?}", error);
                // Giving logger to send out log.
                std::thread::sleep(std::time::Duration::from_millis(500));
                panic!("Reconciliation error! Shutting down.");
            }
        }
    }
}
