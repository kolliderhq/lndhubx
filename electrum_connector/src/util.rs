use crate::error::Error;
use bitcoin::Txid;
use bitcoincore_rpc::json::GetRawTransactionResult;
use bitcoincore_rpc::{Client as BitcoinRpcClient, RpcApi};
use std::str::FromStr;

pub fn get_transaction(rpc_client: &BitcoinRpcClient, tx_hash: &str) -> Result<(GetRawTransactionResult, i64), Error> {
    let txid = Txid::from_str(tx_hash).map_err(|err| Error { error: err.to_string() })?;
    let tx = rpc_client
        .get_raw_transaction_info(&txid, None)
        .map_err(|err| Error { error: err.to_string() })?;
    let mut inputs_value: i64 = 0;
    let mut outputs_value: i64 = 0;
    for vin in tx.vin.iter() {
        if let Some(vin_txid) = vin.txid {
            match rpc_client.get_raw_transaction_info(&vin_txid, None) {
                Ok(vin_lookup) => match vin.vout {
                    Some(vin_vout) => {
                        if let Some(vout) = vin_lookup.vout.get(vin_vout as usize) {
                            if let Some(ref addresses) = vout.script_pub_key.addresses {
                                if !addresses.is_empty() {
                                    inputs_value += vout.value.to_sat() as i64;
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!(
                            "Failed to get vout from vin lookup txid: {} due to missing vout index, vin dump: {:?}",
                            vin_txid, vin
                        );
                        continue;
                    }
                },
                Err(err) => {
                    eprintln!(
                        "Failed to lookup vin txid: {}, error: {}, vin dump: {:?}",
                        vin_txid, err, vin
                    );
                    continue;
                }
            }
        } else {
            eprintln!("Failed to get txid from vin: {:?}", vin);
            continue;
        }
    }
    for vout in tx.vout.iter() {
        if let Some(ref addresses) = vout.script_pub_key.addresses {
            if !addresses.is_empty() {
                outputs_value += vout.value.to_sat() as i64;
            }
        }
    }
    let fees = inputs_value - outputs_value;
    Ok((tx, fees))
}
