#[macro_use]
extern crate clap;
use dirs;

use clap::{App, Arg};
use solana::wallet::gen_keypair_file;
use std::error;

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new("solana-keygen")
        .version(crate_version!())
        .arg(
            Arg::with_name("outfile")
                .short("o")
                .long("outfile")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to generated file"),
        )
        .get_matches();

    let mut path = dirs::home_dir().expect("home directory");
    let outfile = if matches.is_present("outfile") {
        matches.value_of("outfile").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };

    let serialized_keypair = gen_keypair_file(outfile.to_string())?;
    if outfile == "-" {
        println!("{}", serialized_keypair);
    }
    Ok(())
}
