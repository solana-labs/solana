use clap::{
    crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use solana_sdk::pubkey::write_pubkey;
use solana_sdk::signature::{gen_keypair_file, read_keypair, KeypairUtil};
use std::error;
use std::path::Path;
use std::process::exit;

fn check_for_overwrite(outfile: &str, matches: &ArgMatches) {
    let force = matches.is_present("force");
    if !force && Path::new(outfile).exists() {
        eprintln!("Refusing to overwrite {} without --force flag", outfile);
        exit(1);
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("new")
                .about("Generate new keypair file")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("PATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                )
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                ),
        )
        .subcommand(
            SubCommand::with_name("pubkey")
                .about("Display the pubkey from a keypair file")
                .setting(AppSettings::DisableVersion)
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
                )
                .arg(
                    Arg::with_name("force")
                        .short("f")
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("pubkey", Some(matches)) => {
            let mut path = dirs::home_dir().expect("home directory");
            let infile = if matches.is_present("infile") {
                matches.value_of("infile").unwrap()
            } else {
                path.extend(&[".config", "solana", "id.json"]);
                path.to_str().unwrap()
            };
            let keypair = read_keypair(infile)?;

            if matches.is_present("outfile") {
                let outfile = matches.value_of("outfile").unwrap();
                check_for_overwrite(&outfile, &matches);
                write_pubkey(outfile, keypair.pubkey())?;
            } else {
                println!("{}", keypair.pubkey());
            }
        }
        ("new", Some(matches)) => {
            let mut path = dirs::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile").unwrap()
            } else {
                path.extend(&[".config", "solana", "id.json"]);
                path.to_str().unwrap()
            };

            if outfile != "-" {
                check_for_overwrite(&outfile, &matches);
            }
            let serialized_keypair = gen_keypair_file(outfile)?;
            if outfile == "-" {
                println!("{}", serialized_keypair);
            } else {
                println!("Wrote {}", outfile);
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}
