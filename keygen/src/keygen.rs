use clap::{crate_description, crate_name, crate_version, App, Arg, SubCommand};
use solana_sdk::pubkey::write_pubkey;
use solana_sdk::signature::{gen_keypair_file, read_keypair, KeypairUtil};
use std::error;

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("outfile")
                .short("o")
                .long("outfile")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to generated file"),
        )
        .subcommand(
            SubCommand::with_name("new")
                .about("Generate new keypair file")
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("PATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("pubkey")
                .about("Display the pubkey from a keypair file")
                .arg(
                    Arg::with_name("infile")
                        .index(1)
                        .value_name("PATH")
                        .takes_value(true)
                        .help("Path to keypair file"),
                )
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("PATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("pubkey", Some(pubkey_matches)) => {
            let mut path = dirs::home_dir().expect("home directory");
            let infile = if pubkey_matches.is_present("infile") {
                pubkey_matches.value_of("infile").unwrap()
            } else {
                path.extend(&[".config", "solana", "id.json"]);
                path.to_str().unwrap()
            };
            let keypair = read_keypair(infile)?;

            if pubkey_matches.is_present("outfile") {
                let outfile = pubkey_matches.value_of("outfile").unwrap();
                write_pubkey(outfile, keypair.pubkey())?;
            } else {
                println!("{}", keypair.pubkey());
            }
        }
        match_tuple => {
            let working_matches = if let (_, Some(new_matches)) = match_tuple {
                new_matches
            } else {
                &matches
            };

            let mut path = dirs::home_dir().expect("home directory");
            let outfile = if working_matches.is_present("outfile") {
                working_matches.value_of("outfile").unwrap()
            } else {
                path.extend(&[".config", "solana", "id.json"]);
                path.to_str().unwrap()
            };

            let serialized_keypair = gen_keypair_file(outfile)?;
            if outfile == "-" {
                println!("{}", serialized_keypair);
            }
        }
    }

    Ok(())
}
