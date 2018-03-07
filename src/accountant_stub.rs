//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use std::net::UdpSocket;
use std::io;
use bincode::{deserialize, serialize};
use transaction::Transaction;
use signature::{PublicKey, Signature};
use hash::Sha256Hash;
use entry::Entry;
use ring::signature::Ed25519KeyPair;
use accountant_skel::{Request, Response};

pub struct AccountantStub {
    pub addr: String,
    pub socket: UdpSocket,
    pub last_id: Option<Sha256Hash>,
}

impl AccountantStub {
    pub fn new(addr: &str, socket: UdpSocket) -> Self {
        AccountantStub {
            addr: addr.to_string(),
            socket,
            last_id: None,
        }
    }

    pub fn transfer_signed(&self, tr: Transaction<i64>) -> io::Result<usize> {
        let req = Request::Transaction(tr);
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr)
    }

    pub fn transfer(
        &self,
        n: i64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
        last_id: &Sha256Hash,
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

    fn get_id(&self, is_last: bool) -> io::Result<Sha256Hash> {
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

    pub fn get_last_id(&self) -> io::Result<Sha256Hash> {
        self.get_id(true)
    }

    pub fn wait_on_signature(&mut self, wait_sig: &Signature) -> io::Result<()> {
        let last_id = match self.last_id {
            None => {
                let first_id = self.get_id(false)?;
                self.last_id = Some(first_id);
                first_id
            }
            Some(last_id) => last_id,
        };

        let req = Request::GetEntries { last_id };
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr).map(|_| ())?;

        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize signature");
        if let Response::Entries { entries } = resp {
            for Entry { id, event, .. } in entries {
                self.last_id = Some(id);
                if let Some(sig) = event.get_signature() {
                    if sig == *wait_sig {
                        return Ok(());
                    }
                }
            }
        }

        // TODO: Loop until we found it.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accountant::Accountant;
    use accountant_skel::AccountantSkel;
    use std::thread::{sleep, spawn};
    use std::time::Duration;
    use genesis::{Creator, Genesis};

    #[test]
    fn test_accountant_stub() {
        let addr = "127.0.0.1:9000";
        let send_addr = "127.0.0.1:9001";
        let bob = Creator::new(1_000);
        let bob_pubkey = bob.pubkey;
        let alice = Genesis::new(10_000, vec![bob]);
        let acc = Accountant::new(&alice, None);
        spawn(move || AccountantSkel::new(acc).serve(addr).unwrap());
        sleep(Duration::from_millis(30));

        let socket = UdpSocket::bind(send_addr).unwrap();
        let mut acc = AccountantStub::new(addr, socket);
        let last_id = acc.get_last_id().unwrap();
        let sig = acc.transfer(500, &alice.get_keypair(), bob_pubkey, &last_id)
            .unwrap();
        acc.wait_on_signature(&sig).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap().unwrap(), 1_500);
    }
}
