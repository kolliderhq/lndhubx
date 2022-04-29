use models::invoices::Invoice;
use msgs::*;
use xerror::lnd_connector::*;

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use utils::time::*;

use core_types::*;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LndConnectorSettings {
    pub node_url: String,
    pub macaroon_path: String,
    pub tls_path: String,
}

pub struct LndConnector {
    _settings: LndConnectorSettings,
    client: tonic_lnd::Client,
}

impl LndConnector {
    pub async fn new(settings: LndConnectorSettings) -> Self {
        let client = tonic_lnd::connect(
            settings.node_url.clone(),
            settings.tls_path.clone(),
            settings.macaroon_path.clone(),
        )
        .await
        .expect("failed to connect");

        Self {
            _settings: settings,
            client,
        }
    }

    pub async fn sub_invoices(&mut self, listener: Sender<Message>) {
        while let Ok(inv) = self
            .client
            .subscribe_invoices(tonic_lnd::rpc::InvoiceSubscription {
                add_index: 0,
                settle_index: 0,
            })
            .await
        {
            if let Ok(Some(invoice)) = inv.into_inner().message().await {
                let invoice_state = tonic_lnd::rpc::invoice::InvoiceState::from_i32(invoice.state).unwrap();
                if invoice_state == tonic_lnd::rpc::invoice::InvoiceState::Settled {
                    let deposit = Deposit {
                        payment_request: invoice.payment_request,
                    };
                    let msg = Message::Deposit(deposit);
                    listener.send(msg).expect("Failed to send a message");
                }
            }
        }
    }

    pub async fn create_invoice(
        &mut self,
        amount: u64,
        memo: String,
        uid: UserId,
        account_id: Uuid,
    ) -> Result<Invoice, LndConnectorError> {
        let invoice = tonic_lnd::rpc::Invoice {
            value: amount as i64,
            memo,
            expiry: 86400,
            ..Default::default()
        };
        if let Ok(resp) = self.client.add_invoice(invoice).await {
            let add_invoice = resp.into_inner();
            let invoice = Invoice {
                uid: uid as i32,
                payment_request: add_invoice.payment_request,
                payment_hash: hex::encode(add_invoice.payment_addr),
                rhash: hex::encode(add_invoice.r_hash),
                created_at: time_now() as i64,
                value: amount as i64,
                value_msat: amount as i64 * 1000,
                expiry: 86400,
                settled: false,
                add_index: add_invoice.add_index as i64,
                settled_date: 0,
                owner: Some(uid as i32),
                account_id: account_id.to_string(),
                incoming: true,
                fees: None,
            };
            return Ok(invoice);
        }
        Err(LndConnectorError::FailedToCreateInvoice)
    }

    pub async fn pay_invoice(&mut self, payment_request: String) -> Result<(), LndConnectorError> {
        // Limiting the fees we pay to 2%.
        let limit = tonic_lnd::rpc::fee_limit::Limit::Fixed(2);
        let fee_limit = tonic_lnd::rpc::FeeLimit { limit: Some(limit) };
        let send_payment = tonic_lnd::rpc::SendRequest {
            payment_request,
            fee_limit: Some(fee_limit),
            allow_self_payment: true,
            ..Default::default()
        };
        if let Ok(resp) = self.client.send_payment_sync(send_payment).await {
            let r = resp.into_inner();
            if !r.payment_error.is_empty() {
                return Err(LndConnectorError::FailedToSendPayment);
            }
            return Ok(());
        }
        Err(LndConnectorError::FailedToSendPayment)
    }

    pub async fn get_node_info(&mut self) -> Result<LndNodeInfo, LndConnectorError> {
        let get_node_info = tonic_lnd::rpc::NodeInfoRequest::default();
        if let Ok(resp) = self.client.get_node_info(get_node_info).await {
            dbg!(&resp);
            return Ok(LndNodeInfo {
                pubkey: String::from("fuck"),
                name: String::from("hello"),
            });
        }
        Err(LndConnectorError::FailedToGetNodeInfo)
    }

    // pub fn lnurl_auth(&self) {
    // }
}
