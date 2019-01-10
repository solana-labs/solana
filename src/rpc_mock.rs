// Implementation of RpcRequestHandler trait for testing Rpc requests without i/o

use crate::rpc_request::{RpcClient, RpcError, RpcRequestHandler};
use serde_json::{self, Number, Value};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::error;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;

pub enum MockRpcRequest {
    ConfirmTransaction,
    GetAccountInfo,
    GetBalance,
    GetConfirmationTime,
    GetLastId,
    GetSignatureStatus,
    GetTransactionCount,
    RequestAirdrop,
    SendTransaction,
    RegisterNode,
    SignVote,
    DeregisterNode,
    GetStorageMiningLastId,
    GetStorageMiningEntryHeight,
    GetStoragePubkeysForEntryHeight,
}

impl RpcRequestHandler for MockRpcRequest {
    fn make_rpc_request(
        &self,
        client: &RpcClient,
        _id: u64,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>> {
        match &client.addr as &str {
            "succeeds" => {
                match self {
                    MockRpcRequest::ConfirmTransaction => {
                        if let Some(Value::Array(param_array)) = params {
                            if let Value::String(param_string) = &param_array[0] {
                                if param_string == "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8" {
                                    Ok(Value::Bool(true))
                                } else {
                                    Ok(Value::Bool(false))
                                }
                            } else {
                                Err(RpcError::RpcRequestError("Missing parameter".to_string()))?
                            }
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    MockRpcRequest::GetBalance => {
                        Ok(Value::Number(Number::from(50)))
                    }
                    MockRpcRequest::GetLastId => {
                        Ok(Value::String(
                            "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8".to_string(),
                        ))
                    }
                    MockRpcRequest::GetSignatureStatus => {
                        Ok(Value::String(
                            "Confirmed".to_string(),
                        ))
                    }
                    MockRpcRequest::GetTransactionCount => {
                        Ok(Value::Number(Number::from(1234)))
                    }
                    MockRpcRequest::SendTransaction => {
                        Ok(Value::String(
                            "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8".to_string(),
                        ))
                    }
                    _ => {
                        Ok(Value::Null)
                    }
                }
            }
            "airdrop" => {
                match self {
                    MockRpcRequest::GetBalance => {
                        Ok(Value::Number(Number::from(0)))
                    }
                    MockRpcRequest::GetLastId => {
                        Ok(Value::String(
                            "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8".to_string(),
                        ))
                    }
                    MockRpcRequest::GetSignatureStatus => {
                        Ok(Value::String(
                            "Confirmed".to_string(),
                        ))
                    }
                    MockRpcRequest::SendTransaction => {
                        Ok(Value::String(
                            "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8".to_string(),
                        ))
                    }
                    _ => {
                        Ok(Value::Null)
                    }
                }
            }
            "bad_sig_status" => {
                Ok(Value::String("Nonexistant".to_string()))
            }
            _ => {
                Ok(Value::Null)
            }
        }
    }
}

pub fn request_airdrop_transaction(
    _drone_addr: &SocketAddr,
    _id: &Pubkey,
    tokens: u64,
    _last_id: Hash,
) -> Result<Transaction, Error> {
    if tokens == 0 {
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))
    } else {
        let key = Keypair::new();
        let to = Keypair::new().pubkey();
        let last_id = Hash::default();
        let tx = Transaction::system_new(&key, to, 50, last_id);
        Ok(tx)
    }
}
