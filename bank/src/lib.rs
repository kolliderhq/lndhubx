pub mod bank_engine;

use bank_engine::*;
use futures::prelude::*;
use std::time::Instant;

use diesel::{r2d2::ConnectionManager, PgConnection};
use zmq::Socket as ZmqSocket;

use core_types::*;
use crossbeam_channel::bounded;
use msgs::*;

use lnd_connector::connector::*;
use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use influxdb2::Client;
use futures::stream::FuturesUnordered;

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

    let points = vec![influxdb2::models::DataPoint::builder("bank_states")
        // .tag("host", "server01")
        .field("btc_user_balance", btc_balance.to_f64().unwrap())
        .field("eur_user_balance", eur_balance.to_f64().unwrap())
        .field("usd_user_balance", usd_balance.to_f64().unwrap())
        .field("fee_balance", bank.ledger.fee_account.balance.to_f64().unwrap())
        .field(
            "insurance_fund_balance",
            bank.ledger.insurance_fund_account.balance.to_f64().unwrap(),
        )
        .field("ln_network_max_fee", bank.ln_network_max_fee.to_f64().unwrap())
        .field("ln_network_fee_margin", bank.ln_network_fee_margin.to_f64().unwrap())
        .field("internal_tx_fee", bank.internal_tx_fee.to_f64().unwrap())
        .field("external_tx_fee", bank.external_tx_fee.to_f64().unwrap())
        .field("external_tx_fee", bank.external_tx_fee.to_f64().unwrap())
        .build()
        .unwrap()];

    client.write(bucket, stream::iter(points)).await.unwrap();
}

pub async fn start(
    settings: BankEngineSettings,
    lnd_connector_settings: LndConnectorSettings,
    api_recv: ZmqSocket,
    api_sender: ZmqSocket,
    dealer_sender: ZmqSocket,
    dealer_recv: ZmqSocket,
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
    let (priority_tx, priority_rx) = bounded(1024);

    let invoice_task = {
        async move {
            lnd_connector_invoices.sub_invoices(invoice_tx).await;
        }
    };

    tokio::spawn(invoice_task);

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

    let mut state_insertion_interval = Instant::now();

    insert_bank_state(&bank_engine, &influx_client, &settings.influx_bucket.clone()).await;

    let mut listener = |msg: Message, destination: ServiceIdentity| match destination {
        ServiceIdentity::Api => {
            let payload = bincode::serialize(&msg).unwrap();
            api_sender.send_multipart(vec![vec![], vec![], payload], 0x00).unwrap();
        }
        ServiceIdentity::Dealer => {
            let payload = bincode::serialize(&msg).unwrap();
            dealer_sender.send(payload, 0x00).unwrap();
        }
        ServiceIdentity::Loopback => {
            priority_tx.send(msg).unwrap();
        }
        _ => {}
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

        // Receiving msgs from dealer.
        if let Ok(frame) = dealer_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut listener).await;
            };
        }

        if let Ok(msg) = priority_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
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
    }
}
