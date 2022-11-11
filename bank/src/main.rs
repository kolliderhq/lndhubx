pub mod bank_engine;
pub mod ledger;

use bank::{bank_engine::*, start};
use lnd_connector::connector::LndConnectorSettings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = utils::config::get_config_from_env::<BankEngineSettings>().expect("Failed to load settings.");
    let lnd_connector_settings =
        utils::config::get_config_from_env::<LndConnectorSettings>().expect("Failed to load settings.");

    start(settings, lnd_connector_settings).await
}
