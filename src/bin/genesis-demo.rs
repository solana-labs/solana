extern crate isatty;
extern crate serde_json;
extern crate solana;

use isatty::stdin_isatty;
use solana::entry::create_entry;
use solana::event::Event;
use solana::hash::Hash;
use solana::mint::Mint;
use solana::signature::{KeyPair, KeyPairUtil, PublicKey};
use solana::transaction::Transaction;
use std::io::{stdin, Read};
use std::process::exit;

fn transfer(from: &KeyPair, (to, tokens): (PublicKey, i64), last_id: Hash) -> Event {
    Event::Transaction(Transaction::new(from, to, tokens, last_id))
}

fn main() {
    if stdin_isatty() {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    let mint: Mint = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });
    let mut entries = mint.create_entries();

    let from = mint.keypair();
    let seed = mint.seed();
    let alice = (KeyPair::new().pubkey(), 200);
    let bob = (KeyPair::new().pubkey(), 100);
    let events = vec![transfer(&from, alice, seed), transfer(&from, bob, seed)];
    entries.push(create_entry(&seed, 0, events));

    for entry in entries {
        let serialized = serde_json::to_string(&entry).unwrap_or_else(|e| {
            eprintln!("failed to serialize: {}", e);
            exit(1);
        });
        println!("{}", serialized);
    }
}
