use crate::ArgConstant;
use bip39::{Language, Mnemonic, Seed};
use clap::values_t;
use rpassword::prompt_password_stderr;
use solana_sdk::signature::{
    keypair_from_seed, keypair_from_seed_phrase_and_passphrase, read_keypair_file, Keypair,
    KeypairUtil,
};
use std::error;

// Keyword used to indicate that the user should be asked for a keypair seed phrase
pub const ASK_KEYWORD: &str = "ASK";

pub const ASK_SEED_PHRASE_ARG: ArgConstant<'static> = ArgConstant {
    long: "ask-seed-phrase",
    name: "ask_seed_phrase",
    help: "Securely recover a keypair using a seed phrase and optional passphrase",
};

pub const SKIP_SEED_PHRASE_VALIDATION_ARG: ArgConstant<'static> = ArgConstant {
    long: "skip-seed-phrase-validation",
    name: "skip_seed_phrase_validation",
    help: "Skip validation of seed phrases. Use this if your phrase does not use the BIP39 official English word list",
};

#[derive(Debug, PartialEq)]
pub enum Source {
    File,
    Generated,
    SeedPhrase,
}

pub struct KeypairWithSource {
    pub keypair: Keypair,
    pub source: Source,
}

impl KeypairWithSource {
    fn new(keypair: Keypair, source: Source) -> Self {
        Self { keypair, source }
    }
}

/// Reads user input from stdin to retrieve a seed phrase and passphrase for keypair derivation
pub(crate) fn keypair_from_seed_phrase(
    keypair_name: &str,
    skip_validation: bool,
) -> Result<Keypair, Box<dyn error::Error>> {
    let seed_phrase = prompt_password_stderr(&format!("[{}] seed phrase: ", keypair_name))?;
    let seed_phrase = seed_phrase.trim();
    let passphrase_prompt = format!(
        "[{}] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue: ",
        keypair_name,
    );

    if skip_validation {
        let passphrase = prompt_password_stderr(&passphrase_prompt)?;
        keypair_from_seed_phrase_and_passphrase(&seed_phrase, &passphrase)
    } else {
        let mnemonic = Mnemonic::from_phrase(seed_phrase, Language::English)?;
        let passphrase = prompt_password_stderr(&passphrase_prompt)?;
        let seed = Seed::new(&mnemonic, &passphrase);
        keypair_from_seed(seed.as_bytes())
    }
}

/// Checks CLI arguments to determine whether a keypair should be:
///   - inputted securely via stdin,
///   - read in from a file,
///   - or newly generated
pub fn keypair_input(
    matches: &clap::ArgMatches,
    keypair_name: &str,
) -> Result<KeypairWithSource, Box<dyn error::Error>> {
    let ask_seed_phrase_matches =
        values_t!(matches.values_of(ASK_SEED_PHRASE_ARG.name), String).unwrap_or_default();
    let keypair_match_name = keypair_name.replace('-', "_");
    if ask_seed_phrase_matches
        .iter()
        .any(|s| s.as_str() == keypair_name)
    {
        if matches.value_of(keypair_match_name).is_some() {
            clap::Error::with_description(
                &format!(
                    "`--{} {}` cannot be used with `{} <PATH>`",
                    ASK_SEED_PHRASE_ARG.long, keypair_name, keypair_name
                ),
                clap::ErrorKind::ArgumentConflict,
            )
            .exit();
        }

        let skip_validation = matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
        keypair_from_seed_phrase(keypair_name, skip_validation)
            .map(|keypair| KeypairWithSource::new(keypair, Source::SeedPhrase))
    } else if let Some(keypair_file) = matches.value_of(keypair_match_name) {
        read_keypair_file(keypair_file).map(|keypair| KeypairWithSource::new(keypair, Source::File))
    } else {
        Ok(KeypairWithSource::new(Keypair::new(), Source::Generated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ArgMatches;

    #[test]
    fn test_keypair_input() {
        let arg_matches = ArgMatches::default();
        let KeypairWithSource { source, .. } = keypair_input(&arg_matches, "").unwrap();
        assert_eq!(source, Source::Generated);
    }
}
