use electrum_connector::connector_config::ConnectorConfig;
use electrum_connector::electrum_client::ElectrumClient;
use electrum_connector::explorer::BlockExplorer;
use std::time::Duration;
use utils::xzmq::SocketContext;

#[tokio::main]
async fn main() {
    let config = utils::config::get_config_from_env::<ConnectorConfig>().expect("Failed to load settings.");
    let electrum_client = ElectrumClient::new(
        &config.electrum_username,
        &config.electrum_password,
        &config.electrum_path,
        &config.electrum_url,
    );
    let zmq_context = SocketContext::new();
    let listener = |tx_state| println!("Received transaction state: {:?}", tx_state);
    let _block_explorer = BlockExplorer::new(config, zmq_context, &electrum_client, listener)
        .await
        .expect("Failed to create block explorer");
    std::thread::sleep(Duration::from_secs(3600));
}
