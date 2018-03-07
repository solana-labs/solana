extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use silk::event::Event;
use silk::transaction::Transaction;
use silk::log::create_entries;
use silk::signature::{KeyPair, KeyPairUtil, PublicKey};
use silk::hash::Hash;
use std::io::stdin;

fn transfer(from: &KeyPair, (to, tokens): (PublicKey, i64), last_id: Hash) -> Event {
    Event::Transaction(Transaction::new(&from, to, tokens, last_id))
}

fn main() {
    let alice = (KeyPair::new().pubkey(), 200);
    let bob = (KeyPair::new().pubkey(), 100);

    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    let from = gen.keypair();
    let seed = gen.seed();
    let mut events = gen.create_events();
    events.push(transfer(&from, alice, seed));
    events.push(transfer(&from, bob, seed));

    for entry in create_entries(&seed, events) {
        println!("{}", serde_json::to_string(&entry).unwrap());
    }
}
