extern crate clap;
extern crate ring;
extern crate serde_json;

use clap::{App, Arg};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use std::error;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() -> Result<(), Box<error::Error>> {
    let matches = App::new("solana-genesis")
        .arg(
            Arg::with_name("outfile")
                .short("o")
                .long("outfile")
                .value_name("PATH")
                .takes_value(true)
                .default_value("~/.config/solana/id.json")
                .help("Number of tokens with which to initialize mint"),
        )
        .get_matches();

    let rnd = SystemRandom::new();
    let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rnd)?;
    let serialized = serde_json::to_string(&pkcs8_bytes.to_vec())?;

    let outfile = matches.value_of("outfile").unwrap();

    if outfile == "-" {
        println!("{}", serialized);
    } else {
        if let Some(outdir) = Path::new(outfile).parent() {
            fs::create_dir_all(outdir)?;
        }
        let mut f = File::create(outfile)?;
        f.write_all(&serialized.into_bytes())?;
    }

    Ok(())
}
