extern crate isatty;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::bank::MAX_ENTRY_IDS;
use solana::entry::{next_entry, Entry};
use solana::mint::MintDemo;
use solana::signature::{GenKeys, KeyPairUtil};
use solana::transaction::Transaction;
use std::io::{stdin, Read};
use std::process::exit;

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

    let keypairs = rnd.gen_n_keypairs(num_accounts);

    let mint_keypair = demo.mint.keypair();
    let last_id = demo.mint.last_id();

    for entry in demo.mint.create_entries() {
        println!("{}", serde_json::to_string(&entry).unwrap());
    }

    eprintln!("Creating {} empty entries...", MAX_ENTRY_IDS);

    // Offer client lots of entry IDs to use for each transaction's last_id.
    let mut last_id = last_id;
    let mut last_ids = vec![];
    for _ in 0..MAX_ENTRY_IDS {
        let entry = next_entry(&last_id, 1, vec![]);
        last_id = entry.id;
        last_ids.push(last_id);
        let serialized = serde_json::to_string(&entry).unwrap_or_else(|e| {
            eprintln!("failed to serialize: {}", e);
            exit(1);
        });
        println!("{}", serialized);
    }

    eprintln!("Creating {} transactions...", num_accounts);
    let transactions: Vec<_> = keypairs
        .into_par_iter()
        .enumerate()
        .map(|(i, rando)| {
            let last_id = last_ids[i % MAX_ENTRY_IDS];
            Transaction::new(&mint_keypair, rando.pubkey(), tokens_per_user, last_id)
        })
        .collect();

    eprintln!("Logging the creation of {} accounts...", num_accounts);
    let entry = Entry::new(&last_id, 0, transactions);
    println!("{}", serde_json::to_string(&entry).unwrap());
}
