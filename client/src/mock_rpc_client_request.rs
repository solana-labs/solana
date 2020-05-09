use crate::{
    client_error::Result,
    generic_rpc_client_request::GenericRpcClientRequest,
    rpc_request::RpcRequest,
    rpc_response::{Response, RpcResponseContext},
};
use serde_json::{Number, Value};
use solana_sdk::{
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    instruction::InstructionError,
    signature::Signature,
    transaction::{self, Transaction, TransactionError},
};
use solana_transaction_status::TransactionStatus;
use std::{collections::HashMap, sync::RwLock};

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";
pub const SIGNATURE: &str =
    "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";

pub type Mocks = HashMap<RpcRequest, Value>;
pub struct MockRpcClientRequest {
    mocks: RwLock<Mocks>,
    url: String,
}

impl MockRpcClientRequest {
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

impl GenericRpcClientRequest for MockRpcClientRequest {
    fn send(
        &self,
        request: RpcRequest,
        params: serde_json::Value,
        _retries: usize,
    ) -> Result<serde_json::Value> {
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
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1 },
                    value: vec![status],
                })?
            }
            RpcRequest::GetTransactionCount => Value::Number(Number::from(1234)),
            RpcRequest::GetSlot => Value::Number(Number::from(0)),
            RpcRequest::SendTransaction => {
                let signature = if self.url == "malicious" {
                    Signature::new(&[8; 64]).to_string()
                } else {
                    let tx_str = params.as_array().unwrap()[0].as_str().unwrap().to_string();
                    let data = bs58::decode(tx_str).into_vec().unwrap();
                    let tx: Transaction = bincode::deserialize(&data).unwrap();
                    tx.signatures[0].to_string()
                };
                Value::String(signature)
            }
            RpcRequest::GetMinimumBalanceForRentExemption => Value::Number(Number::from(1234)),
            _ => Value::Null,
        };
        Ok(val)
    }
}
