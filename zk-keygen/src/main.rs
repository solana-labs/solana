use {
    bip39::{Mnemonic, MnemonicType, Seed},
    clap::{crate_description, crate_name, Arg, ArgMatches, Command},
    solana_clap_v3_utils::{
        input_parsers::STDOUT_OUTFILE_TOKEN,
        input_validators::is_prompt_signer_source,
        keygen::{
            check_for_overwrite,
            mnemonic::{acquire_language, acquire_passphrase_and_message, WORD_COUNT_ARG},
            no_outfile_arg, KeyGenerationCommonArgs, NO_OUTFILE_ARG,
        },
        keypair::{
            ae_key_from_path, ae_key_from_seed_phrase, elgamal_keypair_from_path,
            elgamal_keypair_from_seed_phrase, SKIP_SEED_PHRASE_VALIDATION_ARG,
        },
        DisplayError,
    },
    solana_sdk::signer::{EncodableKey, SeedDerivable},
    solana_zk_token_sdk::encryption::{auth_encryption::AeKey, elgamal::ElGamalKeypair},
    std::error,
};

fn output_encodable_key<K: EncodableKey>(
    key: &K,
    outfile: &str,
    source: &str,
) -> Result<(), Box<dyn error::Error>> {
    if outfile == STDOUT_OUTFILE_TOKEN {
        let mut stdout = std::io::stdout();
        key.write(&mut stdout)?;
    } else {
        key.write_to_file(outfile)?;
        println!("Wrote {source} to {outfile}");
    }
    Ok(())
}

fn app(crate_version: &str) -> Command {
    Command::new(crate_name!())
        .about(crate_description!())
        .version(crate_version)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("new")
                .about("Generate a new encryption key/keypair file from a random seed phrase and optional BIP39 passphrase")
                .disable_version_flag(true)
                .arg(
                    Arg::new("type")
                        .long("type")
                        .takes_value(true)
                        .possible_values(["elgamal", "aes128"])
                        .value_name("TYPE")
                        .required(true)
                        .help("The type of encryption key")
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
                        .long("force")
                        .help("Overwrite the output file if it exists"),
                )
                .arg(
                    Arg::new("silent")
                        .long("silent")
                        .help("Do not display seed phrase. Useful when piping output to other programs that prompt for user input, like gpg"),
                )
                .key_generation_common_args()
                .arg(no_outfile_arg().conflicts_with_all(&["outfile", "silent"]))
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
                    Arg::new("type")
                        .long("type")
                        .takes_value(true)
                        .possible_values(["elgamal"])
                        .value_name("TYPE")
                        .required(true)
                        .help("The type of keypair")
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
                    Arg::new("type")
                        .long("type")
                        .takes_value(true)
                        .possible_values(["elgamal", "aes128"])
                        .value_name("TYPE")
                        .required(true)
                        .help("The type of keypair")
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
    let matches = app(solana_version::version!())
        .try_get_matches()
        .unwrap_or_else(|e| e.exit());
    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(matches: &ArgMatches) -> Result<(), Box<dyn error::Error>> {
    let subcommand = matches.subcommand().unwrap();
    match subcommand {
        ("new", matches) => {
            let key_type = KeyType::get_key_type(matches);

            let mut path = dirs_next::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile")
            } else if matches.is_present(NO_OUTFILE_ARG.name) {
                None
            } else {
                path.extend([".config", "solana", key_type.default_file_name()]);
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

            let mnemonic = Mnemonic::new(mnemonic_type, language);
            let (passphrase, passphrase_message) = acquire_passphrase_and_message(matches).unwrap();
            let seed = Seed::new(&mnemonic, &passphrase);

            let silent = matches.is_present("silent");

            match key_type {
                KeyType::ElGamal => {
                    if !silent {
                        eprintln!("Generating a new ElGamal keypair");
                    }

                    let elgamal_keypair = ElGamalKeypair::from_seed(seed.as_bytes())?;
                    if let Some(outfile) = outfile {
                        output_encodable_key(&elgamal_keypair, outfile, "new ElGamal keypair")
                            .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
                    }

                    if !silent {
                        let phrase: &str = mnemonic.phrase();
                        let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                        println!(
                            "{}\npubkey: {}\n{}\nSave this seed phrase{} to recover your new ElGamal keypair:\n{}\n{}",
                            &divider, elgamal_keypair.public, &divider, passphrase_message, phrase, &divider
                        );
                    }
                }
                KeyType::Aes128 => {
                    if !silent {
                        eprintln!("Generating a new AES128 encryption key");
                    }

                    let aes_key = AeKey::from_seed(seed.as_bytes())?;
                    if let Some(outfile) = outfile {
                        output_encodable_key(&aes_key, outfile, "new AES128 key")
                            .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
                    }

                    if !silent {
                        let phrase: &str = mnemonic.phrase();
                        let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
                        println!(
                            "{}\nSave this seed phrase{} to recover your new AES128 key:\n{}\n{}",
                            &divider, passphrase_message, phrase, &divider
                        );
                    }
                }
            }
        }
        ("pubkey", matches) => {
            let key_type = KeyType::get_key_type(matches);

            let mut path = dirs_next::home_dir().expect("home directory");
            let path = if matches.is_present("keypair") {
                matches.value_of("keypair").unwrap()
            } else {
                path.extend([".config", "solana", key_type.default_file_name()]);
                path.to_str().unwrap()
            };

            // wrap the logic inside a match statement in case more keys are supported in the
            // future
            match key_type {
                KeyType::ElGamal => {
                    let elgamal_pubkey =
                        elgamal_keypair_from_path(matches, path, "pubkey recovery", false)?.public;

                    if matches.is_present("outfile") {
                        let outfile = matches.value_of("outfile").unwrap();
                        check_for_overwrite(outfile, matches)?;
                        elgamal_pubkey.write_to_file(outfile)?;
                    } else {
                        println!("{elgamal_pubkey}");
                    }
                }
                _ => unreachable!(),
            }
        }
        ("recover", matches) => {
            let key_type = KeyType::get_key_type(matches);

            let mut path = dirs_next::home_dir().expect("home directory");
            let outfile = if matches.is_present("outfile") {
                matches.value_of("outfile").unwrap()
            } else {
                path.extend([".config", "solana", key_type.default_file_name()]);
                path.to_str().unwrap()
            };

            if outfile != STDOUT_OUTFILE_TOKEN {
                check_for_overwrite(outfile, matches)?;
            }

            match key_type {
                KeyType::ElGamal => {
                    let keypair_name = "recover";
                    let keypair = if let Some(path) = matches.value_of("prompt_signer") {
                        elgamal_keypair_from_path(matches, path, keypair_name, true)?
                    } else {
                        let skip_validation =
                            matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
                        elgamal_keypair_from_seed_phrase(
                            keypair_name,
                            skip_validation,
                            true,
                            None,
                            true,
                        )?
                    };
                    output_encodable_key(&keypair, outfile, "recovered ElGamal keypair")?;
                }
                KeyType::Aes128 => {
                    let key_name = "recover";
                    let key = if let Some(path) = matches.value_of("prompt_signer") {
                        ae_key_from_path(matches, path, key_name)?
                    } else {
                        let skip_validation =
                            matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
                        ae_key_from_seed_phrase(key_name, skip_validation, None, true)?
                    };
                    output_encodable_key(&key, outfile, "recovered AES128 key")?;
                }
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

enum KeyType {
    ElGamal,
    Aes128,
}

impl KeyType {
    fn default_file_name(&self) -> &str {
        match self {
            KeyType::ElGamal => "elgamal.json",
            KeyType::Aes128 => "aes128.json",
        }
    }

    fn get_key_type(matches: &ArgMatches) -> Self {
        match matches.value_of("type").unwrap() {
            "elgamal" => Self::ElGamal,
            "aes128" => Self::Aes128,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::pubkey::Pubkey,
        tempfile::{tempdir, TempDir},
    };

    fn process_test_command(args: &[&str]) -> Result<(), Box<dyn error::Error>> {
        let solana_version = solana_version::version!();
        let app_matches = app(solana_version).get_matches_from(args);
        do_main(&app_matches)
    }

    fn tmp_outfile_path(out_dir: &TempDir, name: &str) -> String {
        let path = out_dir.path().join(name);
        path.into_os_string().into_string().unwrap()
    }

    #[test]
    fn test_arguments() {
        let solana_version = solana_version::version!();

        // run clap internal assert statements
        app(solana_version).debug_assert();
    }

    #[test]
    fn test_new_elgamal() {
        let outfile_dir = tempdir().unwrap();
        // use `Pubkey::new_unique()` to generate names for temporary key files
        let outfile_path = tmp_outfile_path(&outfile_dir, &Pubkey::new_unique().to_string());

        // general success case
        process_test_command(&[
            "solana-zk-keygen",
            "new",
            "--type",
            "elgamal",
            "--outfile",
            &outfile_path,
            "--no-bip39-passphrase",
        ])
        .unwrap();

        // refuse to overwrite file
        let result = process_test_command(&[
            "solana-zk-keygen",
            "new",
            "--type",
            "elgamal",
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
            "--type",
            "elgamal",
            "--no-bip39-passphrase",
            "--no-outfile",
        ])
        .unwrap();
    }

    #[test]
    fn test_new_aes128() {
        let outfile_dir = tempdir().unwrap();
        // use `Pubkey::new_unique()` to generate names for temporary key files
        let outfile_path = tmp_outfile_path(&outfile_dir, &Pubkey::new_unique().to_string());

        // general success case
        process_test_command(&[
            "solana-zk-keygen",
            "new",
            "--type",
            "aes128",
            "--outfile",
            &outfile_path,
            "--no-bip39-passphrase",
        ])
        .unwrap();

        // refuse to overwrite file
        let result = process_test_command(&[
            "solana-zk-keygen",
            "new",
            "--type",
            "aes128",
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
            "--type",
            "aes128",
            "--no-bip39-passphrase",
            "--no-outfile",
        ])
        .unwrap();
    }
}
