#![allow(clippy::arithmetic_side_effects)]
use {
    bip39::{Mnemonic, MnemonicType, Seed},
    clap::{crate_description, crate_name, value_parser, Arg, ArgMatches, Command},
    solana_clap_v3_utils::{
        input_parsers::STDOUT_OUTFILE_TOKEN,
        input_validators::is_prompt_signer_source,
        keygen::{
            check_for_overwrite,
            derivation_path::{acquire_derivation_path, derivation_path_arg},
            mnemonic::{
                acquire_language, acquire_passphrase_and_message, no_passphrase_and_message,
                WORD_COUNT_ARG,
            },
            no_outfile_arg, KeyGenerationCommonArgs, NO_OUTFILE_ARG,
        },
        keypair::{
            keypair_from_path, keypair_from_seed_phrase, signer_from_path,
            SKIP_SEED_PHRASE_VALIDATION_ARG,
        },
        DisplayError,
    },
    solana_cli_config::{Config, CONFIG_FILE},
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::{write_pubkey_file, Pubkey},
        signature::{
            keypair_from_seed, keypair_from_seed_and_derivation_path, write_keypair,
            write_keypair_file, Keypair, Signer,
        },
    },
    std::{
        collections::HashSet,
        error,
        rc::Rc,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc,
        },
        thread,
        time::Instant,
    },
};

mod smallest_length_44_public_key {
    use solana_sdk::{pubkey, pubkey::Pubkey};

    pub(super) static PUBKEY: Pubkey = pubkey!("21111111111111111111111111111111111111111111");

    #[test]
    fn assert_length() {
        use crate::smallest_length_44_public_key;
        assert_eq!(smallest_length_44_public_key::PUBKEY.to_string().len(), 44);
    }
}

struct GrindMatch {
    starts: String,
    ends: String,
    count: AtomicU64,
}

fn get_keypair_from_matches(
    matches: &ArgMatches,
    config: Config,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let mut path = dirs_next::home_dir().expect("home directory");
    let path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else if !config.keypair_path.is_empty() {
        &config.keypair_path
    } else {
        path.extend([".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };
    signer_from_path(matches, path, "pubkey recovery", wallet_manager)
}

fn output_keypair(
    keypair: &Keypair,
    outfile: &str,
    source: &str,
) -> Result<(), Box<dyn error::Error>> {
    if outfile == STDOUT_OUTFILE_TOKEN {
        let mut stdout = std::io::stdout();
        write_keypair(keypair, &mut stdout)?;
    } else {
        write_keypair_file(keypair, outfile)?;
        println!("Wrote {source} keypair to {outfile}");
    }
    Ok(())
}

fn grind_validator_starts_with(v: &str) -> Result<(), String> {
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

fn grind_validator_ends_with(v: &str) -> Result<(), String> {
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

fn grind_validator_starts_and_ends_with(v: &str) -> Result<(), String> {
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

fn grind_print_info(grind_matches: &[GrindMatch], num_threads: usize) {
    println!("Searching with {num_threads} threads for:");
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
    num_threads: usize,
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
    grind_print_info(&grind_matches, num_threads);
    grind_matches
}

fn app<'a>(num_threads: &'a str, crate_version: &'a str) -> Command<'a> {
    Command::new(crate_name!())
        .about(crate_description!())
        .version(crate_version)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg({
            let arg = Arg::new("config_file")
                .short('C')
                .long("config")
                .value_name("FILEPATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *CONFIG_FILE {
                arg.default_value(config_file)
            } else {
                arg
            }
        })
        .subcommand(
            Command::new("verify")
                .about("Verify a keypair can sign and verify a message.")
                .arg(
                    Arg::new("pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Public key"),
                )
                .arg(
                    Arg::new("keypair")
                        .index(2)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .help("Filepath or URL to a keypair"),
                )
        )
        .subcommand(
            Command::new("new")
                .about("Generate new keypair file from a random seed phrase and optional BIP39 passphrase")
                .disable_version_flag(true)
                .arg(
                    Arg::new("outfile")
                        .short('o')
                        .long("outfile")
                        .value_name("FILEPATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                )
                .arg(
                    Arg::new("silent")
                        .short('s')
                        .long("silent")
                        .help("Do not display seed phrase. Useful when piping output to other programs that prompt for user input, like gpg"),
                )
                .arg(
                    derivation_path_arg()
                )
                .key_generation_common_args()
                .arg(no_outfile_arg()
                    .conflicts_with_all(&["outfile", "silent"])
                )
        )
        .subcommand(
            Command::new("grind")
                .about("Grind for vanity keypairs")
                .disable_version_flag(true)
                .arg(
                    Arg::new("ignore_case")
                        .long("ignore-case")
                        .help("Performs case insensitive matches"),
                )
                .arg(
                    Arg::new("starts_with")
                        .long("starts-with")
                        .value_name("PREFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple_occurrences(true)
                        .multiple_values(true)
                        .validator(grind_validator_starts_with)
                        .help("Saves specified number of keypairs whos public key starts with the indicated prefix\nExample: --starts-with sol:4\nPREFIX type is Base58\nCOUNT type is u64"),
                )
                .arg(
                    Arg::new("ends_with")
                        .long("ends-with")
                        .value_name("SUFFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple_occurrences(true)
                        .multiple_values(true)
                        .validator(grind_validator_ends_with)
                        .help("Saves specified number of keypairs whos public key ends with the indicated suffix\nExample: --ends-with ana:4\nSUFFIX type is Base58\nCOUNT type is u64"),
                )
                .arg(
                    Arg::new("starts_and_ends_with")
                        .long("starts-and-ends-with")
                        .value_name("PREFIX:SUFFIX:COUNT")
                        .number_of_values(1)
                        .takes_value(true)
                        .multiple_occurrences(true)
                        .multiple_values(true)
                        .validator(grind_validator_starts_and_ends_with)
                        .help("Saves specified number of keypairs whos public key starts and ends with the indicated perfix and suffix\nExample: --starts-and-ends-with sol:ana:4\nPREFIX and SUFFIX type is Base58\nCOUNT type is u64"),
                )
                .arg(
                    Arg::new("num_threads")
                        .long("num-threads")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .value_parser(value_parser!(usize))
                        .default_value(num_threads)
                        .help("Specify the number of grind threads"),
                )
                .arg(
                    Arg::new("use_mnemonic")
                        .long("use-mnemonic")
                        .help("Generate using a mnemonic key phrase.  Expect a significant slowdown in this mode"),
                )
                .arg(
                    derivation_path_arg()
                        .requires("use_mnemonic")
                )
                .key_generation_common_args()
                .arg(
                    no_outfile_arg()
                    // Require a seed phrase to avoid generating a keypair
                    // but having no way to get the private key
                    .requires("use_mnemonic")
                )
        )
        .subcommand(
            Command::new("pubkey")
                .about("Display the pubkey from a keypair file")
                .disable_version_flag(true)
                .arg(
                    Arg::new("keypair")
                        .index(1)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .help("Filepath or URL to a keypair"),
                )
                .arg(
                    Arg::new(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                        .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                        .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
                )
                .arg(
                    Arg::new("outfile")
                        .short('o')
                        .long("outfile")
                        .value_name("FILEPATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                )
        )
        .subcommand(
            Command::new("recover")
                .about("Recover keypair from seed phrase and optional BIP39 passphrase")
                .disable_version_flag(true)
                .arg(
                    Arg::new("prompt_signer")
                        .index(1)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .validator(is_prompt_signer_source)
                        .help("`prompt:` URI scheme or `ASK` keyword"),
                )
                .arg(
                    Arg::new("outfile")
                        .short('o')
                        .long("outfile")
                        .value_name("FILEPATH")
                        .takes_value(true)
                        .help("Path to generated file"),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                )
                .arg(
                    Arg::new(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                        .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                        .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
                ),

        )
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let default_num_threads = num_cpus::get().to_string();
    let matches = app(&default_num_threads, solana_version::version!())
        .try_get_matches()
        .unwrap_or_else(|e| e.exit());
    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(matches: &ArgMatches) -> Result<(), Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };

    let mut wallet_manager = None;

    let subcommand = matches.subcommand().unwrap();

    match subcommand {
        ("pubkey", matches) => {
            let pubkey =
                get_keypair_from_matches(matches, config, &mut wallet_manager)?.try_pubkey()?;

            if matches.is_present("outfile") {
                let outfile = matches.value_of("outfile").unwrap();
                check_for_overwrite(outfile, matches)?;
                write_pubkey_file(outfile, pubkey)?;
            } else {
                println!("{pubkey}");
            }
        }
        ("new", matches) => {
            let mut path = dirs_next::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile")
            } else if matches.is_present(NO_OUTFILE_ARG.name) {
                None
            } else {
                path.extend([".config", "solana", "id.json"]);
                Some(path.to_str().unwrap())
            };

            match outfile {
                Some(STDOUT_OUTFILE_TOKEN) => (),
                Some(outfile) => check_for_overwrite(outfile, matches)?,
                None => (),
            }

            let word_count: usize = matches.value_of_t(WORD_COUNT_ARG.name).unwrap();
            let mnemonic_type = MnemonicType::for_word_count(word_count)?;
            let language = acquire_language(matches);

            let silent = matches.is_present("silent");
            if !silent {
                println!("Generating a new keypair");
            }

            let derivation_path = acquire_derivation_path(matches)?;

            let mnemonic = Mnemonic::new(mnemonic_type, language);
            let (passphrase, passphrase_message) = acquire_passphrase_and_message(matches).unwrap();

            let seed = Seed::new(&mnemonic, &passphrase);
            let keypair = match derivation_path {
                Some(_) => keypair_from_seed_and_derivation_path(seed.as_bytes(), derivation_path),
                None => keypair_from_seed(seed.as_bytes()),
            }?;

            if let Some(outfile) = outfile {
                output_keypair(&keypair, outfile, "new")
                    .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
            }

            if !silent {
                let phrase: &str = mnemonic.phrase();
                let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                println!(
                    "{}\npubkey: {}\n{}\nSave this seed phrase{} to recover your new keypair:\n{}\n{}",
                    &divider, keypair.pubkey(), &divider, passphrase_message, phrase, &divider
                );
            }
        }
        ("recover", matches) => {
            let mut path = dirs_next::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile").unwrap()
            } else {
                path.extend([".config", "solana", "id.json"]);
                path.to_str().unwrap()
            };

            if outfile != STDOUT_OUTFILE_TOKEN {
                check_for_overwrite(outfile, matches)?;
            }

            let keypair_name = "recover";
            let keypair = if let Some(path) = matches.value_of("prompt_signer") {
                keypair_from_path(matches, path, keypair_name, true)?
            } else {
                let skip_validation = matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
                keypair_from_seed_phrase(keypair_name, skip_validation, true, None, true)?
            };
            output_keypair(&keypair, outfile, "recovered")?;
        }
        ("grind", matches) => {
            let ignore_case = matches.is_present("ignore_case");

            let starts_with_args = if matches.is_present("starts_with") {
                matches
                    .values_of_t_or_exit::<String>("starts_with")
                    .into_iter()
                    .map(|s| if ignore_case { s.to_lowercase() } else { s })
                    .collect()
            } else {
                HashSet::new()
            };
            let ends_with_args = if matches.is_present("ends_with") {
                matches
                    .values_of_t_or_exit::<String>("ends_with")
                    .into_iter()
                    .map(|s| if ignore_case { s.to_lowercase() } else { s })
                    .collect()
            } else {
                HashSet::new()
            };
            let starts_and_ends_with_args = if matches.is_present("starts_and_ends_with") {
                matches
                    .values_of_t_or_exit::<String>("starts_and_ends_with")
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
                return Err(
                    "Error: No keypair search criteria provided (--starts-with or --ends-with or --starts-and-ends-with)".into()
                );
            }

            let num_threads = *matches.get_one::<usize>("num_threads").unwrap();

            let grind_matches = grind_parse_args(
                ignore_case,
                starts_with_args,
                ends_with_args,
                starts_and_ends_with_args,
                num_threads,
            );

            let use_mnemonic = matches.is_present("use_mnemonic");

            let derivation_path = acquire_derivation_path(matches)?;

            let word_count: usize = matches.value_of_t(WORD_COUNT_ARG.name).unwrap();
            let mnemonic_type = MnemonicType::for_word_count(word_count)?;
            let language = acquire_language(matches);

            let (passphrase, passphrase_message) = if use_mnemonic {
                acquire_passphrase_and_message(matches).unwrap()
            } else {
                no_passphrase_and_message()
            };
            let no_outfile = matches.is_present(NO_OUTFILE_ARG.name);

            // The vast majority of base58 encoded public keys have length 44, but
            // these only encapsulate prefixes 1-9 and A-H.  If the user is searching
            // for a keypair that starts with a prefix of J-Z or a-z, then there is no
            // reason to waste time searching for a keypair that will never match
            let skip_len_44_pubkeys = grind_matches
                .iter()
                .map(|g| {
                    let target_key = if ignore_case {
                        g.starts.to_ascii_uppercase()
                    } else {
                        g.starts.clone()
                    };
                    let target_key =
                        target_key + &(0..44 - g.starts.len()).map(|_| "1").collect::<String>();
                    bs58::decode(target_key).into_vec()
                })
                .filter_map(|s| s.ok())
                .all(|s| s.len() > 32);

            let grind_matches_thread_safe = Arc::new(grind_matches);
            let attempts = Arc::new(AtomicU64::new(1));
            let found = Arc::new(AtomicU64::new(0));
            let start = Instant::now();
            let done = Arc::new(AtomicBool::new(false));

            let thread_handles: Vec<_> = (0..num_threads)
                .map(|_| {
                    let done = done.clone();
                    let attempts = attempts.clone();
                    let found = found.clone();
                    let grind_matches_thread_safe = grind_matches_thread_safe.clone();
                    let passphrase = passphrase.clone();
                    let passphrase_message = passphrase_message.clone();
                    let derivation_path = derivation_path.clone();

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
                        let (keypair, phrase) = if use_mnemonic {
                            let mnemonic = Mnemonic::new(mnemonic_type, language);
                            let seed = Seed::new(&mnemonic, &passphrase);
                            let keypair = match derivation_path {
                                Some(_) => keypair_from_seed_and_derivation_path(seed.as_bytes(), derivation_path.clone()),
                                None => keypair_from_seed(seed.as_bytes()),
                            }.unwrap();
                            (keypair, mnemonic.phrase().to_string())
                        } else {
                            (Keypair::new(), "".to_string())
                        };
                        // Skip keypairs that will never match the user specified prefix
                        if skip_len_44_pubkeys && keypair.pubkey() >= smallest_length_44_public_key::PUBKEY {
                            continue;
                        }
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
                                if !no_outfile {
                                    write_keypair_file(&keypair, &format!("{}.json", keypair.pubkey()))
                                    .unwrap();
                                    println!(
                                        "Wrote keypair to {}",
                                        &format!("{}.json", keypair.pubkey())
                                    );
                                }
                                if use_mnemonic {
                                    let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                                    println!(
                                        "{}\nFound matching key {}",
                                        &divider, keypair.pubkey());
                                    println!(
                                        "\nSave this seed phrase{} to recover your new keypair:\n{}\n{}",
                                        passphrase_message, phrase, &divider
                                    );
                                }
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
        ("verify", matches) => {
            let keypair = get_keypair_from_matches(matches, config, &mut wallet_manager)?;
            let simple_message = Message::new(
                &[Instruction::new_with_bincode(
                    Pubkey::default(),
                    &0,
                    vec![AccountMeta::new(keypair.pubkey(), true)],
                )],
                Some(&keypair.pubkey()),
            )
            .serialize();
            let signature = keypair.try_sign_message(&simple_message)?;
            let pubkey_bs58 = matches.value_of("pubkey").unwrap();
            let pubkey = bs58::decode(pubkey_bs58).into_vec().unwrap();
            if signature.verify(&pubkey, &simple_message) {
                println!("Verification for public key: {pubkey_bs58}: Success");
            } else {
                let err_msg = format!("Verification for public key: {pubkey_bs58}: Failed");
                return Err(err_msg.into());
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        tempfile::{tempdir, TempDir},
    };

    fn process_test_command(args: &[&str]) -> Result<(), Box<dyn error::Error>> {
        let default_num_threads = num_cpus::get().to_string();
        let solana_version = solana_version::version!();
        let app_matches = app(&default_num_threads, solana_version).get_matches_from(args);
        do_main(&app_matches)
    }

    fn create_tmp_keypair_and_config_file(
        keypair_out_dir: &TempDir,
        config_out_dir: &TempDir,
    ) -> (Pubkey, String, String) {
        let keypair = Keypair::new();
        let keypair_path = keypair_out_dir
            .path()
            .join(format!("{}-keypair", keypair.pubkey()));
        let keypair_outfile = keypair_path.into_os_string().into_string().unwrap();
        write_keypair_file(&keypair, &keypair_outfile).unwrap();

        let config = Config {
            keypair_path: keypair_outfile.clone(),
            ..Config::default()
        };
        let config_path = config_out_dir
            .path()
            .join(format!("{}-config", keypair.pubkey()));
        let config_outfile = config_path.into_os_string().into_string().unwrap();
        config.save(&config_outfile).unwrap();

        (keypair.pubkey(), keypair_outfile, config_outfile)
    }

    fn tmp_outfile_path(out_dir: &TempDir, name: &str) -> String {
        let path = out_dir.path().join(name);
        path.into_os_string().into_string().unwrap()
    }

    #[test]
    fn test_arguments() {
        let default_num_threads = num_cpus::get().to_string();
        let solana_version = solana_version::version!();

        // run clap internal assert statements
        app(&default_num_threads, solana_version).debug_assert();
    }

    #[test]
    fn test_verify() {
        let keypair_out_dir = tempdir().unwrap();
        let config_out_dir = tempdir().unwrap();
        let (correct_pubkey, keypair_path, config_path) =
            create_tmp_keypair_and_config_file(&keypair_out_dir, &config_out_dir);

        // success case using a keypair file
        process_test_command(&[
            "solana-keygen",
            "verify",
            &correct_pubkey.to_string(),
            &keypair_path,
        ])
        .unwrap();

        // success case using a config file
        process_test_command(&[
            "solana-keygen",
            "verify",
            &correct_pubkey.to_string(),
            "--config",
            &config_path,
        ])
        .unwrap();

        // fail case using a keypair file
        let incorrect_pubkey = Pubkey::new_unique();
        let result = process_test_command(&[
            "solana-keygen",
            "verify",
            &incorrect_pubkey.to_string(),
            &keypair_path,
        ])
        .unwrap_err()
        .to_string();

        let expected = format!("Verification for public key: {incorrect_pubkey}: Failed");
        assert_eq!(result, expected);

        // fail case using a config file
        process_test_command(&[
            "solana-keygen",
            "verify",
            &incorrect_pubkey.to_string(),
            "--config",
            &config_path,
        ])
        .unwrap_err()
        .to_string();

        let expected = format!("Verification for public key: {incorrect_pubkey}: Failed");
        assert_eq!(result, expected);

        // keypair file takes precedence over config file
        let alt_keypair_out_dir = tempdir().unwrap();
        let alt_config_out_dir = tempdir().unwrap();
        let (_, alt_keypair_path, alt_config_path) =
            create_tmp_keypair_and_config_file(&alt_keypair_out_dir, &alt_config_out_dir);

        process_test_command(&[
            "solana-keygen",
            "verify",
            &correct_pubkey.to_string(),
            &keypair_path,
            "--config",
            &alt_config_path,
        ])
        .unwrap();

        process_test_command(&[
            "solana-keygen",
            "verify",
            &correct_pubkey.to_string(),
            &alt_keypair_path,
            "--config",
            &config_path,
        ])
        .unwrap_err()
        .to_string();

        let expected = format!("Verification for public key: {incorrect_pubkey}: Failed");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_pubkey() {
        let keypair_out_dir = tempdir().unwrap();
        let config_out_dir = tempdir().unwrap();
        let (expected_pubkey, keypair_path, config_path) =
            create_tmp_keypair_and_config_file(&keypair_out_dir, &config_out_dir);

        // success case using a keypair file
        {
            let outfile_dir = tempdir().unwrap();
            let outfile_path = tmp_outfile_path(&outfile_dir, &expected_pubkey.to_string());

            process_test_command(&[
                "solana-keygen",
                "pubkey",
                &keypair_path,
                "--outfile",
                &outfile_path,
            ])
            .unwrap();

            let result_pubkey = solana_sdk::pubkey::read_pubkey_file(&outfile_path).unwrap();
            assert_eq!(result_pubkey, expected_pubkey);
        }

        // success case using a config file
        {
            let outfile_dir = tempdir().unwrap();
            let outfile_path = tmp_outfile_path(&outfile_dir, &expected_pubkey.to_string());

            process_test_command(&[
                "solana-keygen",
                "pubkey",
                "--config",
                &config_path,
                "--outfile",
                &outfile_path,
            ])
            .unwrap();

            let result_pubkey = solana_sdk::pubkey::read_pubkey_file(&outfile_path).unwrap();
            assert_eq!(result_pubkey, expected_pubkey);
        }

        // keypair file takes precedence over config file
        {
            let alt_keypair_out_dir = tempdir().unwrap();
            let alt_config_out_dir = tempdir().unwrap();
            let (_, _, alt_config_path) =
                create_tmp_keypair_and_config_file(&alt_keypair_out_dir, &alt_config_out_dir);
            let outfile_dir = tempdir().unwrap();
            let outfile_path = tmp_outfile_path(&outfile_dir, &expected_pubkey.to_string());

            process_test_command(&[
                "solana-keygen",
                "pubkey",
                &keypair_path,
                "--config",
                &alt_config_path,
                "--outfile",
                &outfile_path,
            ])
            .unwrap();

            let result_pubkey = solana_sdk::pubkey::read_pubkey_file(&outfile_path).unwrap();
            assert_eq!(result_pubkey, expected_pubkey);
        }

        // refuse to overwrite file
        {
            let outfile_dir = tempdir().unwrap();
            let outfile_path = tmp_outfile_path(&outfile_dir, &expected_pubkey.to_string());

            process_test_command(&[
                "solana-keygen",
                "pubkey",
                &keypair_path,
                "--outfile",
                &outfile_path,
            ])
            .unwrap();

            let result = process_test_command(&[
                "solana-keygen",
                "pubkey",
                "--config",
                &config_path,
                "--outfile",
                &outfile_path,
            ])
            .unwrap_err()
            .to_string();

            let expected = format!("Refusing to overwrite {outfile_path} without --force flag");
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn test_new() {
        let keypair_out_dir = tempdir().unwrap();
        let config_out_dir = tempdir().unwrap();
        let (expected_pubkey, _, _) =
            create_tmp_keypair_and_config_file(&keypair_out_dir, &config_out_dir);

        let outfile_dir = tempdir().unwrap();
        let outfile_path = tmp_outfile_path(&outfile_dir, &expected_pubkey.to_string());

        // general success case
        process_test_command(&[
            "solana-keygen",
            "new",
            "--outfile",
            &outfile_path,
            "--no-bip39-passphrase",
        ])
        .unwrap();

        // refuse to overwrite file
        let result = process_test_command(&[
            "solana-keygen",
            "new",
            "--outfile",
            &outfile_path,
            "--no-bip39-passphrase",
        ])
        .unwrap_err()
        .to_string();

        let expected = format!("Refusing to overwrite {outfile_path} without --force flag");
        assert_eq!(result, expected);

        // no outfile
        process_test_command(&[
            "solana-keygen",
            "new",
            "--no-bip39-passphrase",
            "--no-outfile",
        ])
        .unwrap();

        // sanity check on languages and word count combinations
        let languages = [
            "english",
            "chinese-simplified",
            "chinese-traditional",
            "japanese",
            "spanish",
            "korean",
            "french",
            "italian",
        ];
        let word_counts = ["12", "15", "18", "21", "24"];

        for language in languages {
            for word_count in word_counts {
                process_test_command(&[
                    "solana-keygen",
                    "new",
                    "--no-outfile",
                    "--no-bip39-passphrase",
                    "--language",
                    language,
                    "--word-count",
                    word_count,
                ])
                .unwrap();
            }
        }

        // sanity check derivation path
        process_test_command(&[
            "solana-keygen",
            "new",
            "--no-bip39-passphrase",
            "--no-outfile",
            "--derivation-path",
            // empty derivation path
        ])
        .unwrap();

        process_test_command(&[
            "solana-keygen",
            "new",
            "--no-bip39-passphrase",
            "--no-outfile",
            "--derivation-path",
            "m/44'/501'/0'/0'", // default derivation path
        ])
        .unwrap();

        let result = process_test_command(&[
            "solana-keygen",
            "new",
            "--no-bip39-passphrase",
            "--no-outfile",
            "--derivation-path",
            "-", // invalid derivation path
        ])
        .unwrap_err()
        .to_string();

        let expected = "invalid derivation path: invalid prefix: -";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_grind() {
        // simple sanity checks
        process_test_command(&[
            "solana-keygen",
            "grind",
            "--no-outfile",
            "--no-bip39-passphrase",
            "--use-mnemonic",
            "--starts-with",
            "a:1",
        ])
        .unwrap();

        process_test_command(&[
            "solana-keygen",
            "grind",
            "--no-outfile",
            "--no-bip39-passphrase",
            "--use-mnemonic",
            "--ends-with",
            "b:1",
        ])
        .unwrap();
    }
}
