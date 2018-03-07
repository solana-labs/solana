extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use silk::event::Event;
use silk::transaction::Transaction;
use silk::log::create_entries;
use silk::signature::{generate_keypair, get_pubkey, KeyPair, PublicKey};
use silk::hash::Hash;

fn transfer(from: &KeyPair, (to, tokens): (PublicKey, i64), last_id: Hash) -> Event {
    Event::Transaction(Transaction::new(&from, to, tokens, last_id))
}

fn main() {
    let alice = (get_pubkey(&generate_keypair()), 200);
    let bob = (get_pubkey(&generate_keypair()), 100);

    let gen = Genesis::new(500);
    let from = gen.get_keypair();
    let seed = gen.get_seed();
    let mut events = gen.create_events();
    events.push(transfer(&from, alice, seed));
    events.push(transfer(&from, bob, seed));

    create_entries(&seed, events);
    println!("{}", serde_json::to_string(&gen).unwrap());
}
