//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use std::net::TcpStream;
use std::io;
use std::io::{Read, Write};
use bincode::{deserialize, serialize};
use log::{PublicKey, Signature};
use ring::signature::Ed25519KeyPair;
use accountant_skel::{Request, Response};

pub struct AccountantStub {
    pub addr: String,
}

impl AccountantStub {
    pub fn new(addr: &str) -> Self {
        AccountantStub {
            addr: addr.to_string(),
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
        let mut stream = TcpStream::connect(&self.addr)?;
        stream.write(&data)
    }

    pub fn deposit(self: &mut Self, n: u64, keypair: &Ed25519KeyPair) -> io::Result<usize> {
        use log::{get_pubkey, sign_serialized};
        let key = get_pubkey(keypair);
        let sig = sign_serialized(&n, keypair);
        self.deposit_signed(key, n, sig)
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
        println!("TcpStream::connect()...");
        let mut stream = TcpStream::connect(&self.addr)?;
        println!("Connected.");
        println!("accountant_stub: Writing transfer message...");
        let ret = stream.write(&data);
        println!("Done.");
        ret
    }

    pub fn transfer(
        self: &mut Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
    ) -> io::Result<usize> {
        use log::{get_pubkey, sign_transaction_data};
        let from = get_pubkey(keypair);
        let sig = sign_transaction_data(&n, keypair, &to);
        self.transfer_signed(from, to, n, sig)
    }

    pub fn get_balance(self: &mut Self, pubkey: &PublicKey) -> io::Result<u64> {
        let mut stream = TcpStream::connect(&self.addr)?;
        let req = Request::GetBalance { key: *pubkey };
        let data = serialize(&req).expect("serialize GetBalance");
        stream.write(&data)?;
        let mut buf = vec![0u8; 1024];
        stream.read(&mut buf)?;
        let resp = deserialize(&buf).expect("deserialize balance");
        let Response::Balance { key, val } = resp;
        assert_eq!(key, *pubkey);
        Ok(val)
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
        let addr = "127.0.0.1:8000";
        spawn(move || {
            let zero = Sha256Hash::default();
            let acc = Accountant::new(&zero, None);
            let mut skel = AccountantSkel::new(acc);
            skel.serve(addr).unwrap();
        });

        sleep(Duration::from_millis(30));

        let mut acc = AccountantStub::new(addr);
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        acc.deposit(1_000, &bob_keypair).unwrap();

        sleep(Duration::from_millis(30));
        let bob_pubkey = get_pubkey(&bob_keypair);
        acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();

        sleep(Duration::from_millis(300));
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);
    }
}
