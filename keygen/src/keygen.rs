use bip39::{Language, Mnemonic, MnemonicType, Seed};
use bs58;
use clap::{
    crate_description, crate_name, value_t, values_t_or_exit, App, AppSettings, Arg, ArgMatches,
    SubCommand,
};
use num_cpus;
use solana_clap_utils::{
    input_validators::is_derivation,
    keypair::{
        check_for_usb, keypair_from_seed_phrase, prompt_passphrase, signer_from_path,
        SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
    DisplayError,
};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::{write_pubkey_file, Pubkey},
    signature::{keypair_from_seed, write_keypair, write_keypair_file, Keypair, Signer},
};
use std::{
    collections::HashSet,
    error,
    path::Path,
    process::exit,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Instant,
};

const NO_PASSPHRASE: &str = "";

struct GrindMatch {
    starts: String,
    ends: String,
    count: AtomicU64,
}

fn check_for_overwrite(outfile: &str, matches: &ArgMatches) {
    let force = matches.is_present("force");
    if !force && Path::new(outfile).exists() {
        eprintln!("Refusing to overwrite {} without --force flag", outfile);
        exit(1);
    }
}

fn get_keypair_from_matches(
    matches: &ArgMatches,
    config: Config,
    wallet_manager: Option<Arc<RemoteWalletManager>>,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let mut path = dirs::home_dir().expect("home directory");
    let path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else if config.keypair_path != "" {
        &config.keypair_path
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };
    signer_from_path(matches, path, "pubkey recovery", wallet_manager.as_ref())
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

fn grind_validator_starts_with(v: String) -> Result<(), String> {
    if v.matches(':').count() != 1 || (v.starts_with(':') || v.ends_with(':')) {
        return Err(String::from("Expected : between PREFIX and COUNT"));
    }
    let args: Vec<&str> = v.split(':').collect();
    bs58::decode(&args[0])
        .into_vec()
        .map_err(|err| format!("{}: {:?}", args[0], err))?;
    let count = args[1].parse::<u64>();
    if count.is_err() || count.unwrap() == 0 {
        return Err(String::from("Expected COUNT to be of type u64"));
    }
    Ok(())
}

fn grind_validator_ends_with(v: String) -> Result<(), String> {
    if v.matches(':').count() != 1 || (v.starts_with(':') || v.ends_with(':')) {
        return Err(String::from("Expected : between SUFFIX and COUNT"));
    }
    let args: Vec<&str> = v.split(':').collect();
    bs58::decode(&args[0])
        .into_vec()
        .map_err(|err| format!("{}: {:?}", args[0], err))?;
    let count = args[1].parse::<u64>();
    if count.is_err() || count.unwrap() == 0 {
        return Err(String::from("Expected COUNT to be of type u64"));
    }
    Ok(())
}

fn grind_validator_starts_and_ends_with(v: String) -> Result<(), String> {
    if v.matches(':').count() != 2 || (v.starts_with(':') || v.ends_with(':')) {
        return Err(String::from(
            "Expected : between PREFIX and SUFFIX and COUNT",
        ));
    }
    let args: Vec<&str> = v.split(':').collect();
    bs58::decode(&args[0])
        .into_vec()
        .map_err(|err| format!("{}: {:?}", args[0], err))?;
    bs58::decode(&args[1])
        .into_vec()
        .map_err(|err| format!("{}: {:?}", args[1], err))?;
    let count = args[2].parse::<u64>();
    if count.is_err() || count.unwrap() == 0 {
        return Err(String::from("Expected COUNT to be a u64"));
    }
    Ok(())
}

fn grind_print_info(grind_matches: &[GrindMatch]) {
    println!("Searching with {} threads for:", num_cpus::get());
    for gm in grind_matches {
        let mut msg = Vec::<String>::new();
        if gm.count.load(Ordering::Relaxed) > 1 {
            msg.push("pubkeys".to_string());
            msg.push("start".to_string());
            msg.push("end".to_string());
        } else {
            msg.push("pubkey".to_string());
            msg.push("starts".to_string());
            msg.push("ends".to_string());
        }
        println!(
            "\t{} {} that {} with '{}' and {} with '{}'",
            gm.count.load(Ordering::Relaxed),
            msg[0],
            msg[1],
            gm.starts,
            msg[2],
            gm.ends
        );
    }
}

fn grind_parse_args(
    ignore_case: bool,
    starts_with_args: HashSet<String>,
    ends_with_args: HashSet<String>,
    starts_and_ends_with_args: HashSet<String>,
) -> Vec<GrindMatch> {
    let mut grind_matches = Vec::<GrindMatch>::new();
    for sw in starts_with_args {
        let args: Vec<&str> = sw.split(':').collect();
        grind_matches.push(GrindMatch {
            starts: if ignore_case {
                args[0].to_lowercase()
            } else {
                args[0].to_string()
            },
            ends: "".to_string(),
            count: AtomicU64::new(args[1].parse::<u64>().unwrap()),
        });
    }
    for ew in ends_with_args {
        let args: Vec<&str> = ew.split(':').collect();
        grind_matches.push(GrindMatch {
            starts: "".to_string(),
            ends: if ignore_case {
                args[0].to_lowercase()
            } else {
                args[0].to_string()
            },
            count: AtomicU64::new(args[1].parse::<u64>().unwrap()),
        });
    }
    for swew in starts_and_ends_with_args {
        let args: Vec<&str> = swew.split(':').collect();
        grind_matches.push(GrindMatch {
            starts: if ignore_case {
                args[0].to_lowercase()
            } else {
                args[0].to_string()
            },
            ends: if ignore_case {
                args[1].to_lowercase()
            } else {
                args[1].to_string()
            },
            count: AtomicU64::new(args[2].parse::<u64>().unwrap()),
        });
    }
    grind_print_info(&grind_matches);
    grind_matches
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("FILEPATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *CONFIG_FILE {
                arg.default_value(&config_file)
            } else {
                arg
            }
        })
        .subcommand(
            SubCommand::with_name("verify")
                .about("Verify a keypair can sign and verify a message.")
                .arg(
                    Arg::with_name("pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Public key"),
                )
                .arg(
                    Arg::with_name("keypair")
                        .index(2)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .help("Filepath or URL to a keypair"),
                )
        )
        .subcommand(
            SubCommand::with_name("new")
                .about("Generate new keypair file from a passphrase and random seed phrase")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("FILEPATH")
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
                    Arg::with_name("word_count")
                        .long("word-count")
                        .possible_values(&["12", "15", "18", "21", "24"])
                        .default_value("12")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .help("Specify the number of words that will be present in the generated seed phrase"),
                )
                .arg(
                    Arg::with_name("no_passphrase")
                        .long("no-passphrase")
                        .help("Do not prompt for a passphrase"),
                )
                .arg(
                    Arg::with_name("no_outfile")
                        .long("no-outfile")
                        .conflicts_with_all(&["outfile", "silent"])
                        .help("Only print a seed phrase and pubkey. Do not output a keypair file"),
                )
                .arg(
                    Arg::with_name("silent")
                        .short("s")
                        .long("silent")
                        .help("Do not display seed phrase. Useful when piping output to other programs that prompt for user input, like gpg"),
                )
        )
        .subcommand(
            SubCommand::with_name("grind")
                .about("Grind for vanity keypairs")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("ignore_case")
                        .long("ignore-case")
                        .help("Performs case insensitive matches"),
                )
                .arg(
                    Arg::with_name("starts_with")
                        .long("starts-with")
                        .value_name("PREFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple(true)
                        .validator(grind_validator_starts_with)
                        .help("Saves specified number of keypairs whos public key starts with the indicated prefix\nExample: --starts-with sol:4\nPREFIX type is Base58\nCOUNT type is u64"),
                )
                .arg(
                    Arg::with_name("ends_with")
                        .long("ends-with")
                        .value_name("SUFFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple(true)
                        .validator(grind_validator_ends_with)
                        .help("Saves specified number of keypairs whos public key ends with the indicated suffix\nExample: --ends-with ana:4\nSUFFIX type is Base58\nCOUNT type is u64"),
                )
                .arg(
                    Arg::with_name("starts_and_ends_with")
                        .long("starts-and-ends-with")
                        .value_name("PREFIX:SUFFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple(true)
                        .validator(grind_validator_starts_and_ends_with)
                        .help("Saves specified number of keypairs whos public key starts and ends with the indicated perfix and suffix\nExample: --starts-and-ends-with sol:ana:4\nPREFIX and SUFFIX type is Base58\nCOUNT type is u64"),
                ),
        )
        .subcommand(
            SubCommand::with_name("pubkey")
                .about("Display the pubkey from a keypair file")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("keypair")
                        .index(1)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .help("Filepath or URL to a keypair"),
                )
                .arg(
                    Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                        .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                        .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
                )
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("FILEPATH")
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
                    Arg::with_name("derivation_path")
                        .long("derivation-path")
                        .value_name("ACCOUNT or ACCOUNT/CHANGE")
                        .takes_value(true)
                        .validator(is_derivation)
                        .help("Derivation path to use: m/44'/501'/ACCOUNT'/CHANGE'; default key is device base pubkey: m/44'/501'/0'")
                ),
        )
        .subcommand(
            SubCommand::with_name("recover")
                .about("Recover keypair from seed phrase and passphrase")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("outfile")
                        .short("o")
                        .long("outfile")
                        .value_name("FILEPATH")
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
                    Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                        .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                        .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
                ),

        )
        .get_matches();

    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(matches: &ArgMatches<'_>) -> Result<(), Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };

    let wallet_manager =
        if check_for_usb(std::env::args()) || check_for_usb([config.keypair_path.clone()].iter()) {
            maybe_wallet_manager()?
        } else {
            None
        };

    match matches.subcommand() {
        ("pubkey", Some(matches)) => {
            let pubkey = get_keypair_from_matches(matches, config, wallet_manager)?.try_pubkey()?;

            if matches.is_present("outfile") {
                let outfile = matches.value_of("outfile").unwrap();
                check_for_overwrite(&outfile, &matches);
                write_pubkey_file(outfile, pubkey)?;
            } else {
                println!("{}", pubkey);
            }
        }
        ("new", Some(matches)) => {
            let mut path = dirs::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile")
            } else if matches.is_present("no_outfile") {
                None
            } else {
                path.extend(&[".config", "solana", "id.json"]);
                Some(path.to_str().unwrap())
            };

            match outfile {
                Some("-") => (),
                Some(outfile) => check_for_overwrite(&outfile, &matches),
                None => (),
            }

            let word_count = value_t!(matches.value_of("word_count"), usize).unwrap();
            let mnemonic_type = MnemonicType::for_word_count(word_count)?;
            let mnemonic = Mnemonic::new(mnemonic_type, Language::English);
            let passphrase = if matches.is_present("no_passphrase") {
                NO_PASSPHRASE.to_string()
            } else {
                eprintln!("Generating a new keypair");
                prompt_passphrase(
                    "For added security, enter a passphrase (empty for no passphrase): ",
                )?
            };
            let seed = Seed::new(&mnemonic, &passphrase);
            let keypair = keypair_from_seed(seed.as_bytes())?;

            if let Some(outfile) = outfile {
                output_keypair(&keypair, &outfile, "new")
                    .map_err(|err| format!("Unable to write {}: {}", outfile, err))?;
            }

            let silent = matches.is_present("silent");
            if !silent {
                let phrase: &str = mnemonic.phrase();
                let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                eprintln!(
                    "{}\npubkey: {}\n{}\nSave this seed phrase to recover your new keypair:\n{}\n{}",
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

            let skip_validation = matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
            let keypair = keypair_from_seed_phrase("recover", skip_validation, true)?;
            output_keypair(&keypair, &outfile, "recovered")?;
        }
        ("grind", Some(matches)) => {
            let ignore_case = matches.is_present("ignore_case");

            let starts_with_args = if matches.is_present("starts_with") {
                values_t_or_exit!(matches, "starts_with", String)
                    .into_iter()
                    .map(|s| if ignore_case { s.to_lowercase() } else { s })
                    .collect()
            } else {
                HashSet::new()
            };
            let ends_with_args = if matches.is_present("ends_with") {
                values_t_or_exit!(matches, "ends_with", String)
                    .into_iter()
                    .map(|s| if ignore_case { s.to_lowercase() } else { s })
                    .collect()
            } else {
                HashSet::new()
            };
            let starts_and_ends_with_args = if matches.is_present("starts_and_ends_with") {
                values_t_or_exit!(matches, "starts_and_ends_with", String)
                    .into_iter()
                    .map(|s| if ignore_case { s.to_lowercase() } else { s })
                    .collect()
            } else {
                HashSet::new()
            };

            if starts_with_args.is_empty()
                && ends_with_args.is_empty()
                && starts_and_ends_with_args.is_empty()
            {
                eprintln!(
                    "Error: No keypair search criteria provided (--starts-with or --ends-with or --starts-and-ends-with)"
                );
                exit(1);
            }

            let grind_matches = grind_parse_args(
                ignore_case,
                starts_with_args,
                ends_with_args,
                starts_and_ends_with_args,
            );

            let grind_matches_thread_safe = Arc::new(grind_matches);
            let attempts = Arc::new(AtomicU64::new(1));
            let found = Arc::new(AtomicU64::new(0));
            let start = Instant::now();
            let done = Arc::new(AtomicBool::new(false));

            let thread_handles: Vec<_> = (0..num_cpus::get())
                .map(|_| {
                    let done = done.clone();
                    let attempts = attempts.clone();
                    let found = found.clone();
                    let grind_matches_thread_safe = grind_matches_thread_safe.clone();

                    thread::spawn(move || loop {
                        if done.load(Ordering::Relaxed) {
                            break;
                        }
                        let attempts = attempts.fetch_add(1, Ordering::Relaxed);
                        if attempts % 1_000_000 == 0 {
                            println!(
                                "Searched {} keypairs in {}s. {} matches found.",
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
                        let mut total_matches_found = 0;
                        for i in 0..grind_matches_thread_safe.len() {
                            if grind_matches_thread_safe[i].count.load(Ordering::Relaxed) == 0 {
                                total_matches_found += 1;
                                continue;
                            }
                            if (!grind_matches_thread_safe[i].starts.is_empty()
                                && grind_matches_thread_safe[i].ends.is_empty()
                                && pubkey.starts_with(&grind_matches_thread_safe[i].starts))
                                || (grind_matches_thread_safe[i].starts.is_empty()
                                    && !grind_matches_thread_safe[i].ends.is_empty()
                                    && pubkey.ends_with(&grind_matches_thread_safe[i].ends))
                                || (!grind_matches_thread_safe[i].starts.is_empty()
                                    && !grind_matches_thread_safe[i].ends.is_empty()
                                    && pubkey.starts_with(&grind_matches_thread_safe[i].starts)
                                    && pubkey.ends_with(&grind_matches_thread_safe[i].ends))
                            {
                                let _found = found.fetch_add(1, Ordering::Relaxed);
                                grind_matches_thread_safe[i]
                                    .count
                                    .fetch_sub(1, Ordering::Relaxed);
                                println!(
                                    "Wrote keypair to {}",
                                    &format!("{}.json", keypair.pubkey())
                                );
                                write_keypair_file(&keypair, &format!("{}.json", keypair.pubkey()))
                                    .unwrap();
                            }
                        }
                        if total_matches_found == grind_matches_thread_safe.len() {
                            done.store(true, Ordering::Relaxed);
                        }
                    })
                })
                .collect();

            for thread_handle in thread_handles {
                thread_handle.join().unwrap();
            }
        }
        ("verify", Some(matches)) => {
            let keypair = get_keypair_from_matches(matches, config, wallet_manager)?;
            let simple_message = Message::new(&[Instruction::new(
                Pubkey::default(),
                &0,
                vec![AccountMeta::new(keypair.pubkey(), true)],
            )])
            .serialize();
            let signature = keypair.try_sign_message(&simple_message)?;
            let pubkey_bs58 = matches.value_of("pubkey").unwrap();
            let pubkey = bs58::decode(pubkey_bs58).into_vec().unwrap();
            if signature.verify(&pubkey, &simple_message) {
                println!("Verification for public key: {}: Success", pubkey_bs58);
            } else {
                println!("Verification for public key: {}: Failed", pubkey_bs58);
                exit(1);
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}
