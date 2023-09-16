use {
    crate::{
        input_parsers::value_of,
        keypair::{
            keypair_from_seed_phrase, pubkey_from_path, resolve_signer_from_path, signer_from_path,
            ASK_KEYWORD, SKIP_SEED_PHRASE_VALIDATION_ARG,
        },
    },
    clap::ArgMatches,
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_sdk::{
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signature, Signer},
    },
    std::{error, rc::Rc, str::FromStr},
};

// Sentinel value used to indicate to write to screen instead of file
pub const STDOUT_OUTFILE_TOKEN: &str = "-";

// Return the keypair for an argument with filename `name` or None if not present.
pub fn keypair_of(matches: &ArgMatches, name: &str) -> Option<Keypair> {
    if let Some(value) = matches.value_of(name) {
        if value == ASK_KEYWORD {
            let skip_validation = matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
            keypair_from_seed_phrase(name, skip_validation, true, None, true).ok()
        } else {
            read_keypair_file(value).ok()
        }
    } else {
        None
    }
}

// Return the keypair for an argument with filename `name` or `None` if not present wrapped inside `Result`.
pub fn try_keypair_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Keypair>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(keypair_of(matches, name))
}

pub fn keypairs_of(matches: &ArgMatches, name: &str) -> Option<Vec<Keypair>> {
    matches.values_of(name).map(|values| {
        values
            .filter_map(|value| {
                if value == ASK_KEYWORD {
                    let skip_validation = matches.is_present(SKIP_SEED_PHRASE_VALIDATION_ARG.name);
                    keypair_from_seed_phrase(name, skip_validation, true, None, true).ok()
                } else {
                    read_keypair_file(value).ok()
                }
            })
            .collect()
    })
}

pub fn try_keypairs_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Vec<Keypair>>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(keypairs_of(matches, name))
}

// Return a pubkey for an argument that can itself be parsed into a pubkey,
// or is a filename that can be read as a keypair
pub fn pubkey_of(matches: &ArgMatches, name: &str) -> Option<Pubkey> {
    value_of(matches, name).or_else(|| keypair_of(matches, name).map(|keypair| keypair.pubkey()))
}

// Return a `Result` wrapped pubkey for an argument that can itself be parsed into a pubkey,
// or is a filename that can be read as a keypair
pub fn try_pubkey_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Pubkey>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(pubkey_of(matches, name))
}

pub fn pubkeys_of(matches: &ArgMatches, name: &str) -> Option<Vec<Pubkey>> {
    matches.values_of(name).map(|values| {
        values
            .map(|value| {
                value.parse::<Pubkey>().unwrap_or_else(|_| {
                    read_keypair_file(value)
                        .expect("read_keypair_file failed")
                        .pubkey()
                })
            })
            .collect()
    })
}

pub fn try_pubkeys_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Vec<Pubkey>>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(pubkeys_of(matches, name))
}

// Return pubkey/signature pairs for a string of the form pubkey=signature
pub fn pubkeys_sigs_of(matches: &ArgMatches, name: &str) -> Option<Vec<(Pubkey, Signature)>> {
    matches.values_of(name).map(|values| {
        values
            .map(|pubkey_signer_string| {
                let mut signer = pubkey_signer_string.split('=');
                let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
                let sig = Signature::from_str(signer.next().unwrap()).unwrap();
                (key, sig)
            })
            .collect()
    })
}

// Return pubkey/signature pairs for a string of the form pubkey=signature wrapped inside `Result`
#[allow(clippy::type_complexity)]
pub fn try_pubkeys_sigs_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Vec<(Pubkey, Signature)>>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(pubkeys_sigs_of(matches, name))
}

// Return a signer from matches at `name`
#[allow(clippy::type_complexity)]
pub fn signer_of(
    matches: &ArgMatches,
    name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<(Option<Box<dyn Signer>>, Option<Pubkey>), Box<dyn std::error::Error>> {
    if let Some(location) = matches.try_get_one::<String>(name)? {
        let signer = signer_from_path(matches, location, name, wallet_manager)?;
        let signer_pubkey = signer.pubkey();
        Ok((Some(signer), Some(signer_pubkey)))
    } else {
        Ok((None, None))
    }
}

pub fn pubkey_of_signer(
    matches: &ArgMatches,
    name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Option<Pubkey>, Box<dyn std::error::Error>> {
    if let Some(location) = matches.try_get_one::<String>(name)? {
        Ok(Some(pubkey_from_path(
            matches,
            location,
            name,
            wallet_manager,
        )?))
    } else {
        Ok(None)
    }
}

pub fn pubkeys_of_multiple_signers(
    matches: &ArgMatches,
    name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Option<Vec<Pubkey>>, Box<dyn std::error::Error>> {
    if let Some(pubkey_matches) = matches.try_get_many::<String>(name)? {
        let mut pubkeys: Vec<Pubkey> = vec![];
        for signer in pubkey_matches {
            pubkeys.push(pubkey_from_path(matches, signer, name, wallet_manager)?);
        }
        Ok(Some(pubkeys))
    } else {
        Ok(None)
    }
}

pub fn resolve_signer(
    matches: &ArgMatches,
    name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    resolve_signer_from_path(
        matches,
        matches.try_get_one::<String>(name)?.unwrap(),
        name,
        wallet_manager,
    )
}
