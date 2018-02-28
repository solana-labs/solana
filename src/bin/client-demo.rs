extern crate generic_array;
extern crate silk;

//use log::{Event, PublicKey, Sha256Hash};
//use std::net::TcpStream;
//use ring::signature::Ed25519KeyPair;
//
//pub struct AccountantStub {
//    pub stream: TcpStream,
//}
//
//impl AccountantStub {
//    pub fn new(addr: ()) -> Self {
//        let mut stream = TcpStream::connect(addr).unwrap();
//        AccountantStub {
//            stream: TcpString,
//        }
//    }
//
//    pub fn deposit(
//        self: &Self,
//        n: u64,
//        keypair: &Ed25519KeyPair,
//    ) -> Result<(), SendError<Event<u64>>> {
//        use log::sign_hash;
//        let event = sign_hash(n, &keypair);
//        self.stream.send(&serialize(event))
//    }
//
//    pub fn transfer(
//        self: &mut Self,
//        n: u64,
//        keypair: &Ed25519KeyPair,
//        pubkey: PublicKey,
//    ) -> io::Result<()> {
//        use log::transfer_hash;
//        use generic_array::GenericArray;
//        let event = transfer_hash(n, &keypair);
//        self.stream.send(&serialize(event))
//    }
//
//    pub fn get_balance(
//        self: &mut Self,
//        pubkey: PublicKey,
//    ) -> io::Result<()> {
//        let event = GetBalance { key: pubkey };
//        self.stream.send(&serialize(event));
//        msg = deserialize(self.sender.recv());
//        if let AccountantMsg::Balance { val } = msg {
//           Ok(val)
//        } else {
//           Err()
//        }
//    }
//}

use silk::accountant::Accountant;
use std::thread::sleep;
use std::time::Duration;
use silk::log::{generate_keypair, Sha256Hash};
use silk::historian::ExitReason;
use generic_array::GenericArray;

fn main() {
    let zero = Sha256Hash::default();
    let mut acc = Accountant::new(&zero, Some(2));
    let alice_keypair = generate_keypair();
    let bob_keypair = generate_keypair();
    acc.deposit(10_000, &alice_keypair).unwrap();
    acc.deposit(1_000, &bob_keypair).unwrap();

    sleep(Duration::from_millis(30));
    let bob_pubkey = GenericArray::clone_from_slice(bob_keypair.public_key_bytes());
    acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();

    sleep(Duration::from_millis(30));
    assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);

    drop(acc.historian.sender);
    assert_eq!(
        acc.historian.thread_hdl.join().unwrap().1,
        ExitReason::RecvDisconnected
    );
}
