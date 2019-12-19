use crate::rpc_request::{Response, RpcResponseContext};
use crate::{
    client_error::ClientError, generic_rpc_client_request::GenericRpcClientRequest,
    rpc_request::RpcRequest,
};
use serde_json::{Number, Value};
use solana_sdk::{
    fee_calculator::FeeCalculator,
    instruction::InstructionError,
    transaction::{self, TransactionError},
};

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";
pub const SIGNATURE: &str =
    "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";

pub struct MockRpcClientRequest {
    url: String,
}

impl MockRpcClientRequest {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

impl GenericRpcClientRequest for MockRpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: serde_json::Value,
        _retries: usize,
    ) -> Result<serde_json::Value, ClientError> {
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
            RpcRequest::GetBalance => {
                let n = if self.url == "airdrop" { 0 } else { 50 };
                serde_json::to_value(Response {
                    context: RpcResponseContext { slot: 1 },
                    value: Value::Number(Number::from(n)),
                })?
            }
            RpcRequest::GetRecentBlockhash => serde_json::to_value(Response {
                context: RpcResponseContext { slot: 1 },
                value: (
                    Value::String(PUBKEY.to_string()),
                    serde_json::to_value(FeeCalculator::default()).unwrap(),
                ),
            })?,
            RpcRequest::GetSignatureStatus => {
                let response: Option<transaction::Result<()>> = if self.url == "account_in_use" {
                    Some(Err(TransactionError::AccountInUse))
                } else if self.url == "instruction_error" {
                    Some(Err(TransactionError::InstructionError(
                        0,
                        InstructionError::UninitializedAccount,
                    )))
                } else if self.url == "sig_not_found" {
                    None
                } else {
                    Some(Ok(()))
                };
                serde_json::to_value(response).unwrap()
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
