use crate::{
    client_error::Result,
    rpc_request::RpcRequest,
    rpc_response::{Response, RpcResponseContext, RpcVersionInfo},
    rpc_sender::RpcSender,
};
use serde_json::{json, Number, Value};
use solana_sdk::{
    epoch_info::EpochInfo,
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    instruction::InstructionError,
    signature::Signature,
    transaction::{self, Transaction, TransactionError},
};
use solana_transaction_status::TransactionStatus;
use solana_version::Version;
use std::{collections::HashMap, sync::RwLock};

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";
pub const SIGNATURE: &str =
    "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";

pub type Mocks = HashMap<RpcRequest, Value>;
pub struct MockSender {
    mocks: RwLock<Mocks>,
    url: String,
}

impl MockSender {
    pub fn new(url: String) -> Self {
        Self::new_with_mocks(url, Mocks::default())
    }

    pub fn new_with_mocks(url: String, mocks: Mocks) -> Self {
        Self {
            url,
            mocks: RwLock::new(mocks),
        }
    }
}

impl RpcSender for MockSender {
    fn send(&self, request: RpcRequest, params: serde_json::Value) -> Result<serde_json::Value> {
        if let Some(value) = self.mocks.write().unwrap().remove(&request) {
            return Ok(value);
        }
        if self.url == "fails" {
            return Ok(Value::Null);
        }
        let val = match request {
            RpcRequest::GetBalance => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: Value::Number(Number::from(50)),
            })?,
            RpcRequest::GetRecentBlockhash => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: (
                    Value::String(PUBKEY.to_string()),
                    serde_json::to_value(FeeCalculator::default()).unwrap(),
                ),
            })?,
            RpcRequest::GetEpochInfo => serde_json::to_value(EpochInfo {
                epoch: 1,
                slot_index: 2,
                slots_in_epoch: 32,
                absolute_slot: 34,
                block_height: 34,
                transaction_count: Some(123),
            })?,
            RpcRequest::GetFeeCalculatorForBlockhash => {
                let value = if self.url == "blockhash_expired" {
                    Value::Null
                } else {
                    serde_json::to_value(Some(FeeCalculator::default())).unwrap()
                };
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1 },
                    value,
                })?
            }
            RpcRequest::GetFeeRateGovernor => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: serde_json::to_value(FeeRateGovernor::default()).unwrap(),
            })?,
            RpcRequest::GetSignatureStatuses => {
                let status: transaction::Result<()> = if self.url == "account_in_use" {
                    Err(TransactionError::AccountInUse)
                } else if self.url == "instruction_error" {
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::UninitializedAccount,
                    ))
                } else {
                    Ok(())
                };
                let status = if self.url == "sig_not_found" {
                    None
                } else {
                    let err = status.clone().err();
                    Some(TransactionStatus {
                        status,
                        slot: 1,
                        confirmations: None,
                        err,
                    })
                };
                let statuses: Vec<Option<TransactionStatus>> = params.as_array().unwrap()[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|_| status.clone())
                    .collect();
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1 },
                    value: statuses,
                })?
            }
            RpcRequest::GetTransactionCount => Value::Number(Number::from(1234)),
            RpcRequest::GetSlot => Value::Number(Number::from(0)),
            RpcRequest::SendTransaction => {
                let signature = if self.url == "malicious" {
                    Signature::new(&[8; 64]).to_string()
                } else {
                    let tx_str = params.as_array().unwrap()[0].as_str().unwrap().to_string();
                    let data = base64::decode(tx_str).unwrap();
                    let tx: Transaction = bincode::deserialize(&data).unwrap();
                    tx.signatures[0].to_string()
                };
                Value::String(signature)
            }
            RpcRequest::GetMinimumBalanceForRentExemption => Value::Number(Number::from(20)),
            RpcRequest::GetVersion => {
                let version = Version::default();
                json!(RpcVersionInfo {
                    solana_core: version.to_string(),
                    feature_set: Some(version.feature_set),
                })
            }
            _ => Value::Null,
        };
        Ok(val)
    }
}
