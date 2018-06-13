extern crate atty;
extern crate rayon;
extern crate ring;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use solana::mint::{Mint, MintDemo};
use std::io;
use std::process::exit;

fn main() {
    let mut input_text = String::new();
    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a token number");
        exit(1);
    }

    io::stdin().read_line(&mut input_text).unwrap();
    let trimmed = input_text.trim();
    let tokens = trimmed.parse::<i64>().unwrap();

    let mint = Mint::new(tokens);
    let tokens_per_user = 1_000;
    let num_accounts = tokens / tokens_per_user;

    let demo = MintDemo { mint, num_accounts };
    println!("{}", serde_json::to_string(&demo).unwrap());
}
