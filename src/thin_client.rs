//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use crate::cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo};
use crate::fullnode::{Fullnode, FullnodeConfig};
use crate::gossip_service::GossipService;
use crate::packet::PACKET_DATA_SIZE;
use crate::result::{Error, Result};
use crate::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use bincode::serialize_into;
use bs58;
use log::Level;
use serde_json;
use solana_metrics;
use solana_metrics::influxdb;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing;
use solana_sdk::transaction::Transaction;
use std;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    rpc_addr: SocketAddr,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    rpc_client: RpcClient,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `transactions_addr` over `transactions_socket` using UDP.
    pub fn new(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
    ) -> Self {
        Self::new_from_client(
            rpc_addr,
            transactions_addr,
            transactions_socket,
            RpcClient::new_from_socket(rpc_addr),
        )
    }

    pub fn new_with_timeout(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        timeout: Duration,
    ) -> Self {
        let rpc_client = RpcClient::new_with_timeout(rpc_addr, timeout);
        Self::new_from_client(rpc_addr, transactions_addr, transactions_socket, rpc_client)
    }

    fn new_from_client(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
        rpc_client: RpcClient,
    ) -> Self {
        ThinClient {
            rpc_client,
            rpc_addr,
            transactions_addr,
            transactions_socket,
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, transaction: &Transaction) -> io::Result<Signature> {
        let mut buf = vec![0; transaction.serialized_size().unwrap() as usize];
        let mut wr = std::io::Cursor::new(&mut buf[..]);
        serialize_into(&mut wr, &transaction)
            .expect("serialize Transaction in pub fn transfer_signed");
        assert!(buf.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&buf[..], &self.transactions_addr)?;
        Ok(transaction.signatures[0])
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer(
        &mut self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            transaction.sign(&[keypair], self.get_last_id());
            let mut buf = vec![0; transaction.serialized_size().unwrap() as usize];
            let mut wr = std::io::Cursor::new(&mut buf[..]);
            serialize_into(&mut wr, &transaction)
                .expect("serialize Transaction in pub fn transfer_signed");
            self.transactions_socket
                .send_to(&buf[..], &self.transactions_addr)?;
            if self.poll_for_signature(&transaction.signatures[0]).is_ok() {
                return Ok(transaction.signatures[0]);
            }
            info!("{} tries failed transfer to {}", x, self.transactions_addr);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "retry_transfer failed",
        ))
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        tokens: u64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        debug!(
            "transfer: tokens={} from={:?} to={:?} last_id={:?}",
            tokens,
            keypair.pubkey(),
            to,
            last_id
        );
        let now = Instant::now();
        let transaction = SystemTransaction::new_account(keypair, to, tokens, *last_id, 0);
        let result = self.transfer_signed(&transaction);
        solana_metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("transfer".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        result
    }

    pub fn get_account_userdata(&mut self, pubkey: &Pubkey) -> io::Result<Option<Vec<u8>>> {
        let params = json!([format!("{}", pubkey)]);
        let response =
            self.rpc_client
                .make_rpc_request(1, RpcRequest::GetAccountInfo, Some(params));
        match response {
            Ok(account_json) => {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                Ok(Some(account.userdata))
            }
            Err(error) => {
                debug!("get_account_userdata failed: {:?}", error);
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "get_account_userdata failed",
                ))
            }
        }
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &Pubkey) -> io::Result<u64> {
        trace!("get_balance sending request to {}", self.rpc_addr);
        let params = json!([format!("{}", pubkey)]);
        let response =
            self.rpc_client
                .make_rpc_request(1, RpcRequest::GetAccountInfo, Some(params));

        response
            .and_then(|account_json| {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                trace!("Response account {:?} {:?}", pubkey, account);
                trace!("get_balance {:?}", account.tokens);
                Ok(account.tokens)
            })
            .map_err(|error| {
                debug!("Response account {}: None (error: {:?})", pubkey, error);
                io::Error::new(io::ErrorKind::Other, "AccountNotFound")
            })
    }

    /// Request the confirmation time from the leader node
    pub fn get_confirmation_time(&mut self) -> usize {
        trace!("get_confirmation_time");
        loop {
            debug!("get_confirmation_time send_to {}", &self.rpc_addr);

            let response =
                self.rpc_client
                    .make_rpc_request(1, RpcRequest::GetConfirmationTime, None);

            match response {
                Ok(value) => {
                    let confirmation = value.as_u64().unwrap() as usize;
                    return confirmation;
                }
                Err(error) => {
                    debug!("thin_client get_confirmation_time error: {:?}", error);
                }
            };
        }
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will try again 5 times.
    pub fn transaction_count(&mut self) -> u64 {
        debug!("transaction_count");
        for _tries in 0..5 {
            let response =
                self.rpc_client
                    .make_rpc_request(1, RpcRequest::GetTransactionCount, None);

            match response {
                Ok(value) => {
                    debug!("transaction_count response: {:?}", value);
                    let transaction_count = value.as_u64().unwrap();
                    return transaction_count;
                }
                Err(error) => {
                    debug!("transaction_count failed: {:?}", error);
                }
            };
        }
        0
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> Hash {
        loop {
            trace!("get_last_id send_to {}", &self.rpc_addr);
            let response = self
                .rpc_client
                .make_rpc_request(1, RpcRequest::GetLastId, None);

            match response {
                Ok(value) => {
                    let last_id_str = value.as_str().unwrap();
                    let last_id_vec = bs58::decode(last_id_str).into_vec().unwrap();
                    return Hash::new(&last_id_vec);
                }
                Err(error) => {
                    debug!("thin_client get_last_id error: {:?}", error);
                }
            };
        }
    }

    /// Request a new last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_next_last_id(&mut self, previous_last_id: &Hash) -> Hash {
        loop {
            let last_id = self.get_last_id();
            if last_id != *previous_last_id {
                break last_id;
            }
            debug!("Got same last_id ({:?}), will retry...", last_id);
            sleep(Duration::from_millis(100));
        }
    }

    pub fn submit_poll_balance_metrics(elapsed: &Duration) {
        solana_metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("get_balance".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(elapsed) as i64),
                )
                .to_owned(),
        );
    }

    pub fn poll_balance_with_timeout(
        &mut self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        let now = Instant::now();
        loop {
            match self.get_balance(&pubkey) {
                Ok(bal) => {
                    ThinClient::submit_poll_balance_metrics(&now.elapsed());
                    return Ok(bal);
                }
                Err(e) => {
                    sleep(*polling_frequency);
                    if now.elapsed() > *timeout {
                        ThinClient::submit_poll_balance_metrics(&now.elapsed());
                        return Err(e);
                    }
                }
            };
        }
    }

    pub fn poll_get_balance(&mut self, pubkey: &Pubkey) -> io::Result<u64> {
        self.poll_balance_with_timeout(pubkey, &Duration::from_millis(100), &Duration::from_secs(1))
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&mut self, signature: &Signature) -> io::Result<()> {
        let now = Instant::now();
        while !self.check_signature(signature) {
            if now.elapsed().as_secs() > 1 {
                // TODO: Return a better error.
                return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
            }
            sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&mut self, signature: &Signature) -> bool {
        trace!("check_signature: {:?}", signature);
        let params = json!([format!("{}", signature)]);
        let now = Instant::now();

        loop {
            let response = self.rpc_client.make_rpc_request(
                1,
                RpcRequest::ConfirmTransaction,
                Some(params.clone()),
            );

            match response {
                Ok(confirmation) => {
                    let signature_status = confirmation.as_bool().unwrap();
                    if signature_status {
                        trace!("Response found signature");
                    } else {
                        trace!("Response signature not found");
                    }
                    solana_metrics::submit(
                        influxdb::Point::new("thinclient")
                            .add_tag("op", influxdb::Value::String("check_signature".to_string()))
                            .add_field(
                                "duration_ms",
                                influxdb::Value::Integer(
                                    timing::duration_as_ms(&now.elapsed()) as i64
                                ),
                            )
                            .to_owned(),
                    );
                    return signature_status;
                }
                Err(err) => {
                    debug!("check_signature request failed: {:?}", err);
                }
            };
        }
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        solana_metrics::flush();
    }
}

pub fn poll_gossip_for_leader(leader_gossip: SocketAddr, timeout: Option<u64>) -> Result<NodeInfo> {
    let exit = Arc::new(AtomicBool::new(false));
    let (node, gossip_socket) = ClusterInfo::spy_node();
    let my_addr = gossip_socket.local_addr().unwrap();
    let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(node)));
    let gossip_service =
        GossipService::new(&cluster_info.clone(), None, gossip_socket, exit.clone());

    let leader_entry_point = NodeInfo::new_entry_point(&leader_gossip);
    cluster_info
        .write()
        .unwrap()
        .insert_info(leader_entry_point);

    sleep(Duration::from_millis(100));

    let deadline = match timeout {
        Some(timeout) => Duration::new(timeout, 0),
        None => Duration::new(std::u64::MAX, 0),
    };
    let now = Instant::now();
    // Block until leader's correct contact info is received
    let leader;

    loop {
        trace!("polling {:?} for leader from {:?}", leader_gossip, my_addr);

        if let Some(l) = cluster_info.read().unwrap().get_gossip_top_leader() {
            leader = Some(l.clone());
            break;
        }

        if log_enabled!(Level::Trace) {
            trace!("{}", cluster_info.read().unwrap().node_info_trace());
        }

        if now.elapsed() > deadline {
            return Err(Error::ClusterInfoError(ClusterInfoError::NoLeader));
        }

        sleep(Duration::from_millis(100));
    }

    gossip_service.close()?;

    if log_enabled!(Level::Trace) {
        trace!("{}", cluster_info.read().unwrap().node_info_trace());
    }

    Ok(leader.unwrap().clone())
}

pub fn retry_get_balance(
    client: &mut ThinClient,
    bob_pubkey: &Pubkey,
    expected_balance: Option<u64>,
) -> Option<u64> {
    const LAST: usize = 30;
    for run in 0..LAST {
        let balance_result = client.poll_get_balance(bob_pubkey);
        if expected_balance.is_none() {
            return balance_result.ok();
        }
        trace!(
            "retry_get_balance[{}] {:?} {:?}",
            run,
            balance_result,
            expected_balance
        );
        if let (Some(expected_balance), Ok(balance_result)) = (expected_balance, balance_result) {
            if expected_balance == balance_result {
                return Some(balance_result);
            }
        }
    }
    None
}

pub fn new_fullnode(ledger_name: &'static str) -> (Fullnode, NodeInfo, Keypair, String) {
    use crate::blocktree::create_tmp_sample_ledger;
    use crate::cluster_info::Node;
    use crate::fullnode::Fullnode;
    use crate::voting_keypair::VotingKeypair;
    use solana_sdk::signature::KeypairUtil;

    let node_keypair = Arc::new(Keypair::new());
    let node = Node::new_localhost_with_pubkey(node_keypair.pubkey());
    let node_info = node.info.clone();

    let fullnode_config = &FullnodeConfig::default();
    let (mint_keypair, ledger_path, _tick_height, _last_entry_height, _last_id, _last_entry_id) =
        create_tmp_sample_ledger(
            ledger_name,
            10_000,
            0,
            node_info.id,
            42,
            fullnode_config.leader_scheduler_config.ticks_per_slot,
        );

    let vote_account_keypair = Arc::new(Keypair::new());
    let voting_keypair = VotingKeypair::new_local(&vote_account_keypair);
    let node = Fullnode::new(
        node,
        &node_keypair,
        &ledger_path,
        voting_keypair,
        None,
        &FullnodeConfig::default(),
    );

    (node, node_info, mint_keypair, ledger_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::mk_client;
    use bincode::{deserialize, serialize};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::vote_program::VoteState;
    use solana_sdk::vote_transaction::VoteTransaction;
    use std::fs::remove_dir_all;

    #[test]
    fn test_thin_client_basic() {
        solana_logger::setup();
        let (server, leader_data, alice, ledger_path) = new_fullnode("thin_client");
        let server_exit = server.run(None);
        let bob_pubkey = Keypair::new().pubkey();

        info!(
            "found leader: {:?}",
            poll_gossip_for_leader(leader_data.gossip, Some(5)).unwrap()
        );

        let mut client = mk_client(&leader_data);

        let transaction_count = client.transaction_count();
        assert_eq!(transaction_count, 0);

        let confirmation = client.get_confirmation_time();
        assert_eq!(confirmation, 18446744073709551615);

        let last_id = client.get_last_id();
        info!("test_thin_client last_id: {:?}", last_id);

        let signature = client.transfer(500, &alice, bob_pubkey, &last_id).unwrap();
        info!("test_thin_client signature: {:?}", signature);
        client.poll_for_signature(&signature).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);

        let transaction_count = client.transaction_count();
        assert_eq!(transaction_count, 1);
        server_exit();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    #[ignore]
    fn test_bad_sig() {
        solana_logger::setup();
        let (server, leader_data, alice, ledger_path) = new_fullnode("bad_sig");
        let server_exit = server.run(None);
        let bob_pubkey = Keypair::new().pubkey();
        info!(
            "found leader: {:?}",
            poll_gossip_for_leader(leader_data.gossip, Some(5)).unwrap()
        );

        let mut client = mk_client(&leader_data);

        let last_id = client.get_last_id();

        let tx = SystemTransaction::new_account(&alice, bob_pubkey, 500, last_id, 0);

        let _sig = client.transfer_signed(&tx).unwrap();

        let last_id = client.get_last_id();

        let mut tr2 = SystemTransaction::new_account(&alice, bob_pubkey, 501, last_id, 0);
        let mut instruction2 = deserialize(tr2.userdata(0)).unwrap();
        if let SystemInstruction::Move { ref mut tokens } = instruction2 {
            *tokens = 502;
        }
        tr2.instructions[0].userdata = serialize(&instruction2).unwrap();
        let signature = client.transfer_signed(&tr2).unwrap();
        client.poll_for_signature(&signature).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 1001);
        server_exit();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_register_vote_account() {
        solana_logger::setup();
        let (server, leader_data, alice, ledger_path) = new_fullnode("thin_client");
        let server_exit = server.run(None);
        info!(
            "found leader: {:?}",
            poll_gossip_for_leader(leader_data.gossip, Some(5)).unwrap()
        );

        let mut client = mk_client(&leader_data);

        // Create the validator account, transfer some tokens to that account
        let validator_keypair = Keypair::new();
        let last_id = client.get_last_id();
        let signature = client
            .transfer(500, &alice, validator_keypair.pubkey(), &last_id)
            .unwrap();

        client.poll_for_signature(&signature).unwrap();

        // Create and register the vote account
        let validator_vote_account_keypair = Keypair::new();
        let vote_account_id = validator_vote_account_keypair.pubkey();
        let last_id = client.get_last_id();

        let transaction =
            VoteTransaction::new_account(&validator_keypair, vote_account_id, last_id, 1, 1);
        let signature = client.transfer_signed(&transaction).unwrap();
        client.poll_for_signature(&signature).unwrap();

        let balance = retry_get_balance(&mut client, &vote_account_id, Some(1))
            .expect("Expected balance for new account to exist");
        assert_eq!(balance, 1);

        const LAST: usize = 30;
        for run in 0..=LAST {
            let account_user_data = client
                .get_account_userdata(&vote_account_id)
                .expect("Expected valid response for account userdata")
                .expect("Expected valid account userdata to exist after account creation");

            let vote_state = VoteState::deserialize(&account_user_data);

            if vote_state.map(|vote_state| vote_state.node_id) == Ok(validator_keypair.pubkey()) {
                break;
            }

            if run == LAST {
                panic!("Expected successful vote account registration");
            }
            sleep(Duration::from_millis(900));
        }

        server_exit();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_transaction_count() {
        // set a bogus address, see that we don't hang
        solana_logger::setup();
        let addr = "0.0.0.0:1234".parse().unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client =
            ThinClient::new_with_timeout(addr, addr, transactions_socket, Duration::from_secs(2));
        assert_eq!(client.transaction_count(), 0);
    }

    #[test]
    fn test_zero_balance_after_nonzero() {
        solana_logger::setup();
        let (server, leader_data, alice, ledger_path) = new_fullnode("thin_client");
        let server_exit = server.run(None);
        let bob_keypair = Keypair::new();

        info!(
            "found leader: {:?}",
            poll_gossip_for_leader(leader_data.gossip, Some(5)).unwrap()
        );

        let mut client = mk_client(&leader_data);
        let last_id = client.get_last_id();

        let starting_balance = client.poll_get_balance(&alice.pubkey()).unwrap();

        info!("Give Bob 500 tokens");
        let signature = client
            .transfer(500, &alice, bob_keypair.pubkey(), &last_id)
            .unwrap();
        client.poll_for_signature(&signature).unwrap();

        let balance = client.poll_get_balance(&bob_keypair.pubkey());
        assert_eq!(balance.unwrap(), 500);

        info!("Take Bob's 500 tokens away");
        let signature = client
            .transfer(500, &bob_keypair, alice.pubkey(), &last_id)
            .unwrap();
        client.poll_for_signature(&signature).unwrap();
        let balance = client.poll_get_balance(&alice.pubkey()).unwrap();
        assert_eq!(balance, starting_balance);

        info!("Should get an error when Bob's balance hits zero and is purged");
        let balance = client.poll_get_balance(&bob_keypair.pubkey());
        assert!(balance.is_err());

        server_exit();
        remove_dir_all(ledger_path).unwrap();
    }
}
