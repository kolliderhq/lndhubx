pub mod connector;

use connector::*;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let settings = LndConnectorSettings {
        tls_path: "tls.cert".to_string(),
        macaroon_path: "admin.macaroon".to_string(),
        node_url: "http://lnd.staging.kollider.internal:10009".to_string(),
    };

    let mut lnd_connector = LndConnector::new(settings).await;
    lnd_connector
        .create_invoice(1000, "hello".to_string(), 0, Uuid::new_v4())
        .await
        .expect("Failed to create an invoice");

    // Connecting to LND requires only address, cert file, and macaroon file
    // let mut client = tonic_lnd::connect(address, cert_file, macaroon_file)
    //     .await
    //     .expect("failed to connect");

    // let info = client
    // // All calls require at least empty parameter
    //     .get_info(tonic_lnd::rpc::GetInfoRequest {})
    //     .await
    //     .expect("failed to get info");

    // while let Ok(inv) = client.subscribe_invoices(tonic_lnd::rpc::InvoiceSubscription{add_index: 0, settle_index: 0}).await {
    //     if let Ok(msg) = inv.into_inner().message().await {
    //         dbg!(&msg);
    //     }
    //     // let res: u64 = invoice_subscription;
    //     // dbg!(&inv.into_inner().message());
    // }

    // We only print it here, note that in real-life code you may want to call `.into_inner()` on
    // the response to get the message.
    // println!("{:#?}", info);
}
