//! The `rpc` module implements the Solana rpc interface.

use bs58;
use client::mk_client;
use crdt::NodeInfo;
use jsonrpc_http_server::jsonrpc_core::*;
use signature::{read_keypair, KeypairUtil, Pubkey, Signature};
use std::mem;

pub const RPC_PORT: u16 = 8899;

#[derive(Clone)]
pub struct Meta {
    pub leader: Option<NodeInfo>,
    pub keypair_location: Option<String>,
}
impl Metadata for Meta {}
impl Default for Meta {
    fn default() -> Self {
        Meta {
            leader: None,
            keypair_location: None,
        }
    }
}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "solana_getAddress")]
        fn address(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "solana_confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "solana_getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<i64>;

        #[rpc(meta, name = "solana_getTransactionCount")]
        fn get_transaction_count(&self, Self::Metadata) -> Result<u64>;

        #[rpc(meta, name = "solana_sendTransaction")]
        fn send_transaction(&self, Self::Metadata, String, i64) -> Result<String>;
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn address(&self, meta: Self::Metadata) -> Result<String> {
        let client_keypair = read_keypair(&meta.keypair_location.unwrap());
        Ok(bs58::encode(client_keypair.unwrap().pubkey()).into_string())
    }
    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        let signature_vec = bs58::decode(id)
            .into_vec()
            .expect("base58-encoded public key");

        if signature_vec.len() != mem::size_of::<Signature>() {
            Err(Error::invalid_request())
        } else {
            let signature = Signature::new(&signature_vec);

            let mut client = mk_client(&meta.leader.unwrap());

            let confirmation = client.check_signature(&signature);
            Ok(confirmation)
        }
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<i64> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .expect("base58-encoded public key");

        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(Error::invalid_request())
        } else {
            let pubkey = Pubkey::new(&pubkey_vec);

            let mut client = mk_client(&meta.leader.unwrap());

            let balance = client.poll_get_balance(&pubkey);
            match balance {
                Ok(balance) => Ok(balance),
                Err(_) => Err(Error::new(ErrorCode::ServerError(-32001))),
            }
        }
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        let mut client = mk_client(&meta.leader.unwrap());
        let tx_count = client.transaction_count();
        Ok(tx_count)
    }
    fn send_transaction(&self, meta: Self::Metadata, to: String, tokens: i64) -> Result<String> {
        let client_keypair = read_keypair(&meta.keypair_location.unwrap()).unwrap();
        let mut client = mk_client(&meta.leader.unwrap());
        let last_id = client.get_last_id();
        let to_pubkey_vec = bs58::decode(to)
            .into_vec()
            .expect("base58-encoded public key");

        if to_pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(Error::invalid_request())
        } else {
            let to_pubkey = Pubkey::new(&to_pubkey_vec);
            let signature = client
                .transfer(tokens, &client_keypair, to_pubkey, &last_id)
                .unwrap();
            Ok(bs58::encode(signature).into_string())
        }
    }
}
