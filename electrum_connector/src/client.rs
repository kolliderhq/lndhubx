use crate::bitcoin::{Output, Transaction};
use crate::messages::*;
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::ops::Add;
use std::time::{Duration, SystemTime};

const SATS_IN_BITCOIN: i64 = 100_000_000;

const JSON_RPC_VER: &str = "2.0";
const CURL_TEXT_ID: &str = "curltext";
const LIST_ADDRESSES_METHOD: &str = "listaddresses";
const GET_ADDRESS_BALANCE_METHOD: &str = "getaddressbalance";
const CREATE_NEW_ADDRESS_METHOD: &str = "createnewaddress";
const IS_MINE_METHOD: &str = "ismine";
const VALIDATE_ADDRESS_METHOD: &str = "validateaddress";
const ON_CHAIN_HISTORY_METHOD: &str = "onchain_history";

pub struct Client {
    username: String,
    password: String,
    wallet_path: String,
    rpc_url: String,
    http_client: HttpClient,
}

impl Client {
    pub fn new(username: &str, password: &str, wallet_path: &str, rpc_url: &str) -> Self {
        let http_client = reqwest::Client::new();
        Self {
            username: username.to_string(),
            password: password.to_string(),
            wallet_path: wallet_path.to_string(),
            rpc_url: rpc_url.to_string(),
            http_client,
        }
    }

    pub async fn get_total_balance(&self) -> Result<GetTotalBalanceResp, Error> {
        let all_addresses = self.get_all_addresses().await?;
        let mut total_balance = GetTotalBalanceResp {
            confirmed: 0,
            unconfirmed: 0,
        };
        for addr in all_addresses {
            let addr_balance = self.get_address_balance(addr).await?;
            total_balance.confirmed += addr_balance.confirmed;
            total_balance.unconfirmed += addr_balance.unconfirmed;
        }
        Ok(total_balance)
    }

    fn get_base_url(&self) -> String {
        format!("http://{}:{}@{}", self.username, self.password, self.rpc_url)
    }

    async fn post<T>(&self, msg: &T) -> Result<reqwest::Response, Error>
    where
        T: Serialize,
    {
        let body = serde_json::to_string(msg).map_err(|err| Error { error: err.to_string() })?;
        let url = self.get_base_url();
        self.http_client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|err| Error { error: err.to_string() })
    }

    async fn get_all_addresses(&self) -> Result<Vec<String>, Error> {
        let params = WalletPathParam {
            wallet: self.wallet_path.clone(),
        };

        let msg = GetAllAddrsBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: LIST_ADDRESSES_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let all_addresses =
                serde_json::from_slice::<GetAllAddrsResp>(&body).map_err(|err| Error { error: err.to_string() })?;
            Ok(all_addresses.result)
        } else {
            Err(Error {
                error: "Could not get all addresses".to_string(),
            })
        }
    }

    pub async fn validate_address(&self, address: String) -> Result<bool, Error> {
        let params = ValidateAddrBodyParam { address };

        let msg = ValidateAddrBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: VALIDATE_ADDRESS_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let validated =
                serde_json::from_slice::<ValidateAddrResp>(&body).map_err(|err| Error { error: err.to_string() })?;
            Ok(validated.result)
        } else {
            Err(Error {
                error: "Could not validate addresses".to_string(),
            })
        }
    }

    pub async fn get_new_address(&self) -> Result<String, Error> {
        let params = WalletPathParam {
            wallet: self.wallet_path.clone(),
        };

        let msg = GetNewAddrBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: CREATE_NEW_ADDRESS_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let new_address =
                serde_json::from_slice::<GetNewAddrResp>(&body).map_err(|err| Error { error: err.to_string() })?;
            Ok(new_address.result)
        } else {
            Err(Error {
                error: "Could not get new address".to_string(),
            })
        }
    }

    async fn get_address_balance(&self, address: String) -> Result<GetAddrBalance, Error> {
        let params = GetAddrBalanceParam { address };

        let msg = GetAddrBalanceBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: GET_ADDRESS_BALANCE_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let address_balance =
                serde_json::from_slice::<GetAddrBalanceResp>(&body).map_err(|err| Error { error: err.to_string() })?;
            Ok(address_balance.result)
        } else {
            Err(Error {
                error: format!("Could not get {} address balance", msg.params.address),
            })
        }
    }

    pub async fn get_wallet_history(&self) -> Result<Vec<Transaction>, Error> {
        let params = GetWalletHistoryParam {
            wallet: self.wallet_path.clone(),
            show_addresses: true,
        };

        let msg = GetWalletHistoryBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: ON_CHAIN_HISTORY_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        let electrum_transactions = if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let wallet_history_response = serde_json::from_slice::<GetWalletHistoryResp>(&body)
                .map_err(|err| Error { error: err.to_string() })?;
            wallet_history_response.result.transactions
        } else {
            return Err(Error {
                error: "Could not get wallet history".to_string(),
            });
        };

        let mut transactions = Vec::with_capacity(electrum_transactions.len());
        for etx in electrum_transactions {
            let bc_value = if let Ok(bc_value) = etx.bc_value.parse() {
                bc_value
            } else {
                eprintln!(
                    "Failed to convert bc_value: {} to f64. Discarding electrum transaction: {:?}",
                    etx.bc_value, etx
                );
                continue;
            };
            let mut outputs = Vec::with_capacity(etx.outputs.len());
            for eout in etx.outputs {
                let value = if let Ok(value) = eout.value.parse::<i64>() {
                    value * SATS_IN_BITCOIN
                } else {
                    eprintln!(
                        "Failed to convert output value: {} to f64. Discarding electrum transaction output: {:?}",
                        eout.value, eout
                    );
                    continue;
                };
                let output = Output {
                    address: eout.address,
                    value,
                };
                outputs.push(output);
            }
            let timestamp = SystemTime::UNIX_EPOCH.add(Duration::from_secs(etx.timestamp as u64));
            let transaction = Transaction {
                txid: etx.txid,
                incoming: etx.incoming,
                outputs,
                bc_value,
                timestamp,
                height: etx.height,
                confirmations: etx.confirmations,
            };
            transactions.push(transaction);
        }
        Ok(transactions)
    }

    pub async fn is_mine(&self, address: String) -> Result<bool, Error> {
        let params = IsMineParam {
            address,
            wallet: self.wallet_path.clone(),
        };

        let msg = IsMineBody {
            jsonrpc: JSON_RPC_VER.to_string(),
            id: CURL_TEXT_ID.to_string(),
            method: IS_MINE_METHOD.to_string(),
            params,
        };

        let resp = self.post(&msg).await?;
        if resp.status() == reqwest::StatusCode::OK {
            let body = resp.bytes().await.map_err(|err| Error { error: err.to_string() })?;
            let is_mine_result =
                serde_json::from_slice::<IsMineResp>(&body).map_err(|err| Error { error: err.to_string() })?;
            Ok(is_mine_result.result)
        } else {
            Err(Error {
                error: format!("Could not get is mine on {} address", msg.params.address),
            })
        }
    }
}
