//! A command-line executable for generating the chain's genesis block.

extern crate atty;
#[macro_use]
extern crate clap;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use clap::{App, Arg};
use solana::entry_writer::EntryWriter;
use solana::mint::Mint;
use std::error;
use std::io::{stdin, stdout, Read};
use std::process::exit;

fn main() -> Result<(), Box<error::Error>> {
    let matches = App::new("solana-genesis")
        .arg(
            Arg::with_name("tokens")
                .short("t")
                .long("tokens")
                .value_name("NUMBER")
                .takes_value(true)
                .required(true)
                .default_value("0")
                .help("Number of tokens with which to initialize mint"),
        )
        .get_matches();

    let tokens = value_t_or_exit!(matches, "tokens", i64);

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

    let mut writer = stdout();
    EntryWriter::write_entries(&mut writer, mint.create_entries())?;
    Ok(())
}
