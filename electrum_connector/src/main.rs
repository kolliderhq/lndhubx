use electrum_connector::connector_config::ConnectorConfig;
use electrum_connector::electrum_client::ElectrumClient;
use electrum_connector::explorer::BlockExplorer;
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
    let mut block_explorer = BlockExplorer::new(config, zmq_context, electrum_client)
        .await
        .expect("Failed to create block explorer");
    block_explorer.raw_block_handler().await
}
