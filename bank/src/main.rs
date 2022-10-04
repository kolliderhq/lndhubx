pub mod bank_engine;

use utils::xzmq::SocketContext;

use bank::{bank_engine::*, start, BankSockets};
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

    let cli_socket = context.create_response(&settings.bank_cli_resp_address);

    let electrum_connector_tx = context.create_push(&settings.bank_electrum_connector_address);
    let electrum_connector_rx = context.create_pull(&settings.electrum_connector_bank_address);

    let sockets = BankSockets {
        api_recv: api_rx,
        api_sender: api_tx,
        dealer_sender: dealer_tx,
        dealer_recv: dealer_rx,
        cli_socket,
        electrum_sender: electrum_connector_tx,
        electrum_recv: electrum_connector_rx,
    };
    start(settings, lnd_connector_settings, sockets).await
}
