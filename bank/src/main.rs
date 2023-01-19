pub mod bank_engine;
pub mod ledger;

use utils::xzmq::SocketContext;

use bank::{bank_engine::*, start};
use lnd_connector::connector::LndConnectorSettings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = utils::config::get_config_from_env::<BankEngineSettings>().expect("Failed to load settings.");
    let lnd_connector_settings =
        utils::config::get_config_from_env::<LndConnectorSettings>().expect("Failed to load settings.");

    let context = SocketContext::new();
    let api_rx = context.create_pull(&settings.bank_zmq_pull_address);
    let api_tx = context.create_publisher(&settings.bank_zmq_publish_address);

    let dealer_tx = context.create_push(&settings.bank_dealer_push_address);
    let dealer_rx = context.create_pull(&settings.bank_dealer_pull_address);

    let nostr_tx = context.create_push(&settings.bank_nostr_push_address);
    let nostr_rx = context.create_pull(&settings.bank_nostr_pull_address);

    let cli_socket = context.create_response(&settings.bank_cli_resp_address);

    start(
        settings,
        lnd_connector_settings,
        api_rx,
        api_tx,
        dealer_tx,
        dealer_rx,
        nostr_tx,
        nostr_rx,
        cli_socket,
    )
    .await
}
