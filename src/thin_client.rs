//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bank::Bank;
use bincode::serialize;
use bs58;
use cluster_info::{ClusterInfo, ClusterInfoError, NodeInfo};
use hash::Hash;
use log::Level;
use ncp::Ncp;
use packet::PACKET_DATA_SIZE;
use result::{Error, Result};
use rpc_request::RpcRequest;
use serde_json;
use signature::{Keypair, Signature};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use system_transaction::SystemTransaction;
use timing;
use transaction::Transaction;

use influx_db_client as influxdb;
use metrics;

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    rpc_addr: SocketAddr,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<Pubkey, Account>,
    signature_status: bool,
    finality: Option<usize>,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `transactions_addr` over `transactions_socket` using UDP.
    pub fn new(
        rpc_addr: SocketAddr,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
    ) -> Self {
        ThinClient {
            rpc_addr,
            transactions_addr,
            transactions_socket,
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
            signature_status: false,
            finality: None,
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tx: &Transaction) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        assert!(data.len() < PACKET_DATA_SIZE);
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)?;
        Ok(tx.signatures[0])
    }

    /// Retry a sending a signed Transaction to the server for processing.
    pub fn retry_transfer(
        &mut self,
        keypair: &Keypair,
        tx: &mut Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        for x in 0..tries {
            tx.sign(&[&keypair], self.get_last_id());
            let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
            self.transactions_socket
                .send_to(&data, &self.transactions_addr)?;
            if self.poll_for_signature(&tx.signatures[0]).is_ok() {
                return Ok(tx.signatures[0]);
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
        n: u64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let now = Instant::now();
        let tx = Transaction::system_new(keypair, to, n, *last_id);
        let result = self.transfer_signed(&tx);
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("transfer".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                ).to_owned(),
        );
        result
    }

    pub fn get_account_userdata(&mut self, pubkey: &Pubkey) -> io::Result<Option<Vec<u8>>> {
        let params = json!([format!("{}", pubkey)]);
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        let resp = RpcRequest::GetAccountInfo.make_rpc_request(&rpc_string, 1, Some(params));
        if let Ok(account_json) = resp {
            let account: Account =
                serde_json::from_value(account_json).expect("deserialize account");
            return Ok(Some(account.userdata));
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "get_account_userdata failed",
        ))
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &Pubkey) -> io::Result<u64> {
        trace!("get_balance sending request to {}", self.rpc_addr);
        let params = json!([format!("{}", pubkey)]);
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        let resp = RpcRequest::GetAccountInfo.make_rpc_request(&rpc_string, 1, Some(params));
        if let Ok(account_json) = resp {
            let account: Account =
                serde_json::from_value(account_json).expect("deserialize account");
            trace!("Response account {:?} {:?}", pubkey, account);
            self.balances.insert(*pubkey, account.clone());
        } else {
            debug!("Response account {}: None ", pubkey);
            self.balances.remove(&pubkey);
        }
        trace!("get_balance {:?}", self.balances.get(pubkey));
        // TODO: This is a hard coded call to introspect the balance of a budget_dsl contract
        // In the future custom contracts would need their own introspection
        self.balances
            .get(pubkey)
            .map(Bank::read_balance)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "AccountNotFound"))
    }

    /// Request the finality from the leader node
    pub fn get_finality(&mut self) -> usize {
        trace!("get_finality");
        let mut done = false;
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        while !done {
            debug!("get_finality send_to {}", &self.rpc_addr);
            let resp = RpcRequest::GetFinality.make_rpc_request(&rpc_string, 1, None);

            if let Ok(value) = resp {
                done = true;
                let finality = value.as_u64().unwrap() as usize;
                self.finality = Some(finality);
            } else {
                debug!("thin_client get_finality error: {:?}", resp);
            }
        }
        self.finality.expect("some finality")
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will try again 5 times.
    pub fn transaction_count(&mut self) -> u64 {
        debug!("transaction_count");
        let mut tries_left = 5;
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        while tries_left > 0 {
            let resp = RpcRequest::GetTransactionCount.make_rpc_request(&rpc_string, 1, None);

            if let Ok(value) = resp {
                debug!("transaction_count recv_response: {:?}", value);
                tries_left = 0;
                let transaction_count = value.as_u64().unwrap();
                self.transaction_count = transaction_count;
            } else {
                tries_left -= 1;
            }
        }
        self.transaction_count
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> Hash {
        trace!("get_last_id");
        let mut done = false;
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        while !done {
            debug!("get_last_id send_to {}", &self.rpc_addr);
            let resp = RpcRequest::GetLastId.make_rpc_request(&rpc_string, 1, None);

            if let Ok(value) = resp {
                done = true;
                let last_id_str = value.as_str().unwrap();
                let last_id_vec = bs58::decode(last_id_str).into_vec().unwrap();
                let last_id = Hash::new(&last_id_vec);
                self.last_id = Some(last_id);
            } else {
                debug!("thin_client get_last_id error: {:?}", resp);
            }
        }
        self.last_id.expect("some last_id")
    }

    pub fn submit_poll_balance_metrics(elapsed: &Duration) {
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("get_balance".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(elapsed) as i64),
                ).to_owned(),
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
        trace!("check_signature");
        let params = json!([format!("{}", signature)]);
        let now = Instant::now();
        let rpc_string = format!("http://{}", self.rpc_addr.to_string());
        let mut done = false;
        while !done {
            let resp = RpcRequest::ConfirmTransaction.make_rpc_request(
                &rpc_string,
                1,
                Some(params.clone()),
            );

            if let Ok(confirmation) = resp {
                done = true;
                self.signature_status = confirmation.as_bool().unwrap();
                if self.signature_status {
                    trace!("Response found signature");
                } else {
                    trace!("Response signature not found");
                }
            }
        }
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("check_signature".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                ).to_owned(),
        );
        self.signature_status
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        metrics::flush();
    }
}

pub fn poll_gossip_for_leader(leader_ncp: SocketAddr, timeout: Option<u64>) -> Result<NodeInfo> {
    let exit = Arc::new(AtomicBool::new(false));
    let (node, gossip_socket) = ClusterInfo::spy_node();
    let my_addr = gossip_socket.local_addr().unwrap();
    let cluster_info = Arc::new(RwLock::new(
        ClusterInfo::new(node).expect("ClusterInfo::new"),
    ));
    let window = Arc::new(RwLock::new(vec![]));
    let ncp = Ncp::new(
        &cluster_info.clone(),
        window,
        None,
        gossip_socket,
        exit.clone(),
    );

    let leader_entry_point = NodeInfo::new_entry_point(&leader_ncp);
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
        trace!("polling {:?} for leader from {:?}", leader_ncp, my_addr);

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

    ncp.close()?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use bincode::deserialize;
    use cluster_info::Node;
    use fullnode::Fullnode;
    use leader_scheduler::LeaderScheduler;
    use ledger::create_tmp_ledger_with_mint;
    use logger;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use system_program::SystemProgram;
    use vote_program::VoteProgram;
    use vote_transaction::VoteTransaction;

    #[test]
    fn test_thin_client() {
        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();

        let alice = Mint::new(10_000);
        let mut bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let ledger_path = create_tmp_ledger_with_mint("thin_client", &alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);
        let transaction_count = client.transaction_count();
        assert_eq!(transaction_count, 0);
        let finality = client.get_finality();
        assert_eq!(finality, 18446744073709551615);
        let last_id = client.get_last_id();
        let signature = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        client.poll_for_signature(&signature).unwrap();
        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        let transaction_count = client.transaction_count();
        assert_eq!(transaction_count, 1);
        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    // sleep(Duration::from_millis(300)); is unstable
    #[test]
    #[ignore]
    fn test_bad_sig() {
        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let mut bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = create_tmp_ledger_with_mint("bad_sig", &alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        //TODO: remove this sleep, or add a retry so CI is stable
        sleep(Duration::from_millis(300));

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);
        let last_id = client.get_last_id();

        let tx = Transaction::system_new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = client.transfer_signed(&tx).unwrap();

        let last_id = client.get_last_id();

        let mut tr2 = Transaction::system_new(&alice.keypair(), bob_pubkey, 501, last_id);
        let mut instruction2 = deserialize(tr2.userdata(0)).unwrap();
        if let SystemProgram::Move { ref mut tokens } = instruction2 {
            *tokens = 502;
        }
        tr2.instructions[0].userdata = serialize(&instruction2).unwrap();
        let signature = client.transfer_signed(&tr2).unwrap();
        client.poll_for_signature(&signature).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_client_check_signature() {
        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let mut bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = create_tmp_ledger_with_mint("client_check_signature", &alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let entry_height = alice.create_entries().len() as u64;
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            entry_height,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(300));

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);
        let last_id = client.get_last_id();
        let signature = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();

        assert!(client.poll_for_signature(&signature).is_ok());

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_register_vote_account() {
        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let mint = Mint::new(10_000);
        let mut bank = Bank::new(&mint);
        let leader_data = leader.info.clone();
        let ledger_path = create_tmp_ledger_with_mint("client_check_signature", &mint);

        let genesis_entries = &mint.create_entries();
        let entry_height = genesis_entries.len() as u64;

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let leader_vote_account_keypair = Arc::new(Keypair::new());
        let server = Fullnode::new_with_bank(
            leader_keypair,
            leader_vote_account_keypair.clone(),
            bank,
            entry_height,
            &genesis_entries.last().unwrap().id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(300));

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);

        // Create the validator account, transfer some tokens to that account
        let validator_keypair = Keypair::new();
        let last_id = client.get_last_id();
        let signature = client
            .transfer(500, &mint.keypair(), validator_keypair.pubkey(), &last_id)
            .unwrap();

        assert!(client.poll_for_signature(&signature).is_ok());

        // Create the vote account
        let validator_vote_account_keypair = Keypair::new();
        let vote_account_id = validator_vote_account_keypair.pubkey();
        let last_id = client.get_last_id();

        let transaction =
            VoteTransaction::vote_account_new(&validator_keypair, vote_account_id, last_id, 1);
        let signature = client.transfer_signed(&transaction).unwrap();
        assert!(client.poll_for_signature(&signature).is_ok());

        let balance = retry_get_balance(&mut client, &vote_account_id, Some(1))
            .expect("Expected balance for new account to exist");
        assert_eq!(balance, 1);

        // Register the vote account to the validator
        let last_id = client.get_last_id();
        let transaction =
            VoteTransaction::vote_account_register(&validator_keypair, vote_account_id, last_id, 0);
        let signature = client.transfer_signed(&transaction).unwrap();
        assert!(client.poll_for_signature(&signature).is_ok());

        const LAST: usize = 30;
        for run in 0..=LAST {
            println!("Checking for account registered: {}", run);
            let account_user_data = client
                .get_account_userdata(&vote_account_id)
                .expect("Expected valid response for account userdata")
                .expect("Expected valid account userdata to exist after account creation");

            let vote_state = VoteProgram::deserialize(&account_user_data);

            if vote_state.map(|vote_state| vote_state.node_id) == Ok(validator_keypair.pubkey()) {
                break;
            }

            if run == LAST {
                panic!("Expected successful vote account registration");
            }
            sleep(Duration::from_millis(900));
        }

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_transaction_count() {
        // set a bogus address, see that we don't hang
        logger::setup();
        let addr = "0.0.0.0:1234".parse().unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(addr, addr, transactions_socket);
        assert_eq!(client.transaction_count(), 0);
    }

    #[test]
    fn test_zero_balance_after_nonzero() {
        logger::setup();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let mut bank = Bank::new(&alice);
        let bob_keypair = Keypair::new();
        let leader_data = leader.info.clone();
        let ledger_path = create_tmp_ledger_with_mint("zero_balance_check", &alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let entry_height = alice.create_entries().len() as u64;
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            entry_height,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(leader_data.rpc, leader_data.tpu, transactions_socket);
        let last_id = client.get_last_id();

        // give bob 500 tokens
        let signature = client
            .transfer(500, &alice.keypair(), bob_keypair.pubkey(), &last_id)
            .unwrap();
        assert!(client.poll_for_signature(&signature).is_ok());

        let balance = client.poll_get_balance(&bob_keypair.pubkey());
        assert!(balance.is_ok());
        assert_eq!(balance.unwrap(), 500);

        // take them away
        let signature = client
            .transfer(500, &bob_keypair, alice.keypair().pubkey(), &last_id)
            .unwrap();
        assert!(client.poll_for_signature(&signature).is_ok());

        // should get an error when bob's account is purged
        let balance = client.poll_get_balance(&bob_keypair.pubkey());
        assert!(balance.is_err());

        server
            .close()
            .unwrap_or_else(|e| panic!("close() failed! {:?}", e));
        remove_dir_all(ledger_path).unwrap();
    }
}
