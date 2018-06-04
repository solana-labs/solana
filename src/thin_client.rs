//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use hash::Hash;
use request::{Request, Response};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use transaction::Transaction;

pub struct ThinClient {
    requests_addr: SocketAddr,
    requests_socket: UdpSocket,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<PublicKey, Option<i64>>,
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
        let client = ThinClient {
            requests_addr,
            requests_socket,
            transactions_addr,
            transactions_socket,
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
        };
        client
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        trace!("start recv_from");
        self.requests_socket.recv_from(&mut buf)?;
        trace!("end recv_from");
        let resp = deserialize(&buf).expect("deserialize balance in thin_client");
        Ok(resp)
    }

    pub fn process_response(&mut self, resp: Response) {
        match resp {
            Response::Balance { key, val } => {
                trace!("Response balance {:?} {:?}", key, val);
                self.balances.insert(key, val);
            }
            Response::LastId { id } => {
                info!("Response last_id {:?}", id);
                self.last_id = Some(id);
            }
            Response::TransactionCount { transaction_count } => {
                info!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tx: Transaction) -> io::Result<usize> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let tx = Transaction::new(keypair, to, n, *last_id);
        let sig = tx.sig;
        self.transfer_signed(tx).map(|_| sig)
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
            self.process_response(resp);
        }
        self.balances[pubkey].ok_or(io::Error::new(io::ErrorKind::Other, "nokey"))
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will hang.
    pub fn transaction_count(&mut self) -> u64 {
        info!("transaction_count");
        let req = Request::GetTransactionCount;
        let data =
            serialize(&req).expect("serialize GetTransactionCount in pub fn transaction_count");
        self.requests_socket
            .send_to(&data, &self.requests_addr)
            .expect("buffer error in pub fn transaction_count");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("transaction count dropped");
            info!("recv_response {:?}", resp);
            if let &Response::TransactionCount { .. } = &resp {
                done = true;
            }
            self.process_response(resp);
        }
        self.transaction_count
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_last_id(&mut self) -> Hash {
        info!("get_last_id");
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetLastId in pub fn get_last_id");
        self.requests_socket
            .send_to(&data, &self.requests_addr)
            .expect("buffer error in pub fn get_last_id");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("get_last_id response");
            if let &Response::LastId { .. } = &resp {
                done = true;
            }
            self.process_response(resp);
        }
        self.last_id.expect("some last_id")
    }

    pub fn poll_get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        use std::time::Instant;

        let mut balance;
        let now = Instant::now();
        loop {
            balance = self.get_balance(pubkey);
            if balance.is_ok() || now.elapsed().as_secs() > 1 {
                break;
            }
        }

        balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use budget::Budget;
    use crdt::TestNode;
    use logger;
    use mint::Mint;
    use server::Server;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use transaction::{Instruction, Plan};

    #[test]
    fn test_thin_client() {
        logger::setup();
        let leader = TestNode::new();

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = Server::new_leader(
            bank,
            Some(Duration::from_millis(30)),
            leader.data.clone(),
            leader.sockets.requests,
            leader.sockets.transaction,
            leader.sockets.broadcast,
            leader.sockets.respond,
            leader.sockets.gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(900));

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            leader.data.requests_addr,
            requests_socket,
            leader.data.transactions_addr,
            transactions_socket,
        );
        let last_id = client.get_last_id();
        let _sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        let balance = client.poll_get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }

    #[test]
    fn test_bad_sig() {
        logger::setup();
        let leader = TestNode::new();
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = Server::new_leader(
            bank,
            Some(Duration::from_millis(30)),
            leader.data.clone(),
            leader.sockets.requests,
            leader.sockets.transaction,
            leader.sockets.broadcast,
            leader.sockets.respond,
            leader.sockets.gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(300));

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        requests_socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut client = ThinClient::new(
            leader.data.requests_addr,
            requests_socket,
            leader.data.transactions_addr,
            transactions_socket,
        );
        let last_id = client.get_last_id();

        let tx = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = client.transfer_signed(tx).unwrap();

        let last_id = client.get_last_id();

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id);
        if let Instruction::NewContract(contract) = &mut tr2.instruction {
            contract.tokens = 502;
            contract.plan = Plan::Budget(Budget::new_payment(502, bob_pubkey));
        }
        let _sig = client.transfer_signed(tr2).unwrap();

        let balance = client.poll_get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }
}
