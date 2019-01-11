// Implementation of RpcRequestHandler trait for testing Rpc requests without i/o

use crate::rpc_request::{RpcClient, RpcError, RpcRequest, RpcRequestHandler};
use serde_json::{self, Number, Value};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::error;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;

pub const PUBKEY: &str = "7RoSF9fUmdphVCpabEoefH81WwrW7orsWonXWqTXkKV8";
pub const SIGNATURE: &str =
    "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";

pub struct MockRpu {}

impl RpcRequestHandler for MockRpu {
    fn make_rpc_request(
        client: &RpcClient,
        _id: u64,
        request: RpcRequest,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>> {
        if &client.addr == "fails" {
            return Ok(Value::Null);
        }
        if request == RpcRequest::ConfirmTransaction {
            if let Some(Value::Array(param_array)) = params {
                if let Value::String(param_string) = &param_array[0] {
                    return Ok(Value::Bool(param_string == SIGNATURE));
                }
                Err(RpcError::RpcRequestError("Missing parameter".to_string()))?
            }
        } else if request == RpcRequest::GetBalance {
            if &client.addr == "airdrop" {
                return Ok(Value::Number(Number::from(0)));
            }
            return Ok(Value::Number(Number::from(50)));
        } else if request == RpcRequest::GetLastId {
            return Ok(Value::String(PUBKEY.to_string()));
        } else if request == RpcRequest::GetSignatureStatus {
            if &client.addr == "account_in_use" {
                return Ok(Value::String("AccountInUse".to_string()));
            } else if &client.addr == "bad_sig_status" {
                return Ok(Value::String("Nonexistant".to_string()));
            }
            return Ok(Value::String("Confirmed".to_string()));
        } else if request == RpcRequest::GetTransactionCount {
            return Ok(Value::Number(Number::from(1234)));
        } else if request == RpcRequest::SendTransaction {
            return Ok(Value::String(SIGNATURE.to_string()));
        }
        Ok(Value::Null)
    }
}

pub fn request_airdrop_transaction(
    _drone_addr: &SocketAddr,
    _id: &Pubkey,
    tokens: u64,
    _last_id: Hash,
) -> Result<Transaction, Error> {
    if tokens == 0 {
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))?
    }
    let key = Keypair::new();
    let to = Keypair::new().pubkey();
    let last_id = Hash::default();
    let tx = Transaction::system_new(&key, to, 50, last_id);
    Ok(tx)
}
