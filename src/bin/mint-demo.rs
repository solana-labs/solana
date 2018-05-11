extern crate rayon;
extern crate ring;
extern crate serde_json;
extern crate solana;

use solana::mint::{Mint, MintDemo};
use std::io;

fn main() {
    let mut input_text = String::new();
    io::stdin().read_line(&mut input_text).unwrap();
    let trimmed = input_text.trim();
    let tokens = trimmed.parse::<i64>().unwrap();

    let mint = Mint::new(tokens);
    let tokens_per_user = 1_000;
    let num_accounts = tokens / tokens_per_user;

    let demo = MintDemo { mint, num_accounts };
    println!("{}", serde_json::to_string(&demo).unwrap());
}
