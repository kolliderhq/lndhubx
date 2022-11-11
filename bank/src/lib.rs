extern crate core;

pub mod accountant;
pub mod bank_engine;
pub mod ledger;

use bank_engine::*;
use futures::prelude::*;
use std::time::Instant;

use diesel::{r2d2::ConnectionManager, PgConnection};

use core_types::*;
use crossbeam_channel::bounded;
use msgs::*;

use lnd_connector::connector::*;
use rust_decimal::prelude::*;
use rust_decimal_macros::*;

use futures::stream::FuturesUnordered;
use influxdb2::Client;

use accountant::*;
use utils::kafka::{Consumer, Producer};

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
        ("fee_balance", bank.ledger.fee_account.balance),
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
            eprintln!("Failed to write point to Influx. Err: {:?}", err);
        }
    }
}

pub async fn start(
    settings: BankEngineSettings,
    lnd_connector_settings: LndConnectorSettings,
    mut kafka_consumer: Consumer,
    kafka_producer: Producer,
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
    let (loopback_tx, loopback_rx) = bounded(1024);

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
    let mut reconciliation_interval = Instant::now();

    insert_bank_state(&bank_engine, &influx_client, &settings.influx_bucket.clone()).await;

    let kafka_loopback = loopback_tx.clone();
    std::thread::spawn(move || {
        while let Some(maybe_message) = kafka_consumer.consume() {
            if let Some(message) = maybe_message {
                if let Err(err) = kafka_loopback.send(message) {
                    panic!("Kafka consumer failed to send a loopback message: {:?}", err);
                }
            }
        }
    });

    let mut listener = |msg: Message, destination: ServiceIdentity| {
        let topic = match destination {
            ServiceIdentity::Api => "api",
            ServiceIdentity::Dealer => "dealer",
            ServiceIdentity::LndConnector => "lnd_connector",
            ServiceIdentity::BankEngine => "bank",
            ServiceIdentity::Cli => "cli",
            ServiceIdentity::Loopback => {
                if let Err(err) = loopback_tx.send(msg) {
                    panic!("Listener failed to send a loopback message: {:?}", err);
                }
                return;
            }
        };
        kafka_producer.produce(topic, &msg);
    };

    loop {
        if let Ok(msg) = payment_thread_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        // Receiving msgs from the invoice subscribtion.
        if let Ok(msg) = invoice_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        if let Ok(msg) = loopback_rx.try_recv() {
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
