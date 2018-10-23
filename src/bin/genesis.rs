//! A command-line executable for generating the chain's genesis block.

extern crate atty;
#[macro_use]
extern crate clap;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use clap::{App, Arg};
use solana::ledger::LedgerWriter;
use solana::mint::Mint;
use std::error;
use std::io::{stdin, Read};
use std::process::exit;

fn main() -> Result<(), Box<error::Error>> {
    let matches = App::new("solana-genesis")
        .version(crate_version!())
        .arg(
            Arg::with_name("tokens")
                .short("t")
                .long("tokens")
                .value_name("NUM")
                .takes_value(true)
                .required(true)
                .help("Number of tokens with which to initialize mint"),
        ).arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory as persistent ledger location"),
        ).get_matches();

    let tokens = value_t_or_exit!(matches, "tokens", i64);
    let ledger_path = matches.value_of("ledger").unwrap();

    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer)?;
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    let pkcs8: Vec<u8> = serde_json::from_str(&buffer)?;
    let mint = Mint::new_with_pkcs8(tokens, pkcs8);

    let mut ledger_writer = LedgerWriter::open(&ledger_path, true)?;
    ledger_writer.write_entries(&mint.create_entries())?;

    Ok(())
}
