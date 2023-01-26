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
use rust_decimal_macros::dec;
use utils::xzmq::ZmqSocket;

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
            eprintln!("Failed to write point to Influx. Err: {}", err);
        }
    }
}

pub async fn store_quotes(dealer: &DealerEngine, client: &Client, bucket: &str) {
    let some_sats = Money::new(Currency::BTC, Some(dec!(0.000005000)));
    let (btc_usd_rate, _btc_usd_fee) = dealer.get_rate(some_sats.clone(), Currency::USD);

    let (btc_eur_rate, _btc_eur_fee) = dealer.get_rate(some_sats, Currency::EUR);

    let one_usd = Money::new(Currency::USD, Some(dec!(1.0)));
    let (usd_btc_rate, _usd_btc_fee) = dealer.get_rate(one_usd, Currency::BTC);
    let (usd_eur_rate, _usd_eur_fee) = dealer.get_rate(one_usd, Currency::EUR);

    let one_eur = Money::new(Currency::EUR, Some(dec!(1.0)));
    let (eur_btc_rate, _eur_btc_fee) = dealer.get_rate(one_eur, Currency::BTC);
    let (eur_usd_rate, _eur_usd_fee) = dealer.get_rate(one_eur, Currency::USD);

    let fields = vec![
        ("btc_usd", btc_usd_rate),
        ("btc_eur", btc_eur_rate),
        ("usd_btc", usd_btc_rate),
        ("usd_eur", usd_eur_rate),
        ("eur_btc", eur_btc_rate),
        ("eur_usd", eur_usd_rate),
    ];

    let builder = fields.into_iter().fold(
        influxdb2::models::DataPoint::builder("swap_rates"),
        |builder, (field_name, value)| {
            if let Some(rate) = value {
                match rate.value.to_f64() {
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
            eprintln!("Failed to write swap rates data point to Influx. Err: {}", err);
        }
    }
}

pub async fn start(settings: DealerEngineSettings, bank_sender: ZmqSocket, bank_recv: ZmqSocket) {
    let (kollider_client_tx, kollider_client_rx) = bounded(2024);

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

    let influx_client = match settings.clone().influx_host {
        Some(host) => Some(Client::new(
            host,
            settings.influx_org.clone(),
            settings.influx_token.clone(),
        )),
        None => None,
    };

    let mut listener = |msg: Message| {
        utils::xzmq::send_as_bincode(&bank_sender, &msg);
    };

    let mut last_health_check = Instant::now();
    let mut last_house_keeping = Instant::now();
    let mut last_risk_check = Instant::now();
    let mut last_influx_quotes = Instant::now();

    loop {
        // Before we proceed we have to have received a bank state message
        if !synth_dealer.has_bank_state() && synth_dealer.is_ready() {
            let msg = Message::Dealer(Dealer::BankStateRequest(BankStateRequest { req_id: Uuid::new_v4() }));
            listener(msg);
            while let Ok(frame) = bank_recv.recv_msg(0) {
                if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                    if let Message::Dealer(Dealer::BankState(ref _bank_state)) = message {
                        synth_dealer.process_msg(message, &mut listener);
                        last_risk_check = Instant::now();
                        break;
                    }
                };
            }
        }

        if let Ok(frame) = bank_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                synth_dealer.process_msg(message, &mut listener);
            };
        }

        if let Ok(message) = kollider_client_rx.try_recv() {
            synth_dealer.process_msg(message, &mut listener);
        }

        if last_risk_check.elapsed().as_secs() > 10 {
            if synth_dealer.has_bank_state() {
                synth_dealer.check_risk(&mut listener);
                last_risk_check = Instant::now();
            }
            if influx_client.is_some() {
                insert_dealer_state(
                    &synth_dealer,
                    &influx_client.as_ref().unwrap(),
                    &settings.influx_bucket.clone(),
                )
                .await;
            }
        }

        if last_health_check.elapsed().as_secs() > 5 {
            synth_dealer.check_health(&mut listener);
            last_health_check = Instant::now();
        }

        if last_house_keeping.elapsed().as_secs() > 30 {
            last_house_keeping = Instant::now();
            synth_dealer.sweep_excess_funds(&mut listener);
        }

        if last_influx_quotes.elapsed().as_secs() > 1 {
            if let Some(ref client) = influx_client {
                store_quotes(&synth_dealer, client, &settings.influx_bucket).await;
                last_influx_quotes = Instant::now();
            }
        }
    }
}
