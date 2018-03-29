//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use accountant_skel::{Request, Response};
use bincode::{deserialize, serialize};
use entry::Entry;
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
    pub fn new(addr: &str, socket: UdpSocket) -> Self {
        AccountantStub {
            addr: addr.to_string(),
            socket,
        }
    }

    pub fn transfer_signed(&self, tr: Transaction) -> io::Result<usize> {
        let req = Request::Transaction(tr);
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr)
    }

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

    fn get_id(&self, is_last: bool) -> io::Result<Hash> {
        let req = Request::GetId { is_last };
        let data = serialize(&req).expect("serialize GetId");
        self.socket.send_to(&data, &self.addr)?;
        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize Id");
        if let Response::Id { id, .. } = resp {
            return Ok(id);
        }
        Ok(Default::default())
    }

    pub fn get_last_id(&self) -> io::Result<Hash> {
        self.get_id(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accountant::Accountant;
    use accountant_skel::AccountantSkel;
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
        let acc = Accountant::new(&alice, Some(30));
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let acc = Arc::new(Mutex::new(AccountantSkel::new(acc, sink())));
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
