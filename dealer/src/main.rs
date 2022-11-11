pub mod dealer_engine;

use dealer::dealer_engine::*;
use dealer::start;
use utils::kafka::{Consumer, Producer};

#[tokio::main]
async fn main() {
    let settings = utils::config::get_config_from_env::<DealerEngineSettings>().expect("Failed to load settings.");

    let kafka_consumer = Consumer::new("dealer", "dealer", &settings.kafka_broker_addresses);
    let kafka_producer = Producer::new(&settings.kafka_broker_addresses);

    start(settings, kafka_producer, kafka_consumer).await;
}
