pub mod dealer_engine;

use crossbeam::channel::bounded;
use dealer_engine::*;
use msgs::dealer::{BankStateRequest, Dealer};
use msgs::*;
use std::time::Instant;
use uuid::Uuid;

use kollider_hedging::KolliderHedgingClient;

use core_types::*;
use futures::prelude::*;
use influxdb2::Client;
use rust_decimal::prelude::*;
use utils::kafka::{Consumer, Producer};

pub async fn insert_dealer_state(dealer: &DealerEngine, client: &Client, bucket: &str) {
    let usd_hedged_qty = dealer.get_hedged_quantity(Symbol::from("BTCUSD.PERP"));
    let eur_hedged_qty = dealer.get_hedged_quantity(Symbol::from("BTCEUR.PERP"));

    let fields = vec![
        ("usd_hedged_quantity", usd_hedged_qty),
        ("eur_hedged_quantity", eur_hedged_qty),
    ];

    let builder = fields.into_iter().fold(
        influxdb2::models::DataPoint::builder("dealer_states"),
        |builder, (field_name, value)| {
            if let Ok(defined) = value {
                match defined.to_f64() {
                    Some(converted) => builder.field(field_name, converted),
                    None => builder,
                }
            } else {
                builder
            }
        },
    );

    if let Ok(data_point) = builder.build() {
        let points = vec![data_point];
        if let Err(err) = client.write(bucket, stream::iter(points)).await {
            eprintln!("Failed to write point to Influx. Err: {:?}", err);
        }
    }
}

pub async fn start(settings: DealerEngineSettings, kafka_producer: Producer, mut kafka_consumer: Consumer) {
    let (kollider_client_tx, kollider_client_rx) = bounded(2024);
    let (kafka_tx, kafka_rx) = bounded(2024);

    let ws_client = match KolliderHedgingClient::connect(
        &settings.kollider_ws_url,
        &settings.kollider_api_key,
        &settings.kollider_api_secret,
        &settings.kollider_api_passphrase,
        kollider_client_tx,
    ) {
        Ok(connected) => connected,
        Err(err) => {
            eprintln!(
                "Failed to connect to: {}, reason: {:?}. Exiting",
                settings.kollider_ws_url, err
            );
            return;
        }
    };

    let mut synth_dealer = DealerEngine::new(settings.clone(), ws_client);

    let influx_client = Client::new(
        settings.influx_host.clone(),
        settings.influx_org.clone(),
        settings.influx_token.clone(),
    );

    let mut listener = |msg: Message| {
        kafka_producer.produce("bank", &msg);
    };

    let mut last_health_check = Instant::now();
    let mut last_house_keeping = Instant::now();
    let mut last_risk_check = Instant::now();

    std::thread::spawn(move || {
        while let Some(maybe_message) = kafka_consumer.consume() {
            if let Some(message) = maybe_message {
                if let Err(err) = kafka_tx.send(message) {
                    panic!("Kafka consumer failed to post a message into channel: {:?}", err);
                }
            }
        }
    });

    loop {
        // Before we proceed we have to have received a bank state message
        if !synth_dealer.has_bank_state() && synth_dealer.is_ready() {
            let msg = Message::Dealer(Dealer::BankStateRequest(BankStateRequest {
                req_id: Uuid::new_v4(),
                requesting_identity: ServiceIdentity::Dealer,
            }));
            listener(msg);
            while let Ok(message) = kafka_rx.recv() {
                if let Message::Dealer(Dealer::BankState(ref _bank_state)) = message {
                    synth_dealer.process_msg(message, &mut listener);
                    last_risk_check = Instant::now();
                    break;
                }
            }
        }

        if let Ok(message) = kafka_rx.try_recv() {
            synth_dealer.process_msg(message, &mut listener);
        }

        if let Ok(message) = kollider_client_rx.try_recv() {
            synth_dealer.process_msg(message, &mut listener);
        }

        if last_risk_check.elapsed().as_secs() > 10 {
            if synth_dealer.has_bank_state() {
                synth_dealer.check_risk(&mut listener);
                last_risk_check = Instant::now();
            }
            insert_dealer_state(&synth_dealer, &influx_client, &settings.influx_bucket.clone()).await;
        }

        if last_health_check.elapsed().as_secs() > 5 {
            synth_dealer.check_health(&mut listener);
            last_health_check = Instant::now();
        }

        if last_house_keeping.elapsed().as_secs() > 30 {
            last_house_keeping = Instant::now();
            synth_dealer.sweep_excess_funds(&mut listener);
        }
    }
}
