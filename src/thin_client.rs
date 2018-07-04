//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use hash::Hash;
use request::{Request, Response};
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

use influx_db_client as influxdb;
use metrics;

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    requests_addr: SocketAddr,
    requests_socket: UdpSocket,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<PublicKey, i64>,
    pubkey_version: u64,
    pubkey_signature: Signature,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with Rpu
    /// over `requests_socket` and `transactions_socket`. To receive responses, the caller must bind `socket`
    /// to a public address before invoking ThinClient methods.
    pub fn new(
        requests_addr: SocketAddr,
        requests_socket: UdpSocket,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
    ) -> Self {
        ThinClient {
            requests_addr,
            requests_socket,
            transactions_addr,
            transactions_socket,
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
            pubkey_version: 0,
            pubkey_signature: Signature::default(),
        }
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        trace!("start recv_from");
        self.requests_socket.recv_from(&mut buf)?;
        trace!("end recv_from");
        deserialize(&buf).or_else(|_| Err(io::Error::new(io::ErrorKind::Other, "deserialize")))
    }

    pub fn process_response(&mut self, resp: &Response) {
        match *resp {
            Response::Balance { key, val } => {
                trace!("Response balance {:?} {:?}", key, val);
                self.balances.insert(key, val);
            }
            Response::LastId { id } => {
                trace!("Response last_id {:?}", id);
                self.last_id = Some(id);
            }
            Response::TransactionCount { transaction_count } => {
                trace!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
            Response::PubKeyVersion { version, signature } => {
                if self.pubkey_version == version {
                    trace!("Response signature not found");
                } else {
                    self.pubkey_version = version;
                    self.pubkey_signature = signature;
                    trace!("Response found signature");
                }
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tx: &Transaction) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)?;
        Ok(*tx.sig())
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: &Hash,
        version: u64,
    ) -> io::Result<Signature> {
        let now = Instant::now();
        let tx = Transaction::new(keypair, to, n, *last_id, version);
        let result = self.transfer_signed(&tx);
        metrics::submit(
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

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        trace!("get_balance");
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance in pub fn get_balance");
        self.requests_socket
            .send_to(&data, &self.requests_addr)
            .expect("buffer error in pub fn get_balance");
        let mut done = false;
        while !done {
            let resp = self.recv_response()?;
            trace!("recv_response {:?}", resp);
            if let Response::Balance { key, .. } = &resp {
                done = key == pubkey;
            }
            self.process_response(&resp);
        }
        self.balances
            .get(pubkey)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "nokey"))
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will hang.
    pub fn transaction_count(&mut self) -> u64 {
        debug!("transaction_count");
        let req = Request::GetTransactionCount;
        let data =
            serialize(&req).expect("serialize GetTransactionCount in pub fn transaction_count");
        let mut done = false;
        while !done {
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn transaction_count");

            if let Ok(resp) = self.recv_response() {
                debug!("transaction_count recv_response: {:?}", resp);
                if let Response::TransactionCount { .. } = resp {
                    done = true;
                }
                self.process_response(&resp);
            }
        }
        self.transaction_count
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> Hash {
        trace!("get_last_id");
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetLastId in pub fn get_last_id");
        let mut done = false;
        while !done {
            debug!("get_last_id send_to {}", &self.requests_addr);
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_last_id");

            match self.recv_response() {
                Ok(resp) => {
                    if let Response::LastId { .. } = resp {
                        done = true;
                    }
                    self.process_response(&resp);
                }
                Err(e) => {
                    debug!("thin_client get_last_id error: {}", e);
                }
            }
        }
        self.last_id.expect("some last_id")
    }

    pub fn poll_get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        let mut balance_result;
        let mut balance_value = -1;
        let now = Instant::now();
        loop {
            balance_result = self.get_balance(pubkey);
            if balance_result.is_ok() {
                balance_value = *balance_result.as_ref().unwrap();
            }
            if balance_value > 0 || now.elapsed().as_secs() > 1 {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("get_balance".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        if balance_value >= 0 {
            Ok(balance_value)
        } else {
            assert!(balance_result.is_err());
            balance_result
        }
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn get_version(&mut self, pubkey: &PublicKey) -> (u64, Signature) {
        trace!("check_signature");
        let req = Request::GetPubKeyVersion { pubkey: *pubkey };
        let data = serialize(&req).expect("serialize GetPubKeyVersion in pub fn get_version");
        let now = Instant::now();
        let mut done = false;
        while !done {
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_last_id");

            if let Ok(resp) = self.recv_response() {
                if let Response::PubKeyVersion { .. } = resp {
                    done = true;
                }
                self.process_response(&resp);
            }
        }
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("check_signature".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        (self.pubkey_version, self.pubkey_signature)
    }
    /// Retry the transfer until it succeeds
    /// To retry correctly
    /// 1. client gets the current version of the sender
    /// 2. client generates a transfer transaction for that version
    /// 3. client check the updated version of the sender
    /// 4. If the updated version didn't change, retry the transfer at current version
    /// 5. If the updated version changed but signature doesn't match, retry the transfer at new
    ///    version
    /// 6. If the updated version changed and signature matches, return
    /// * returns the destination key's balance
    pub fn retry_transfer(
        &mut self,
        alice: &KeyPair,
        bob_pubkey: &PublicKey,
        amount: i64,
        retries: usize,
    ) -> Option<(u64, Signature)> {
        let last_id = self.get_last_id();
        let mut version = self.get_version(&alice.pubkey());
        for _ in 0..retries {
            let sig = self.transfer(amount, &alice, *bob_pubkey, &last_id, version.0)
                .unwrap();
            let next_version = self.get_version(&alice.pubkey());
            if next_version.1 == sig {
                return Some(next_version);
            } else if version.0 != next_version.0 {
                version = next_version;
            }
            sleep(Duration::from_millis(100));
        }
        None
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        metrics::flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use budget::Budget;
    use crdt::TestNode;
    use fullnode::FullNode;
    use logger;
    use mint::Mint;
    use service::Service;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use transaction::{Instruction, Plan};

    #[test]
    fn test_thin_client() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.data.clone();

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = FullNode::new_leader(
            leader_keypair,
            bank,
            0,
            None,
            Some(Duration::from_millis(30)),
            leader,
            exit.clone(),
            sink(),
            false,
        );

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );
        let last_id = client.get_last_id();
        let version = client.get_version(&alice.keypair().pubkey());
        for _ in 0..30 {
            let sig = client
                .transfer(500, &alice.keypair(), bob_pubkey, &last_id, version.0)
                .unwrap();
            if sig == client.get_version(&alice.keypair().pubkey()).1 {
                break;
            }
            sleep(Duration::from_millis(300));
        }
        let balance = client.poll_get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn test_bad_sig() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let leader_data = leader.data.clone();

        let server = FullNode::new_leader(
            leader_keypair,
            bank,
            0,
            None,
            Some(Duration::from_millis(30)),
            leader,
            exit.clone(),
            sink(),
            false,
        );

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        requests_socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );
        let last_id = client.get_last_id();
        let version = client.get_version(&alice.keypair().pubkey());
        let tx = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id, version.0);
        for _ in 0..30 {
            let _ = client.transfer_signed(&tx).unwrap();
            if *tx.sig() == client.get_version(&alice.keypair().pubkey()).1 {
                break;
            }
            sleep(Duration::from_millis(300));
        }

        let last_id = client.get_last_id();
        let version = client.get_version(&alice.keypair().pubkey());

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id, version.0);
        if let Instruction::NewContract(contract) = tr2.instruction() {
            let mut contract = contract.clone();
            contract.tokens = 502;
            contract.plan = Plan::Budget(Budget::new_payment(502, bob_pubkey));
            tr2.call.data.user_data = serialize(&Instruction::NewContract(contract)).unwrap();
        }
        for _ in 0..30 {
            let _sig = client.transfer_signed(&tr2).unwrap();
            if version.0 != client.get_version(&alice.keypair().pubkey()).0 {
                break;
            }
            sleep(Duration::from_millis(300));
        }
        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn test_client_retry_transfer() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let leader_data = leader.data.clone();
        let server = FullNode::new_leader(
            leader_keypair,
            bank,
            0,
            None,
            Some(Duration::from_millis(30)),
            leader,
            exit.clone(),
            sink(),
            false,
        );
        sleep(Duration::from_millis(300));

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        requests_socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );
        let result = client.retry_transfer(&alice.keypair(), &bob_pubkey, 500, 30);
        assert!(result.is_some());

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
