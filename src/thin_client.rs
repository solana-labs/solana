//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bank::VersionedAccount;
use bincode::{deserialize, serialize};
use request::{Request, Response};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::{LastId, Transaction};

use influx_db_client as influxdb;
use metrics;

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    requests_addr: SocketAddr,
    requests_socket: UdpSocket,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<LastId>,
    transaction_count: u64,
    balances: HashMap<PublicKey, VersionedAccount>,
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
            Response::Balance { key, val, height } => {
                trace!("Response balance {:?} {:?} {}", key, val, height);
                let v = VersionedAccount {
                    tokens: val,
                    version: height,
                };
                self.balances.insert(key, v);
            }
            Response::LastId { ref id } => {
                trace!("Response last_id {:?}", id);
                self.last_id = Some(id.clone());
            }
            Response::TransactionCount { transaction_count } => {
                trace!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tx: &Transaction) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)?;
        Ok(tx.sig)
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: LastId,
    ) -> io::Result<Signature> {
        let now = Instant::now();
        let tx = Transaction::new(keypair, to, n, last_id);
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

    pub fn get_versioned_account(&mut self, pubkey: &PublicKey) -> io::Result<VersionedAccount> {
        trace!("get_versioned_account");
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
    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        let v = self.get_versioned_account(pubkey)?;
        Ok(v.tokens)
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
    pub fn get_last_id(&mut self) -> LastId {
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
        self.last_id.clone().expect("some last_id")
    }

    /// poll until the account version is greater than the current version for millies
    pub fn poll_update(
        &mut self,
        millies: u64,
        pubkey: &PublicKey,
    ) -> io::Result<VersionedAccount> {
        let now = Instant::now();
        // version 0 is the starting value, so poll for something after 0
        let version = self.balances.get(pubkey).map(|v| v.version).unwrap_or(1);
        loop {
            if let Ok(bal) = self.get_versioned_account(pubkey) {
                if bal.version > version {
                    return Ok(bal);
                }
            }
            let elapsed = now.elapsed();
            let elapsed_ms = elapsed.as_secs() * 1_000_000u64 + (elapsed.subsec_millis() as u64);
            if elapsed_ms > millies {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("poll_update".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_as_ms(&now.elapsed()) as i64),
                )
                .to_owned(),
        );
        Err(io::Error::new(io::ErrorKind::Other, "poll_update timeout"))
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
    use ledger::LedgerWriter;
    use logger;
    use mint::Mint;
    use service::Service;
    use signature::{KeyPair, KeyPairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use transaction::{Instruction, Plan};

    fn tmp_ledger(name: &str, mint: &Mint) -> String {
        let keypair = KeyPair::new();

        let path = format!("/tmp/tmp-ledger-{}-{}", name, keypair.pubkey());

        let mut writer = LedgerWriter::new(&path, true).unwrap();
        writer.write_entries(mint.create_entries()).unwrap();

        path
    }

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
        let ledger_path = tmp_ledger("thin_client", &alice);

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

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );
        let last_id = client.get_last_id();
        let sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        client.poll_for_signature(&sig).unwrap();
        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    // sleep(Duration::from_millis(300)); is unstable
    #[test]
    #[ignore]
    fn test_bad_sig() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let leader_data = leader.data.clone();
        let ledger_path = tmp_ledger("bad_sig", &alice);

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
        //TODO: remove this sleep, or add a retry so CI is stable
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
        let last_id = client.get_last_id();

        let tx = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = client.transfer_signed(&tx).unwrap();

        let last_id = client.get_last_id();

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id);
        if let Instruction::NewContract(contract) = &mut tr2.instruction {
            contract.tokens = 502;
            contract.plan = Plan::Budget(Budget::new_payment(502, bob_pubkey));
        }
        let sig = client.transfer_signed(&tr2).unwrap();
        client.poll_for_signature(&sig).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_client_check_signature() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let leader_data = leader.data.clone();
        let ledger_path = tmp_ledger("client_check_signature", &alice);

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
        let last_id = client.get_last_id();
        let sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        sleep(Duration::from_millis(100));

        assert!(client.check_signature(&sig));

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
}
