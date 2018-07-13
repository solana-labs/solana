//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use budget::Condition;
use chrono::prelude::Utc;
use hash::Hash;
use influx_db_client as influxdb;
use metrics;
use payment_plan::Payment;
use request::{Request, Response};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::{Contract, Transaction, FEE_PER_INSTRUCTION};

/// An object for querying and sending transactions to the network.
pub struct ThinClient {
    requests_addr: SocketAddr,
    requests_socket: UdpSocket,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<PublicKey, i64>,
    signature_status: bool,
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

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&mut self, sig: &Signature) -> io::Result<()> {
        let now = Instant::now();
        while !self.check_signature(sig) {
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
    pub fn check_signature(&mut self, sig: &Signature) -> bool {
        trace!("check_signature");
        let req = Request::GetSignature { signature: *sig };
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

    pub fn retry_get_balance(
        &mut self,
        bob_pubkey: &PublicKey,
        expected: Option<i64>,
    ) -> Option<i64> {
        const LAST: usize = 20;
        for run in 0..(LAST + 1) {
            let out = self.poll_get_balance(bob_pubkey);
            if expected.is_none() || run == LAST {
                return out.ok().clone();
            }
            trace!("retry_get_balance[{}] {:?} {:?}", run, out, expected);
            if let (Some(e), Ok(o)) = (expected, out) {
                if o == e {
                    return Some(o);
                }
            }
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

        let mut alice = Mint::new(10_000);
        let bank = Bank::new(&mut alice);
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
        let mut alice = Mint::new(10_000);
        let bank = Bank::new(&mut alice);
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
        if let Instruction::NewContract(contract) = &mut tr2.instructions[0] {
            contract.tokens = 502;
            contract.plan = Plan::Budget(Budget::new_payment(502, bob_pubkey));
        }
        let sig = client.transfer_signed(&tr2).unwrap();
        client.poll_for_signature(&sig).unwrap();

        let balance = client.get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn test_client_check_signature() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
        let mut alice = Mint::new(10_000);
        let bank = Bank::new(&mut alice);
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
        let last_id = client.get_last_id();
        let sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        sleep(Duration::from_millis(100));

        assert!(client.check_signature(&sig));

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn test_multiple_transfers() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost();
        let mut alice = Mint::new(10_000);
        let bank = Bank::new(&mut alice);
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

        // Make new contract instructions
        let initial_transfer_value: i64 = 500;
        let num_contracts: usize = 10;
        let mut expected_balance: i64 = 0;

        // Create multiple transfer instructions
        let mut multi_instructions = Vec::new();

        for i in 0..num_contracts {
            let transfer_value = initial_transfer_value + i as i64;
            expected_balance += transfer_value;
            let payment = Payment {
                tokens: transfer_value as i64,
                to: bob_pubkey,
            };
            let budget = Budget::Pay(payment);
            let plan = Plan::Budget(budget);
            let contract = Contract::new(transfer_value, plan);
            let instruction = Instruction::NewContract(contract);
            multi_instructions.push(instruction);
        }

        let final_transaction = Transaction::new_from_instructions(
            &alice.keypair(),
            multi_instructions,
            last_id,
            (num_contracts * FEE_PER_INSTRUCTION) as i64,
        );

        let _sig = client.transfer_signed(&final_transaction).unwrap();

        let balance = client
            .retry_get_balance(&bob_pubkey, Some(expected_balance))
            .unwrap();

        assert_eq!(balance, expected_balance);

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn test_contract_fulfillment() {
        logger::setup();
        let leader_keypair = KeyPair::new();
        let leader = TestNode::new_localhost();
        let mut alice = Mint::new(10_000);
        let bank = Bank::new(&mut alice);
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

        // Set up all the contracts
        let initial_transfer_value = 100;
        let num_signature_contracts = 10;
        let num_timestamp_contracts = 10;
        let num_contracts = num_signature_contracts + num_timestamp_contracts;
        let mut contract_ids = Vec::new();
        let mut contract_instructions = Vec::new();
        let mut expected_balance = 0;

        // Make contracts that require timestamps
        for i in 0..num_timestamp_contracts {
            let transfer_value = initial_transfer_value + i as i64;
            expected_balance += transfer_value;

            let date_condition = (
                Condition::Timestamp(Utc::now(), alice.pubkey()),
                Payment {
                    tokens: transfer_value,
                    to: bob_pubkey,
                },
            );

            let budget = Budget::Or(date_condition.clone(), date_condition);
            let plan = Plan::Budget(budget);
            let contract = Contract::new(transfer_value, plan);
            contract_instructions.push(Instruction::NewContract(contract));
        }

        // Make contracts that need a signature
        for i in 0..num_signature_contracts {
            let transfer_value = initial_transfer_value + i as i64;
            expected_balance += transfer_value;

            let budget = Budget::new_authorized_payment(alice.pubkey(), transfer_value, bob_pubkey);

            let plan = Plan::Budget(budget);
            let contract = Contract::new(transfer_value, plan);
            contract_ids.push(contract.id);
            contract_instructions.push(Instruction::NewContract(contract));
        }

        let contract_transaction = Transaction::new_from_instructions(
            &alice.keypair(),
            contract_instructions,
            last_id,
            (num_contracts * FEE_PER_INSTRUCTION) as i64,
        );

        let _sig = client.transfer_signed(&contract_transaction).unwrap();

        // Create instructions to fulfill all the above contracts
        let mut fulfill_instructions: Vec<Instruction> = contract_ids
            .iter()
            .map(|id| Instruction::ApplySignature(*id))
            .collect();

        let fulfill_timestamp_instruction = Instruction::ApplyTimestamp(Utc::now());
        fulfill_instructions.push(fulfill_timestamp_instruction);

        let num_instructions = fulfill_instructions.len();

        let final_transaction = Transaction::new_from_instructions(
            &alice.keypair(),
            fulfill_instructions,
            last_id,
            (num_instructions * FEE_PER_INSTRUCTION) as i64,
        );

        let _sig = client.transfer_signed(&final_transaction).unwrap();
        let balance = client
            .retry_get_balance(&bob_pubkey, Some(expected_balance))
            .unwrap();

        assert_eq!(balance, expected_balance);

        exit.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
