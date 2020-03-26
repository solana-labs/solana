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
    transaction::{self, TransactionError},
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
        request: &RpcRequest,
        params: serde_json::Value,
        _retries: usize,
    ) -> Result<serde_json::Value> {
        if let Some(value) = self.mocks.write().unwrap().remove(request) {
            return Ok(value);
        }
        if self.url == "fails" {
            return Ok(Value::Null);
        }
        let val = match request {
            RpcRequest::ConfirmTransaction => {
                if let Some(params_array) = params.as_array() {
                    if let Value::String(param_string) = &params_array[0] {
                        Value::Bool(param_string == SIGNATURE)
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }
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
            RpcRequest::GetSignatureStatus => {
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
                    Some(TransactionStatus { status, slot: 1 })
                };
                serde_json::to_value(vec![status])?
            }
            RpcRequest::GetTransactionCount => Value::Number(Number::from(1234)),
            RpcRequest::GetSlot => Value::Number(Number::from(0)),
            RpcRequest::SendTransaction => Value::String(SIGNATURE.to_string()),
            RpcRequest::GetMinimumBalanceForRentExemption => Value::Number(Number::from(1234)),
            _ => Value::Null,
        };
        Ok(val)
    }
}
