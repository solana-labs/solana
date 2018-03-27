//! A command-line executable for generating the chain's genesis block.

extern crate serde_json;
extern crate solana;

use solana::mint::Mint;
use std::io::stdin;

fn main() {
    let mint: Mint = serde_json::from_reader(stdin()).unwrap();
    for x in mint.create_entries() {
        println!("{}", serde_json::to_string(&x).unwrap());
    }
}
