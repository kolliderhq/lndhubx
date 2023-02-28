use models::invoices::Invoice;
use msgs::*;
use xerror::lnd_connector::*;

use crossbeam_channel::Sender;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use utils::time::*;

use core_types::*;
use sha256::digest;
use unescape::unescape;
use uuid::Uuid;

use std::collections::HashMap;

const MINIMUM_FEE: i64 = 10;

#[derive(Debug, Clone)]
pub struct PayResponse {
    pub payment_hash: String,
    pub fee: u64,
    pub preimage: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LndConnectorSettings {
    pub host: String,
    pub port: u32,
    pub macaroon_path: String,
    pub tls_path: String,
}

pub struct LndConnector {
    _settings: LndConnectorSettings,
    ln_client: tonic_openssl_lnd::LndLightningClient,
    _router_client: tonic_openssl_lnd::LndRouterClient,
}

impl LndConnector {
    pub async fn new(settings: LndConnectorSettings) -> Self {
        let ln_client = tonic_openssl_lnd::connect_lightning(
            settings.host.clone(),
            settings.port,
            settings.tls_path.clone(),
            settings.macaroon_path.clone(),
        )
        .await
        .expect("failed to connect");

        let router_client = tonic_openssl_lnd::connect_router(
            settings.host.clone(),
            settings.port,
            settings.tls_path.clone(),
            settings.macaroon_path.clone(),
        )
        .await
        .expect("failed to connect");

        Self {
            _settings: settings,
            ln_client,
            _router_client: router_client,
        }
    }

    pub async fn sub_invoices(&mut self, listener: Sender<Message>) {
        loop {
            while let Ok(inv) = self
                .ln_client
                .subscribe_invoices(tonic_openssl_lnd::lnrpc::InvoiceSubscription {
                    add_index: 0,
                    settle_index: 0,
                })
                .await
            {
                if let Ok(Some(invoice)) = inv.into_inner().message().await {
                    if let Some(tonic_openssl_lnd::lnrpc::invoice::InvoiceState::Settled) =
                        tonic_openssl_lnd::lnrpc::invoice::InvoiceState::from_i32(invoice.state)
                    {
                        let deposit = Deposit {
                            payment_request: invoice.payment_request,
                            payment_hash: hex::encode(invoice.r_hash),
                            description_hash: hex::encode(invoice.description_hash),
                            preimage: hex::encode(invoice.r_preimage),
                            value: invoice.value as u64,
                            value_msat: invoice.value_msat as u64,
                            settled: true,
                            creation_date: invoice.creation_date as u64,
                            settle_date: invoice.settle_date as u64,
                        };
                        let msg = Message::Deposit(deposit);
                        listener.send(msg).expect("Failed to send a message");
                    }
                }
            }
            // Sleeping for a little bit before trying to reconnect.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    pub async fn create_invoice(
        &mut self,
        amount: u64,
        memo: String,
        uid: UserId,
        account_id: Uuid,
        metadata: Option<String>,
    ) -> Result<Invoice, LndConnectorError> {
        let hash = match metadata {
            Some(ref m) => {
                if let Some(une) = unescape(m) {
                    digest(une)
                } else {
                    return Err(LndConnectorError::FailedToCreateInvoice);
                }
            }
            None => String::from(""),
        };
        let description_hash = hex::decode(hash).expect("Decoding failed");

        let invoice = tonic_openssl_lnd::lnrpc::Invoice {
            value: amount as i64,
            memo: memo.clone(),
            expiry: 86400,
            description_hash,
            ..Default::default()
        };
        if let Ok(resp) = self.ln_client.add_invoice(invoice).await {
            let add_invoice = resp.into_inner();
            let invoice = Invoice {
                uid: uid as i32,
                payment_request: add_invoice.payment_request,
                payment_hash: hex::encode(add_invoice.r_hash.clone()),
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
                currency: None,
                target_account_currency: None,
                reference: Some(memo),
                description: metadata,
            };
            return Ok(invoice);
        }
        Err(LndConnectorError::FailedToCreateInvoice)
    }

    pub async fn pay_invoice(
        &mut self,
        payment_request: Option<String>,
        key_send: Option<(String, [u8; 32])>,
        amount_in_sats: Decimal,
        max_fee_as_pp: Option<Decimal>,
        max_fee_in_sats: Option<Decimal>,
    ) -> Result<PayResponse, LndConnectorError> {
        if max_fee_as_pp.is_none() && max_fee_in_sats.is_none() {
            return Err(LndConnectorError::FailedToSendPayment);
        }
        let mut max_fee = match max_fee_as_pp {
            Some(m) => (amount_in_sats * m).round_dp(0).to_i64().unwrap_or(0),
            None => 0,
        };

        max_fee = match max_fee_in_sats {
            Some(m) => (m).round_dp(0).to_i64().unwrap_or(max_fee),
            None => max_fee,
        };

        let key_send_request = if let Some((dest, key)) = key_send {
            // If we do key send we have to supply payment hash.
            let sha256_hash_string = sha256::digest(&key);
            let dest = hex::decode(dest).expect("Decoding keysend dest failed");
            let mut custom_records: HashMap<u64, Vec<u8>> = HashMap::new();
            let sha256_hash = hex::decode(sha256_hash_string).expect("Decoding keysend preimage failed");
            custom_records.insert(5482373484, key.clone().to_vec());
            let limit = tonic_openssl_lnd::lnrpc::fee_limit::Limit::Fixed(max_fee.to_i64().unwrap());
            let fee_limit = tonic_openssl_lnd::lnrpc::FeeLimit { limit: Some(limit) };
            let key_send_request = tonic_openssl_lnd::lnrpc::SendRequest {
                dest,
                amt: amount_in_sats.to_i64().unwrap(),
                payment_hash: sha256_hash,
                dest_features: vec![tonic_openssl_lnd::lnrpc::FeatureBit::TlvOnionReq as i32],
                fee_limit: Some(fee_limit),
                dest_custom_records: custom_records,
                ..Default::default()
            };
            Some(key_send_request)
        } else {
            None
        };

        let send_request = if let Some(pr) = payment_request {
            let limit = tonic_openssl_lnd::lnrpc::fee_limit::Limit::Fixed(max_fee);
            let fee_limit = tonic_openssl_lnd::lnrpc::FeeLimit { limit: Some(limit) };
            let send_request = tonic_openssl_lnd::lnrpc::SendRequest {
                payment_request: pr,
                fee_limit: Some(fee_limit),
                allow_self_payment: true,
                ..Default::default()
            };
            Some(send_request)
        } else {
            key_send_request
        };

        if send_request.is_none() {
            return Err(LndConnectorError::FailedToSendPayment);
        }

        if let Ok(resp) = self.ln_client.send_payment_sync(send_request.unwrap()).await {
            let r = resp.into_inner();
            if !r.payment_error.is_empty() {
                dbg!(format!("Payment error: {:?}", r.payment_error));
                return Err(LndConnectorError::FailedToSendPayment);
            }
            let fee = match r.payment_route {
                Some(pr) => pr.total_fees.try_into().unwrap_or(0),
                None => 0,
            };
            let response = PayResponse {
                fee,
                payment_hash: hex::encode(r.payment_hash),
                preimage: Some(hex::encode(r.payment_preimage)),
            };
            return Ok(response);
        }
        Err(LndConnectorError::FailedToSendPayment)
    }

    pub async fn get_node_info(&mut self) -> Result<LndNodeInfo, LndConnectorError> {
        let get_info = tonic_openssl_lnd::lnrpc::GetInfoRequest::default();
        match self.ln_client.get_info(get_info).await {
            Ok(ni) => {
                let resp = ni.into_inner();
                let lnd_node_info = LndNodeInfo {
                    identity_pubkey: resp.identity_pubkey,
                    uris: resp.uris,
                    num_active_channels: resp.num_active_channels as u64,
                    num_pending_channels: resp.num_pending_channels as u64,
                    num_peers: resp.num_peers as u64,
                    testnet: resp.testnet,
                };
                Ok(lnd_node_info)
            }
            Err(err) => {
                dbg!(&err);
                Err(LndConnectorError::FailedToGetNodeInfo)
            }
        }
    }

    pub async fn decode_payment_request(
        &mut self,
        payment_request: String,
    ) -> Result<tonic_openssl_lnd::lnrpc::PayReq, LndConnectorError> {
        let decode = tonic_openssl_lnd::lnrpc::PayReqString {
            pay_req: payment_request,
        };

        if let Ok(resp) = self.ln_client.decode_pay_req(decode).await {
            let inner = resp.into_inner();
            Ok(inner)
        } else {
            Err(LndConnectorError::FailedToDecodePaymentRequest)
        }
    }

    pub async fn probe(
        &mut self,
        payment_request: String,
        max_fee: Decimal,
    ) -> Result<std::vec::Vec<tonic_openssl_lnd::lnrpc::Route>, LndConnectorError> {
        // Max fee is always a percentage of amount.
        let decode = tonic_openssl_lnd::lnrpc::PayReqString {
            pay_req: payment_request.clone(),
        };

        if let Ok(resp) = self.ln_client.decode_pay_req(decode).await {
            let r = resp.into_inner();
            let max_fee = (Decimal::new(r.num_satoshis, 0) * max_fee)
                .round_dp(0)
                .to_i64()
                .unwrap_or(MINIMUM_FEE);
            // Never send a payment with lower fee than 10.
            let max_fee = std::cmp::max(max_fee, MINIMUM_FEE);
            let limit = tonic_openssl_lnd::lnrpc::fee_limit::Limit::Fixed(max_fee);
            let fee_limit = tonic_openssl_lnd::lnrpc::FeeLimit { limit: Some(limit) };
            let query_routes = tonic_openssl_lnd::lnrpc::QueryRoutesRequest {
                pub_key: r.destination,
                amt: r.num_satoshis,
                fee_limit: Some(fee_limit),
                use_mission_control: true,
                ..Default::default()
            };
            match self.ln_client.query_routes(query_routes).await {
                Ok(pr) => {
                    let resp = pr.into_inner();
                    Ok(resp.routes)
                }
                Err(err) => {
                    dbg!(&err);
                    Err(LndConnectorError::FailedToQueryRoutes)
                }
            }
        } else {
            Err(LndConnectorError::FailedToQueryRoutes)
        }
    }
}
