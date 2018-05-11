extern crate isatty;
extern crate rayon;
extern crate ring;
extern crate serde_json;
extern crate solana;
extern crate untrusted;

use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::accountant::MAX_ENTRY_IDS;
use solana::entry::{create_entry, next_tick};
use solana::event::Event;
use solana::mint::MintDemo;
use solana::signature::{GenKeys, KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::io::{stdin, Read};
use std::process::exit;
use untrusted::Input;

// Generate a ledger with lots and lots of accounts.
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

    let demo: MintDemo = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });

    let rnd = GenKeys::new(demo.mint.keypair().public_key_bytes());
    let num_accounts = demo.num_accounts;
    let tokens_per_user = 1_000;

    let users = rnd.gen_n_keys(num_accounts, tokens_per_user);

    let last_id = demo.mint.last_id();
    let mint_keypair = demo.mint.keypair();

    eprintln!("Signing {} transactions...", num_accounts);
    let events: Vec<_> = users
        .into_par_iter()
        .map(|(pkcs8, tokens)| {
            let rando = KeyPair::from_pkcs8(Input::from(&pkcs8)).unwrap();
            let tr = Transaction::new(&mint_keypair, rando.pubkey(), tokens, last_id);
            Event::Transaction(tr)
        })
        .collect();

    for entry in demo.mint.create_entries() {
        println!("{}", serde_json::to_string(&entry).unwrap());
    }

    eprintln!("Logging the creation of {} accounts...", num_accounts);
    let entry = create_entry(&last_id, 0, events);
    println!("{}", serde_json::to_string(&entry).unwrap());

    eprintln!("Creating {} empty entries...", MAX_ENTRY_IDS);
    // Offer client lots of entry IDs to use for each transaction's last_id.
    let mut last_id = last_id;
    for _ in 0..MAX_ENTRY_IDS {
        let entry = next_tick(&last_id, 1);
        last_id = entry.id;
        let serialized = serde_json::to_string(&entry).unwrap_or_else(|e| {
            eprintln!("failed to serialize: {}", e);
            exit(1);
        });
        println!("{}", serialized);
    }
}
