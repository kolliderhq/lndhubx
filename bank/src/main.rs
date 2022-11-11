pub mod bank_engine;
pub mod ledger;

use bank::{bank_engine::*, start};
use lnd_connector::connector::LndConnectorSettings;
use utils::kafka::{Consumer, Producer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = utils::config::get_config_from_env::<BankEngineSettings>().expect("Failed to load settings.");
    let lnd_connector_settings =
        utils::config::get_config_from_env::<LndConnectorSettings>().expect("Failed to load settings.");

    let kafka_consumer = Consumer::new("bank", "bank", &settings.kafka_broker_addresses);
    let kafka_producer = Producer::new(&settings.kafka_broker_addresses);

    start(settings, lnd_connector_settings, kafka_consumer, kafka_producer).await
}
