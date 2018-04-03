//! The `accountant_stub` module is a client-side object that interfaces with a server-side Accountant
//! object via the network interface exposed by AccountantSkel. Client code should use
//! this object instead of writing messages to the network directly. The binary
//! encoding of its messages are unstable and may change in future releases.

use accountant_skel::{Request, Response};
use bincode::{deserialize, serialize};
use hash::Hash;
use signature::{KeyPair, PublicKey, Signature};
use std::io;
use std::net::UdpSocket;
use transaction::Transaction;

pub struct AccountantStub {
    pub addr: String,
    pub socket: UdpSocket,
}

impl AccountantStub {
    /// Create a new AccountantStub that will interface with AccountantSkel
    /// over `socket`. To receive responses, the caller must bind `socket`
    /// to a public address before invoking AccountantStub methods.
    pub fn new(addr: &str, socket: UdpSocket) -> Self {
        AccountantStub {
            addr: addr.to_string(),
            socket,
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
    pub fn get_balance(&self, pubkey: &PublicKey) -> io::Result<Option<i64>> {
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance");
        self.socket.send_to(&data, &self.addr)?;
        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize balance");
        if let Response::Balance { key, val } = resp {
            assert_eq!(key, *pubkey);
            return Ok(val);
        }
        Ok(None)
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response. At the time of this writing,
    /// it also has the side-effect of causing the server to log any
    /// entries that have been published by the Historian.
    pub fn get_last_id(&self) -> io::Result<Hash> {
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetId");
        self.socket.send_to(&data, &self.addr)?;
        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize Id");
        if let Response::LastId { id } = resp {
            return Ok(id);
        }
        Ok(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accountant::Accountant;
    use accountant_skel::AccountantSkel;
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
        let _threads = AccountantSkel::serve(acc, addr, exit.clone()).unwrap();
        sleep(Duration::from_millis(300));

        let socket = UdpSocket::bind(send_addr).unwrap();

        let acc = AccountantStub::new(addr, socket);
        let last_id = acc.get_last_id().unwrap();
        let _sig = acc.transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap().unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
    }
}
