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
                Err(_) => Err(Error {
                    code: ErrorCode::ServerError(-32001),
                    message: "Server error: no node found".to_string(),
                    data: None,
                }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use crdt::{get_ip_addr, TestNode};
    use drone::{Drone, DroneRequest};
    use fullnode::FullNode;
    use jsonrpc_http_server::jsonrpc_core::Response;
    use ledger::LedgerWriter;
    use mint::Mint;
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    fn tmp_ledger(name: &str, mint: &Mint) -> String {
        let keypair = Keypair::new();

        let path = format!("/tmp/tmp-ledger-{}-{}", name, keypair.pubkey());

        let mut writer = LedgerWriter::open(&path, true).unwrap();
        writer.write_entries(mint.create_entries()).unwrap();

        path
    }

    #[test]
    fn test_rpc_request() {
        let leader_keypair = Keypair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.data.clone();

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let ledger_path = tmp_ledger("rpc_request", &alice);

        let server = FullNode::new_leader(
            leader_keypair,
            bank,
            0,
            None,
            Some(Duration::from_millis(30)),
            leader,
            exit.clone(),
            &ledger_path,
            false,
        );
        sleep(Duration::from_millis(900));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        format!("0.0.0.0:{}", RPC_PORT);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"solana_getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let meta = Meta {
            leader: Some(leader_data.clone()),
            keypair_location: None,
        };

        let res = io.handle_request_sync(&req, meta.clone());
        let expected = r#"{"jsonrpc":"2.0","result":0,"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let mut addr: SocketAddr = "0.0.0.0:9900".parse().expect("bind to drone socket");
        addr.set_ip(get_ip_addr().expect("drone get_ip_addr"));
        let mut drone = Drone::new(
            alice.keypair(),
            addr,
            leader_data.contact_info.tpu,
            leader_data.contact_info.rpu,
            None,
            Some(150_000),
        );

        let bob_req = DroneRequest::GetAirdrop {
            airdrop_request_amount: 50,
            client_pubkey: bob_pubkey,
        };
        drone.send_airdrop(bob_req).unwrap();

        let res1 = io.handle_request_sync(&req, meta);
        let expected1 = r#"{"jsonrpc":"2.0","result":50,"id":1}"#;
        let expected1: Response =
            serde_json::from_str(expected1).expect("expected response deserialization");

        let result1: Response = serde_json::from_str(&res1.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected1, result1);

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
    #[test]
    fn test_rpc_request_bad_parameter_type() {
        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"solana_getBalance","params":[1234567890]}"#;
        let meta = Meta {
            leader: None,
            keypair_location: None,
        };

        let res = io.handle_request_sync(req, meta);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params: invalid type: integer `1234567890`, expected a string."},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }
    #[test]
    fn test_rpc_request_bad_pubkey() {
        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"solana_getBalance","params":["a1b2c3d4e5"]}"#;
        let meta = Meta {
            leader: None,
            keypair_location: None,
        };

        let res = io.handle_request_sync(req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

}
