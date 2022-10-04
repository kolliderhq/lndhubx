use electrum_connector::connector_config::ConnectorConfig;
use electrum_connector::start;
use utils::xzmq::SocketContext;

#[tokio::main]
async fn main() {
    let config = utils::config::get_config_from_env::<ConnectorConfig>().expect("Failed to load settings.");
    let zmq_context = SocketContext::new();
    start(config, &zmq_context).await;
}
