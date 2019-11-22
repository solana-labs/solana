use bip39::{Language, Mnemonic, Seed};
use clap::values_t;
use rpassword::prompt_password_stderr;
use solana_sdk::signature::{
    keypair_from_mnemonic_and_passphrase, keypair_from_seed, read_keypair_file, Keypair,
    KeypairUtil,
};
use std::error;

pub const ASK_MNEMONIC_ARG: &str = "ask_mnemonic";
pub const SKIP_MNEMONIC_VALIDATION_ARG: &str = "skip_mnemonic_validation";

/// Reads user input from stdin to retrieve a mnemonic and passphrase for keypair derivation
pub fn keypair_from_mnemonic(
    keypair_name: &str,
    skip_validation: bool,
) -> Result<Keypair, Box<dyn error::Error>> {
    let mnemonic_phrase = prompt_password_stderr(&format!("[{}] mnemonic phrase: ", keypair_name))?;
    let mnemonic_phrase = mnemonic_phrase.trim();

    if skip_validation {
        let passphrase =
            prompt_password_stderr(&format!("[{}] (optional) passphrase: ", keypair_name))?;
        keypair_from_mnemonic_and_passphrase(&mnemonic_phrase, &passphrase)
    } else {
        let mnemonic = Mnemonic::from_phrase(mnemonic_phrase, Language::English)?;
        let passphrase =
            prompt_password_stderr(&format!("[{}] (optional) passphrase: ", keypair_name))?;
        let seed = Seed::new(&mnemonic, &passphrase);
        keypair_from_seed(seed.as_bytes())
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
        values_t!(matches.values_of(ASK_MNEMONIC_ARG), String).unwrap_or_default();
    let keypair_match_name = keypair_name.replace('-', "_");
    if mnemonic_matches.iter().any(|s| s.as_str() == keypair_name) {
        if matches.value_of(keypair_match_name).is_some() {
            let ask_mnemonic_kebab = ASK_MNEMONIC_ARG.replace('_', "-");
            clap::Error::with_description(
                &format!(
                    "`--{} {}` cannot be used with `{} <PATH>`",
                    ask_mnemonic_kebab, keypair_name, keypair_name
                ),
                clap::ErrorKind::ArgumentConflict,
            )
            .exit();
        }

        let skip_validation = matches.is_present(SKIP_MNEMONIC_VALIDATION_ARG);
        keypair_from_mnemonic(keypair_name, skip_validation).map(|keypair| (keypair, false))
    } else if let Some(keypair_file) = matches.value_of(keypair_match_name) {
        read_keypair_file(keypair_file).map(|keypair| (keypair, false))
    } else {
        Ok((Keypair::new(), true))
    }
}
