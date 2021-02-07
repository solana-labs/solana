// Utility to print ValidatorInfo structs for `genesis_accounts.rs`
//
// Usage:
//   cargo run --bin solana-csv-to-validator-infos < validators.csv

use serde::Deserialize;
use std::error::Error;
use std::io;
use std::process;

#[derive(Debug, Deserialize)]
struct ValidatorRecord {
    id: u64,
    tokens: f64,
    adjective: String,
    noun: String,
    identity_pubkey: String,
    vote_pubkey: String,
}

fn parse_csv() -> Result<(), Box<dyn Error>> {
    let mut rdr = csv::Reader::from_reader(io::stdin());
    for result in rdr.deserialize() {
        let record: ValidatorRecord = result?;
        println!(
            r#"ValidatorInfo {{name: "{adjective} {noun}", node: "{identity_pubkey}", node_sol: {tokens:.1}, vote: "{vote_pubkey}", commission: 0}},"#,
            tokens = &record.tokens,
            adjective = &record.adjective,
            noun = &record.noun,
            identity_pubkey = &record.identity_pubkey,
            vote_pubkey = &record.vote_pubkey,
        );
    }
    Ok(())
}

fn main() {
    if let Err(err) = parse_csv() {
        println!("error: {}", err);
        process::exit(1);
    }
}
