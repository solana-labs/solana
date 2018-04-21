//! A command-line executable for generating the chain's genesis block.

extern crate isatty;
extern crate serde_json;
extern crate solana;

use isatty::stdin_isatty;
use solana::mint::Mint;
use std::io::{stdin, Read};
use std::process::exit;

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
    for x in mint.create_entries() {
        let serialized = serde_json::to_string(&x).unwrap_or_else(|e| {
            eprintln!("failed to serialize: {}", e);
            exit(1);
        });
        println!("{}", serialized);
    }
}
