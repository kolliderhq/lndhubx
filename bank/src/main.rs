pub mod bank_engine;

use utils::xzmq::{create_publisher, create_pull, create_push};

use bank::{bank_engine::*, start};
use lnd_connector::connector::LndConnectorSettings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = utils::config::get_config_from_env::<BankEngineSettings>().expect("Failed to load settings.");
    let lnd_connector_settings =
        utils::config::get_config_from_env::<LndConnectorSettings>().expect("Failed to load settings.");

    let api_rx = create_pull(&settings.bank_zmq_pull_address);
    let api_tx = create_publisher(&settings.bank_zmq_publish_address);

    let dealer_tx = create_push(&settings.bank_dealer_push_address);
    let dealer_rx = create_pull(&settings.bank_dealer_pull_address);

    start(settings, lnd_connector_settings, api_rx, api_tx, dealer_tx, dealer_rx).await
}
