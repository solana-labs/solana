use bip39::{Language, Mnemonic, MnemonicType, Seed};
use bs58;
use clap::{
    crate_description, crate_name, crate_version, values_t_or_exit, App, AppSettings, Arg,
    ArgMatches, SubCommand,
};
use num_cpus;
use solana_sdk::{
    pubkey::write_pubkey_file,
    signature::{
        keypair_from_seed, read_keypair, read_keypair_file, write_keypair, write_keypair_file,
        Keypair, KeypairUtil,
    },
};
use std::{
    collections::HashSet,
    error,
    path::Path,
    process::exit,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Instant,
};

const NO_PASSPHRASE: &str = "";

fn check_for_overwrite(outfile: &str, matches: &ArgMatches) {
    let force = matches.is_present("force");
    if !force && Path::new(outfile).exists() {
        eprintln!("Refusing to overwrite {} without --force flag", outfile);
        exit(1);
    }
}

fn output_keypair(
    keypair: &Keypair,
    outfile: &str,
    source: &str,
) -> Result<(), Box<dyn error::Error>> {
    if outfile == "-" {
        let mut stdout = std::io::stdout();
        write_keypair(&keypair, &mut stdout)?;
    } else {
        write_keypair_file(&keypair, outfile)?;
        eprintln!("Wrote {} keypair to {}", source, outfile);
    }
    Ok(())
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
                )
                .arg(
                    Arg::with_name("silent")
                        .short("s")
                        .long("silent")
                        .help("Do not display mnemonic phrase. Useful when piping output to other programs that prompt for user input, like gpg"),
                )
        )
        .subcommand(
            SubCommand::with_name("grind")
                .about("Grind for vanity keypairs")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("ignore_case")
                        .long("ignore-case")
                        .help("Perform case insensitive matches"),
                )
                .arg(
                    Arg::with_name("includes")
                        .long("includes")
                        .value_name("BASE58")
                        .takes_value(true)
                        .multiple(true)
                        .validator(|value| {
                            bs58::decode(&value).into_vec()
                                .map(|_| ())
                                .map_err(|err| format!("{}: {:?}", value, err))
                        })
                        .help("Save keypair if its public key includes this string\n(may be specified multiple times)"),
                )
                .arg(
                    Arg::with_name("starts_with")
                        .long("starts-with")
                        .value_name("BASE58 PREFIX")
                        .takes_value(true)
                        .multiple(true)
                        .validator(|value| {
                            bs58::decode(&value).into_vec()
                                .map(|_| ())
                                .map_err(|err| format!("{}: {:?}", value, err))
                        })
                        .help("Save keypair if its public key starts with this prefix\n(may be specified multiple times)"),
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
        .subcommand(
            SubCommand::with_name("recover")
                .about("Recover keypair from mnemonic phrase")
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
            let keypair = if infile == "-" {
                let mut stdin = std::io::stdin();
                read_keypair(&mut stdin)?
            } else {
                read_keypair_file(infile)?
            };

            if matches.is_present("outfile") {
                let outfile = matches.value_of("outfile").unwrap();
                check_for_overwrite(&outfile, &matches);
                write_pubkey_file(outfile, keypair.pubkey())?;
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

            let mnemonic = Mnemonic::new(MnemonicType::Words12, Language::English);
            let seed = Seed::new(&mnemonic, NO_PASSPHRASE);
            let keypair = keypair_from_seed(seed.as_bytes())?;

            output_keypair(&keypair, &outfile, "new")?;

            let silent = matches.is_present("silent");
            if !silent {
                let phrase: &str = mnemonic.phrase();
                let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                eprintln!(
                    "{}\npubkey: {}\n{}\nSave this mnemonic phrase to recover your new keypair:\n{}\n{}",
                    &divider, keypair.pubkey(), &divider, phrase, &divider
                );
            }
        }
        ("recover", Some(matches)) => {
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

            let phrase = rpassword::prompt_password_stderr("Mnemonic recovery phrase: ").unwrap();
            let mnemonic = Mnemonic::from_phrase(phrase.trim(), Language::English)?;
            let seed = Seed::new(&mnemonic, NO_PASSPHRASE);
            let keypair = keypair_from_seed(seed.as_bytes())?;

            output_keypair(&keypair, &outfile, "recovered")?;
        }
        ("grind", Some(matches)) => {
            let ignore_case = matches.is_present("ignore-case");
            let includes = if matches.is_present("includes") {
                values_t_or_exit!(matches, "includes", String)
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            };

            let starts_with = if matches.is_present("starts_with") {
                values_t_or_exit!(matches, "starts_with", String)
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            };

            if includes.is_empty() && starts_with.is_empty() {
                eprintln!(
                    "Error: No keypair search criteria provided (--includes or --starts-with)"
                );
                exit(1);
            }

            let attempts = Arc::new(AtomicU64::new(1));
            let found = Arc::new(AtomicU64::new(0));
            let start = Instant::now();

            println!(
                "Searching with {} threads for a pubkey containing {:?} or starting with {:?}",
                num_cpus::get(),
                includes,
                starts_with
            );

            let _threads = (0..num_cpus::get())
                .map(|_| {
                    let attempts = attempts.clone();
                    let found = found.clone();
                    let includes = includes.clone();
                    let starts_with = starts_with.clone();

                    thread::spawn(move || loop {
                        let attempts = attempts.fetch_add(1, Ordering::Relaxed);
                        if attempts % 5_000_000 == 0 {
                            println!(
                                "Searched {} keypairs in {}s. {} matches found",
                                attempts,
                                start.elapsed().as_secs(),
                                found.load(Ordering::Relaxed),
                            );
                        }

                        let keypair = Keypair::new();
                        let mut pubkey = bs58::encode(keypair.pubkey()).into_string();

                        if ignore_case {
                            pubkey = pubkey.to_lowercase();
                        }

                        if starts_with.iter().any(|s| pubkey.starts_with(s))
                            || includes.iter().any(|s| pubkey.contains(s))
                        {
                            let found = found.fetch_add(1, Ordering::Relaxed);
                            output_keypair(
                                &keypair,
                                &format!("{}.json", keypair.pubkey()),
                                &format!("{}", found),
                            )
                            .unwrap();
                        }
                    });
                })
                .collect::<Vec<_>>();
            thread::park();
        }
        _ => unreachable!(),
    }

    Ok(())
}
