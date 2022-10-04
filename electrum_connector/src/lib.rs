use crate::connector_config::ConnectorConfig;
use crate::electrum_client::ElectrumClient;
use crate::explorer::BlockExplorer;
use msgs::blockchain::{Blockchain, BtcReceiveAddress};
use msgs::Message;
use utils::xzmq::SocketContext;

pub mod bitcoin;
pub mod connector_config;
pub mod electrum_client;
pub mod error;
pub mod explorer;
pub mod messages;
pub mod tracked_state;
pub mod util;
pub mod zmq_handlers;

pub const SATS_IN_BITCOIN: f64 = 100_000_000.0;
pub const MAX_BLOCK: i64 = 10_000_000_000_000_000;

pub async fn start(config: ConnectorConfig, zmq_context: &SocketContext) {
    let pull_socket = zmq_context.create_pull(&config.bank_electrum_connector_address);
    let push_socket = zmq_context.create_push(&config.electrum_connector_bank_address);

    let electrum_client = ElectrumClient::new(
        &config.electrum_username,
        &config.electrum_password,
        &config.electrum_path,
        &config.electrum_url,
    );

    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(2048);
    let (address_uid_tx, mut address_uid_rx) = tokio::sync::mpsc::channel(2048);

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

    tokio::spawn(async move {
        while let Some(uid) = address_uid_rx.recv().await {
            let response = match electrum_client.get_new_address().await {
                Ok(address) => BtcReceiveAddress {
                    uid,
                    address: Some(address),
                },
                Err(_) => BtcReceiveAddress { uid, address: None },
            };
            let msg = Message::Blockchain(Blockchain::BtcReceiveAddress(response));
            if let Err(err) = events_tx.send(msg).await {
                eprintln!("Failed to send an address response for uid: {}, error: {:?}", uid, err);
            }
        }
    });

    std::thread::spawn(move || {
        while let Ok(data) = pull_socket.recv_msg(0x00) {
            if let Ok(incoming_msg) = bincode::deserialize::<Message>(&data) {
                if let Message::Blockchain(Blockchain::BtcReceiveAddressRequest(address_request)) = incoming_msg {
                    if let Err(err) = address_uid_tx.blocking_send(address_request.uid) {
                        eprintln!("Failed to send message: {:?}", err);
                    }
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
