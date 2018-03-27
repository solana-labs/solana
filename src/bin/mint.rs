extern crate serde_json;
extern crate solana;

use solana::mint::Mint;
use std::io;

fn main() {
    let mut input_text = String::new();
    io::stdin().read_line(&mut input_text).unwrap();
    let trimmed = input_text.trim();
    let tokens = trimmed.parse::<i64>().unwrap();

    let mint = Mint::new(tokens);
    println!("{}", serde_json::to_string(&mint).unwrap());
}
