use electrum_connector::connector_config::ConnectorConfig;
use electrum_connector::electrum_client::ElectrumClient;
use electrum_connector::explorer::BlockExplorer;
use msgs::blockchain::Blockchain;
use msgs::Message;
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
    let pull_socket = zmq_context.create_pull("tcp://abcd:1234");
    let push_socket = zmq_context.create_push("tcp://dcba:4321");

    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(2048);

    let block_explorer_listener_tx = events_tx.clone();
    let block_explorer_listener = move |tx_state| {
        println!("Received transaction state: {:?}", tx_state);
        if block_explorer_listener_tx
            .blocking_send(Message::Blockchain(Blockchain::BcTransactionState(tx_state)))
            .is_err()
        {
            eprintln!("Failed to send transaction state");
        }
    };
    let _block_explorer = BlockExplorer::new(config, zmq_context, &electrum_client, block_explorer_listener)
        .await
        .expect("Failed to create block explorer");

    std::thread::spawn(move || {
        while let Ok(data) = pull_socket.recv_msg(0x00) {
            if let Ok(incoming_msg) = bincode::deserialize::<Message>(&data) {
                if let Err(err) = events_tx.blocking_send(incoming_msg) {
                    eprintln!("Failed to send message: {:?}", err);
                }
            } else {
                eprintln!("Failed to deserialize incoming payload: {:?}", data);
            }
        }
    });

    while let Some(event) = events_rx.recv().await {
        if let Ok(payload) = bincode::serialize(&event) {
            if push_socket.send(&payload, 0x00).is_err() {
                println!("Failed to send event: {:?}", event);
            }
        } else {
            println!("Failed to serialize event: {:?}", event);
        }
    }
}
