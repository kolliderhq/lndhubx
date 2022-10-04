use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalletPathParam {
    pub wallet: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetTotalBalanceResp {
    pub confirmed: f64,
    pub unconfirmed: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAllAddrsBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: WalletPathParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAllAddrsResp {
    pub result: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAddrBalanceParam {
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAddrBalanceBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: GetAddrBalanceParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAddrBalance {
    pub confirmed: f64,
    pub unconfirmed: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAddrBalanceResp {
    pub result: GetAddrBalance,
}

// Begin ValidateAddr
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidateAddrBodyParam {
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidateAddrBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: ValidateAddrBodyParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidateAddrResp {
    pub result: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetNewAddrBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: WalletPathParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetNewAddrResp {
    pub result: String,
}

// Begin GetWalletHistory
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetWalletHistoryParam {
    pub wallet: String,
    pub show_addresses: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetWalletHistoryBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: GetWalletHistoryParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ElectrumOutput {
    pub address: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ElectrumTransaction {
    pub txid: String,
    pub incoming: bool,
    pub outputs: Vec<ElectrumOutput>,
    pub bc_value: String,
    pub timestamp: i64,
    pub height: i64,
    pub confirmations: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetWalletHistoryTxns {
    pub transactions: Vec<ElectrumTransaction>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetWalletHistoryResp {
    pub result: GetWalletHistoryTxns,
}

// Begin IsMine
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IsMineParam {
    pub address: String,
    pub wallet: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IsMineBody {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: IsMineParam,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IsMineResp {
    pub result: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddrRequest {
    pub uid: i64,
    pub address: String,
}
