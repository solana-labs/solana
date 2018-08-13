//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use hash::Hash;
use request::{Request, Response};
use signature::{Keypair, Pubkey, Signature};
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
    balances: HashMap<Pubkey, i64>,
    signature_status: bool,
    finality: Option<usize>,
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
            signature_status: false,
            finality: None,
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
            Response::SignatureStatus { signature_status } => {
                self.signature_status = signature_status;
                if signature_status {
                    trace!("Response found signature");
                } else {
                    trace!("Response signature not found");
                }
            }
            Response::Finality { time } => {
                trace!("Response finality {:?}", time);
                self.finality = Some(time);
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tx: &Transaction) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)?;
        Ok(tx.signature)
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let now = Instant::now();
        let tx = Transaction::new(keypair, to, n, *last_id);
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
    pub fn get_balance(&mut self, pubkey: &Pubkey) -> io::Result<i64> {
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

    /// Request the finality from the leader node
    pub fn get_finality(&mut self) -> usize {
        trace!("get_finality");
        let req = Request::GetFinality;
        let data = serialize(&req).expect("serialize GetFinality in pub fn get_finality");
        let mut done = false;
        while !done {
            debug!("get_finality send_to {}", &self.requests_addr);
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_finality");

            match self.recv_response() {
                Ok(resp) => {
                    if let Response::Finality { .. } = resp {
                        done = true;
                    }
                    self.process_response(&resp);
                }
                Err(e) => {
                    debug!("thin_client get_finality error: {}", e);
                }
            }
        }
        self.finality.expect("some finality")
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

    pub fn poll_get_balance(&mut self, pubkey: &Pubkey) -> io::Result<i64> {
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
        let req = Request::GetSignature {
            signature: *signature,
        };
        let data = serialize(&req).expect("serialize GetSignature in pub fn check_signature");
        let now = Instant::now();
        let mut done = false;
        while !done {
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_last_id");

            if let Ok(resp) = self.recv_response() {
                if let Response::SignatureStatus { .. } = resp {
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
        self.signature_status
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
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use transaction::{Instruction, Plan};

    fn tmp_ledger(name: &str, mint: &Mint) -> String {
        use std::env;
        let out_dir = env::var("OUT_DIR").unwrap();
        let keypair = Keypair::new();

        let path = format!("{}/tmp-ledger-{}-{}", out_dir, name, keypair.pubkey());

        let mut writer = LedgerWriter::open(&path, true).unwrap();
        writer.write_entries(mint.create_entries()).unwrap();

        path
    }

    #[test]
    fn test_thin_client() {
        logger::setup();
        let leader_keypair = Keypair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.data.clone();

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
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
        let signature = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        client.poll_for_signature(&signature).unwrap();
        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    // sleep(Duration::from_millis(300)); is unstable
    #[test]
    #[ignore]
    fn test_bad_sig() {
        logger::setup();
        let leader_keypair = Keypair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
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
        let signature = client.transfer_signed(&tr2).unwrap();
        client.poll_for_signature(&signature).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_client_check_signature() {
        logger::setup();
        let leader_keypair = Keypair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
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
        let signature = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        sleep(Duration::from_millis(100));

        assert!(client.check_signature(&signature));

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
}
