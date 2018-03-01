//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use std::net::UdpSocket;
use std::io;
use bincode::{deserialize, serialize};
use log::{PublicKey, Signature};
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

    pub fn deposit_signed(
        self: &mut Self,
        key: PublicKey,
        val: u64,
        sig: Signature,
    ) -> io::Result<usize> {
        let req = Request::Deposit { key, val, sig };
        let data = serialize(&req).unwrap();
        self.socket.send_to(&data, &self.addr)
    }

    pub fn deposit(self: &mut Self, n: u64, keypair: &Ed25519KeyPair) -> io::Result<Signature> {
        use log::{get_pubkey, sign_serialized};
        let key = get_pubkey(keypair);
        let sig = sign_serialized(&n, keypair);
        self.deposit_signed(key, n, sig).map(|_| sig)
    }

    pub fn transfer_signed(
        self: &mut Self,
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
        self: &mut Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
    ) -> io::Result<Signature> {
        use log::{get_pubkey, sign_transaction_data};
        let from = get_pubkey(keypair);
        let sig = sign_transaction_data(&n, keypair, &to);
        self.transfer_signed(from, to, n, sig).map(|_| sig)
    }

    pub fn get_balance(self: &mut Self, pubkey: &PublicKey) -> io::Result<u64> {
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

    pub fn wait_on_signature(self: &mut Self, wait_sig: &Signature) -> io::Result<()> {
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
    use log::{generate_keypair, get_pubkey, Sha256Hash};

    #[test]
    fn test_accountant_stub() {
        let addr = "127.0.0.1:9000";
        let send_addr = "127.0.0.1:9001";
        spawn(move || {
            let zero = Sha256Hash::default();
            let acc = Accountant::new(&zero, None);
            let mut skel = AccountantSkel::new(acc);
            skel.serve(addr).unwrap();
        });

        sleep(Duration::from_millis(30));

        let socket = UdpSocket::bind(send_addr).unwrap();
        let mut acc = AccountantStub::new(addr, socket);
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        let sig = acc.deposit(1_000, &bob_keypair).unwrap();
        acc.wait_on_signature(&sig).unwrap();

        let bob_pubkey = get_pubkey(&bob_keypair);
        let sig = acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();
        acc.wait_on_signature(&sig).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);
    }
}
