//! A command-line executable for generating the chain's genesis block.

#[macro_use]
extern crate clap;
use serde_json;

use clap::{App, Arg};
use solana::ledger::LedgerWriter;
use solana::mint::Mint;
use solana_sdk::signature::{read_keypair, KeypairUtil};
use std::error;
use std::fs::File;
use std::path::Path;

/**
 * Bootstrap leader gets two tokens:
 * - one token to create an instance of the vote_program with
 * - one token for the transaction fee
 * - one second token to keep the node identity public key valid
 */
pub const BOOTSTRAP_LEADER_TOKENS: u64 = 3;

fn main() -> Result<(), Box<dyn error::Error>> {
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
            Arg::with_name("bootstrap-leader-keypair")
                .short("b")
                .long("bootstrap-leader-keypair")
                .value_name("BOOTSTRAP LEADER KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's keypair"),
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

    // Load the bootstreap leader keypair
    // TODO: Only the public key is really needed, genesis should not have access to the leader's
    //       secret key.
    let leader_keypair = read_keypair(matches.value_of("bootstrap-leader-keypair").unwrap())
        .expect("failed to read bootstrap leader keypair");

    // Parse the input mint configuration
    let num_tokens = value_t_or_exit!(matches, "num_tokens", u64);
    let file = File::open(Path::new(&matches.value_of("mint").unwrap())).unwrap();
    let pkcs8: Vec<u8> = serde_json::from_reader(&file)?;
    let mint = Mint::new_with_pkcs8(
        num_tokens,
        pkcs8,
        leader_keypair.pubkey(),
        BOOTSTRAP_LEADER_TOKENS,
    );

    // Write the ledger entries
    let ledger_path = matches.value_of("ledger").unwrap();
    let mut ledger_writer = LedgerWriter::open(&ledger_path, true)?;
    ledger_writer.write_entries(&mint.create_entries())?;

    Ok(())
}
