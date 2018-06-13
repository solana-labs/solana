extern crate atty;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use solana::mint::Mint;
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
    let tokens = trimmed.parse::<i64>().unwrap_or_else(|e| {
        eprintln!("{}", e);
        exit(1);
    });
    let mint = Mint::new(tokens);
    let serialized = serde_json::to_string(&mint).unwrap_or_else(|e| {
        eprintln!("failed to serialize: {}", e);
        exit(1);
    });
    println!("{}", serialized);
}
