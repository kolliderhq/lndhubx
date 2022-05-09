pub mod bank_engine;

use bank_engine::*;

use diesel::{r2d2::ConnectionManager, PgConnection};
use zmq::Socket as ZmqSocket;

use core_types::*;
use crossbeam_channel::bounded;
use msgs::*;

use lnd_connector::connector::*;

pub async fn start(
    settings: BankEngineSettings,
    lnd_connector_settings: LndConnectorSettings,
    api_recv: ZmqSocket,
    api_sender: ZmqSocket,
    dealer_sender: ZmqSocket,
    dealer_recv: ZmqSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = r2d2::Pool::builder()
        .build(ConnectionManager::<PgConnection>::new(settings.psql_url.clone()))
        .expect("Failed to create pool.");

    let lnd_connector = LndConnector::new(lnd_connector_settings.clone()).await;
    let mut lnd_connector_invoices = LndConnector::new(lnd_connector_settings).await;

    let (invoice_tx, invoice_rx) = bounded(1024);

    let invoice_task = {
        async move {
            lnd_connector_invoices.sub_invoices(invoice_tx).await;
        }
    };

    tokio::spawn(invoice_task);

    let mut bank_engine = BankEngine::new(Some(pool), lnd_connector, settings).await;
    bank_engine.init_accounts();

    let mut listener = |msg: Message, destination: ServiceIdentity| match destination {
        ServiceIdentity::Api => {
            let payload = bincode::serialize(&msg).unwrap();
            api_sender.send_multipart(vec![vec![], vec![], payload], 0x00).unwrap();
        }
        ServiceIdentity::Dealer => {
            let payload = bincode::serialize(&msg).unwrap();
            dealer_sender.send(payload, 0x00).unwrap();
        }
        _ => {}
    };

    loop {
        // Receiving msgs from the api.
        if let Ok(frame) = api_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                dbg!(&message);
                bank_engine.process_msg(message, &mut listener).await;
            };
        }

        // Receiving msgs from the invoice subscribtion.
        if let Ok(msg) = invoice_rx.try_recv() {
            bank_engine.process_msg(msg, &mut listener).await;
        }

        // Receiving msgs from dealer.
        if let Ok(frame) = dealer_recv.recv_msg(1) {
            if let Ok(message) = bincode::deserialize::<Message>(&frame) {
                bank_engine.process_msg(message, &mut listener).await;
            };
        }
    }
}
