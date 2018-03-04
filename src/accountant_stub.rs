//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use std::net::UdpSocket;
use std::io;
use bincode::{deserialize, serialize};
use event::{get_pubkey, sign_transaction_data, PublicKey, Signature};
use ring::signature::Ed25519KeyPair;
use accountant_skel::{Request, Response};

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

    pub fn transfer_signed(
        self: &Self,
        from: PublicKey,
        to: PublicKey,
        val: u64,
        sig: Signature,
    ) -> io::Result<usize> {
        let req = Request::Transfer { from, to, val, sig };
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr)
    }

    pub fn transfer(
        self: &Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
    ) -> io::Result<Signature> {
        let from = get_pubkey(keypair);
        let sig = sign_transaction_data(&n, keypair, &to);
        self.transfer_signed(from, to, n, sig).map(|_| sig)
    }

    pub fn get_balance(self: &Self, pubkey: &PublicKey) -> io::Result<u64> {
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
        Ok(0)
    }

    pub fn wait_on_signature(self: &Self, wait_sig: &Signature) -> io::Result<()> {
        let req = Request::Wait { sig: *wait_sig };
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr).map(|_| ())?;

        let mut buf = vec![0u8; 1024];
        self.socket.recv_from(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize signature");
        if let Response::Confirmed { sig } = resp {
            assert_eq!(sig, *wait_sig);
        }
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
        let acc = AccountantStub::new(addr, socket);
        let sig = acc.transfer(500, &alice.get_keypair(), bob_pubkey).unwrap();
        acc.wait_on_signature(&sig).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);
    }
}
