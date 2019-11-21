use bip39::{Language, Mnemonic, Seed};
use clap::values_t;
use rpassword::prompt_password_stderr;
use solana_sdk::signature::{
    keypair_from_mnemonic_and_passphrase, keypair_from_seed, read_keypair_file, Keypair,
    KeypairUtil,
};
use std::{
    error,
    io::{stdin, stdout, Write},
};

/// Reads user input from stdin to retrieve a mnemonic and passphrase for keypair derivation
pub fn keypair_from_mnemonic(keypair_name: &str) -> Result<Keypair, Box<dyn error::Error>> {
    let mnemonic_phrase =
        prompt_password_stderr(&format!("[{}] mnemonic phrase: ", keypair_name)).unwrap();
    match Mnemonic::from_phrase(mnemonic_phrase.trim(), Language::English) {
        Ok(mnemonic) => {
            let passphrase =
                prompt_password_stderr(&format!("[{}] (optional) passphrase: ", keypair_name))
                    .unwrap();
            let seed = Seed::new(&mnemonic, &passphrase);
            keypair_from_seed(seed.as_bytes())
        }
        Err(err) => {
            print!(
                "[{}] mnemonic phrase validation failed, continue anyways? (y/n): ",
                keypair_name
            );
            stdout().flush().unwrap();

            let mut buffer = String::new();
            stdin().read_line(&mut buffer).unwrap();
            if buffer.trim() != "y" {
                return Err(err.into());
            }

            let passphrase =
                prompt_password_stderr(&format!("[{}] (optional) passphrase: ", keypair_name))
                    .unwrap();

            keypair_from_mnemonic_and_passphrase(&mnemonic_phrase, &passphrase)
        }
    }
}

/// Checks CLI arguments to determine whether a keypair should be:
///   - inputted securely via stdin,
///   - read in from a file,
///   - or newly generated
///
/// Returns the keypair result and whether it was generated.
pub fn keypair_input(
    matches: &clap::ArgMatches,
    keypair_name: &str,
) -> Result<(Keypair, bool), Box<dyn error::Error>> {
    let mnemonic_matches =
        values_t!(matches.values_of("mnemonic_stdin"), String).unwrap_or_default();
    let keypair_match_name = keypair_name.replace('-', "_");
    if mnemonic_matches.iter().any(|s| s.as_str() == keypair_name) {
        if matches.value_of(keypair_match_name).is_some() {
            clap::Error::with_description(
                &format!(
                    "`--mnemonic-stdin {}` cannot be used with `{} <PATH>`",
                    keypair_name, keypair_name
                ),
                clap::ErrorKind::ArgumentConflict,
            )
            .exit();
        }

        keypair_from_mnemonic(keypair_name).map(|keypair| (keypair, false))
    } else if let Some(keypair_file) = matches.value_of(keypair_match_name) {
        read_keypair_file(keypair_file).map(|keypair| (keypair, false))
    } else {
        Ok((Keypair::new(), true))
    }
}
