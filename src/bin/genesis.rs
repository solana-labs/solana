//! A command-line executable for generating the chain's genesis block.

extern crate serde_json;
extern crate solana;

use solana::mint::Mint;
use std::io::stdin;
use std::process::exit;

fn main() {
    let mint: Mint = serde_json::from_reader(stdin()).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });
    for x in mint.create_entries() {
        let serialized = serde_json::to_string(&x).unwrap_or_else(|e| {
            eprintln!("failed to serialize: {}", e);
            exit(1);
        });
        println!("{}", serialized);
    }
}
