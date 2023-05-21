use {
    bip39::{Mnemonic, MnemonicType, Seed},
    clap::{crate_description, crate_name, Arg, ArgMatches, Command},
    solana_clap_v3_utils::{
        input_parsers::STDOUT_OUTFILE_TOKEN,
        keygen::{
            check_for_overwrite,
            mnemonic::{acquire_language, acquire_passphrase_and_message, WORD_COUNT_ARG},
            no_outfile_arg, KeyGenerationCommonArgs, NO_OUTFILE_ARG,
        },
        DisplayError,
    },
    solana_sdk::signer::EncodableKey,
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
            let key_type = match matches.value_of("type").unwrap() {
                "elgamal" => KeyType::ElGamal,
                "aes128" => KeyType::Aes128,
                _ => unreachable!(),
            };

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
