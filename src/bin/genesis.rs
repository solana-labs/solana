//! A command-line executable for generating the chain's genesis block.

extern crate atty;
#[macro_use]
extern crate clap;
extern crate serde_json;
extern crate solana;
extern crate untrusted;

use clap::{App, Arg};
use solana::fullnode::Config;
use solana::ledger::LedgerWriter;
use solana::mint::Mint;
use solana::signature::KeypairUtil;
use std::error;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<error::Error>> {
    let matches = App::new("solana-genesis")
        .version(crate_version!())
        .arg(
            Arg::with_name("num_tokens")
                .short("t")
                .long("num_tokens")
                .value_name("TOKENS")
                .takes_value(true)
                .required(true)
                .help("Number of tokens to create in the mint"),
        )
        .arg(
            Arg::with_name("mint")
                .short("m")
                .long("mint")
                .value_name("MINT")
                .takes_value(true)
                .required(true)
                .help("Path to file containing keys of the mint"),
        )
        .arg(
            Arg::with_name("bootstrap_leader")
                .short("b")
                .long("bootstrap_leader")
                .value_name("BOOTSTRAP LEADER")
                .takes_value(true)
                .required(true)
                .help("Path to file containing keys of the bootstrap leader"),
        )
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory as persistent ledger location"),
        )
        .get_matches();

    // Parse the input leader configuration
    let file = File::open(Path::new(&matches.value_of("bootstrap_leader").unwrap())).unwrap();
    let leader_config: Config = serde_json::from_reader(file).unwrap();
    let leader_keypair = leader_config.keypair();

    // Parse the input mint configuration
    let num_tokens = value_t_or_exit!(matches, "num_tokens", u64);
    let file = File::open(Path::new(&matches.value_of("mint").unwrap())).unwrap();
    let pkcs8: Vec<u8> = serde_json::from_reader(&file)?;
    let mint = Mint::new_with_pkcs8(num_tokens, pkcs8, leader_keypair.pubkey(), 1);

    // Write the ledger entries
    let ledger_path = matches.value_of("ledger").unwrap();
    let mut ledger_writer = LedgerWriter::open(&ledger_path, true)?;
    ledger_writer.write_entries(&mint.create_entries())?;

    Ok(())
}
