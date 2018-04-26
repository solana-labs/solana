extern crate rayon;
extern crate ring;
extern crate serde_json;
extern crate solana;

use rayon::prelude::*;
use ring::rand::SystemRandom;
use solana::mint::{Mint, MintDemo};
use solana::signature::KeyPair;
use std::io;

fn main() {
    let mut input_text = String::new();
    io::stdin().read_line(&mut input_text).unwrap();
    let trimmed = input_text.trim();
    let tokens = trimmed.parse::<i64>().unwrap();

    let mint = Mint::new(tokens);
    let tokens_per_user = 1_000;
    let num_accounts = tokens / tokens_per_user;
    let rnd = SystemRandom::new();

    let users: Vec<_> = (0..num_accounts)
        .into_par_iter()
        .map(|_| {
            let pkcs8 = KeyPair::generate_pkcs8(&rnd).unwrap().to_vec();
            (pkcs8, tokens_per_user)
        })
        .collect();

    let demo = MintDemo { mint, users };
    println!("{}", serde_json::to_string(&demo).unwrap());
}
