use core_types::Currency;
use kollider_hedging::KolliderHedgingClient;
use msgs::kollider_client::{Channel, KolliderApiResponse};
use msgs::Message;
use std::time::{Duration, Instant};
use ws_client::WsClient;

fn subscribe(client: &KolliderHedgingClient) {
    client
        .subscribe_all(vec![Channel::PositionStates, Channel::MarkPrices])
        .expect("Failed to subscribe to channels");
}

fn main() {
    dotenv::dotenv().ok();
    let kollider_url = std::env::var("KOLLIDER_URL").expect("KOLLIDER_URL not defined");
    let api_key = std::env::var("API_KEY").expect("API_KEY not defined");
    let api_secret = std::env::var("API_SECRET").expect("API_SECRET not defined");
    let api_passphrase = std::env::var("API_PASSPHRASE").expect("API_PASSPHRASE not defined");

    let (tx, rx) = crossbeam::channel::unbounded();
    let client = KolliderHedgingClient::connect(&kollider_url, &api_key, &api_secret, &api_passphrase, tx)
        .expect("Failed to create a client");
    subscribe(&client);
    let begin = Instant::now();
    loop {
        let elapsed_secs = begin.elapsed().as_secs();
        let should_buy = elapsed_secs <= 30;
        if elapsed_secs > 60 {
            break;
        }
        [Currency::BTC, Currency::USD]
            .iter()
            .for_each(|currency| match client.get_balance(*currency) {
                Ok(balance) => println!("Your {} balance is {}", currency, balance),
                Err(err) => println!("Your {} balance: {:?}", currency, err),
            });
        if should_buy {
            if client.buy(1, Currency::USD).is_err() {
                eprintln!("Failed to buy fiat currency");
            }
        } else if client.sell(1, Currency::USD).is_err() {
            eprintln!("Failed to sell fiat currency");
        }

        while let Ok(msg) = rx.try_recv() {
            match msg {
                Message::KolliderApiResponse(KolliderApiResponse::Reconnected(reconnection)) => {
                    println!("Reconnected at {}", reconnection.timestamp);
                    println!("Resubscribing channels");
                    subscribe(&client);
                }
                Message::KolliderApiResponse(KolliderApiResponse::Disconnected(disconnection)) => {
                    println!("Disconnected at {}", disconnection.timestamp)
                }
                _ => {}
            }
        }

        std::thread::sleep(Duration::from_millis(1000));
    }
}
