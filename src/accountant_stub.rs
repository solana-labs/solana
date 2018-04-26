//! The `accountant_stub` module is a client-side object that interfaces with a server-side Accountant
//! object via the network interface exposed by AccountantSkel. Client code should use
//! this object instead of writing messages to the network directly. The binary
//! encoding of its messages are unstable and may change in future releases.

use accountant_skel::{Request, Response, Subscription};
use bincode::{deserialize, serialize};
use futures::future::{ok, FutureResult};
use hash::Hash;
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::UdpSocket;
use transaction::Transaction;

pub struct AccountantStub {
    pub addr: String,
    pub socket: UdpSocket,
    last_id: Option<Hash>,
    num_events: u64,
    balances: HashMap<PublicKey, Option<i64>>,
}

impl AccountantStub {
    /// Create a new AccountantStub that will interface with AccountantSkel
    /// over `socket`. To receive responses, the caller must bind `socket`
    /// to a public address before invoking AccountantStub methods.
    pub fn new(addr: &str, socket: UdpSocket) -> Self {
        let stub = AccountantStub {
            addr: addr.to_string(),
            socket,
            last_id: None,
            num_events: 0,
            balances: HashMap::new(),
        };
        stub.init();
        stub
    }

    pub fn init(&self) {
        let subscriptions = vec![Subscription::EntryInfo];
        let req = Request::Subscribe { subscriptions };
        let data = serialize(&req).expect("serialize Subscribe");
        let _res = self.socket.send_to(&data, &self.addr);
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize balance");
        Ok(resp)
    }

    pub fn process_response(&mut self, resp: Response) {
        match resp {
            Response::Balance { key, val } => {
                self.balances.insert(key, val);
            }
            Response::LastId { id } => {
                self.last_id = Some(id);
            }
            Response::EntryInfo(entry_info) => {
                self.last_id = Some(entry_info.id);
                self.num_events += entry_info.num_events;
            }
        }
    }

    /// Send a signed Transaction to the server for processing. This method
    /// does not wait for a response.
    pub fn transfer_signed(&self, tr: Transaction) -> io::Result<usize> {
        let req = Request::Transaction(tr);
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr)
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let tr = Transaction::new(keypair, to, n, *last_id);
        let sig = tr.sig;
        self.transfer_signed(tr).map(|_| sig)
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&mut self, pubkey: &PublicKey) -> FutureResult<i64, i64> {
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance");
        self.socket
            .send_to(&data, &self.addr)
            .expect("buffer error");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("recv response");
            if let &Response::Balance { ref key, .. } = &resp {
                done = key == pubkey;
            }
            self.process_response(resp);
        }
        ok(self.balances[pubkey].unwrap())
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response. At the time of this writing,
    /// it also has the side-effect of causing the server to log any
    /// entries that have been published by the Historian.
    pub fn get_last_id(&mut self) -> FutureResult<Hash, ()> {
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetId");
        self.socket
            .send_to(&data, &self.addr)
            .expect("buffer error");
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("recv response");
            if let &Response::LastId { .. } = &resp {
                done = true;
            }
            self.process_response(resp);
        }
        ok(self.last_id.unwrap_or(Hash::default()))
    }

    /// Return the number of transactions the server processed since creating
    /// this stub instance.
    pub fn transaction_count(&mut self) -> u64 {
        // Wait for at least one EntryInfo.
        let mut done = false;
        while !done {
            let resp = self.recv_response().expect("recv response");
            if let &Response::EntryInfo(_) = &resp {
                done = true;
            }
            self.process_response(resp);
        }

        // Then take the rest.
        self.socket.set_nonblocking(true).expect("set nonblocking");
        loop {
            match self.recv_response() {
                Err(_) => break,
                Ok(resp) => self.process_response(resp),
            }
        }
        self.socket.set_nonblocking(false).expect("set blocking");
        self.num_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accountant::Accountant;
    use accountant_skel::AccountantSkel;
    use futures::Future;
    use historian::Historian;
    use mint::Mint;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::Duration;

    // TODO: Figure out why this test sometimes hangs on TravisCI.
    #[test]
    fn test_accountant_stub() {
        let addr = "127.0.0.1:9000";
        let send_addr = "127.0.0.1:9001";
        let alice = Mint::new(10_000);
        let acc = Accountant::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let historian = Historian::new(&alice.last_id(), Some(30));
        let acc = Arc::new(Mutex::new(AccountantSkel::new(
            acc,
            alice.last_id(),
            sink(),
            historian,
        )));
        let _threads = AccountantSkel::serve(&acc, addr, exit.clone()).unwrap();
        sleep(Duration::from_millis(300));

        let socket = UdpSocket::bind(send_addr).unwrap();
        socket.set_read_timeout(Some(Duration::new(5, 0))).unwrap();

        let mut acc = AccountantStub::new(addr, socket);
        let last_id = acc.get_last_id().wait().unwrap();
        let _sig = acc.transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).wait().unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
    }
}
