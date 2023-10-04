use {
    crate::{
        input_parsers::{keypair_of, keypairs_of, pubkey_of, pubkeys_of},
        keypair::{pubkey_from_path, resolve_signer_from_path, signer_from_path},
    },
    clap::ArgMatches,
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
    },
    std::{error, rc::Rc, str::FromStr},
};

// Sentinel value used to indicate to write to screen instead of file
pub const STDOUT_OUTFILE_TOKEN: &str = "-";

// Return the keypair for an argument with filename `name` or `None` if not present wrapped inside `Result`.
pub fn try_keypair_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Keypair>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(keypair_of(matches, name))
}

pub fn try_keypairs_of(
    matches: &ArgMatches,
    name: &str,
) -> Result<Option<Vec<Keypair>>, Box<dyn error::Error>> {
    matches.try_contains_id(name)?;
    Ok(keypairs_of(matches, name))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PubkeySignature {
    pubkey: Pubkey,
    signature: Signature,
}
impl FromStr for PubkeySignature {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut signer = s.split('=');
        let pubkey = signer
            .next()
            .ok_or_else(|| String::from("Malformed signer string"))?;
        let pubkey = Pubkey::from_str(pubkey).map_err(|err| format!("{err}"))?;

        let signature = signer
            .next()
            .ok_or_else(|| String::from("Malformed signer string"))?;
        let signature = Signature::from_str(signature).map_err(|err| format!("{err}"))?;

        Ok(Self { pubkey, signature })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        clap::{Arg, Command},
        solana_sdk::signature::write_keypair_file,
        std::fs,
    };

    fn app<'ab>() -> Command<'ab> {
        Command::new("test")
            .arg(
                Arg::new("multiple")
                    .long("multiple")
                    .takes_value(true)
                    .multiple_occurrences(true)
                    .multiple_values(true),
            )
            .arg(Arg::new("single").takes_value(true).long("single"))
            .arg(Arg::new("unit").takes_value(true).long("unit"))
    }

    fn tmp_file_path(name: &str, pubkey: &Pubkey) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());

        format!("{out_dir}/tmp/{name}-{pubkey}")
    }

    #[test]
    fn test_keypair_of() {
        let keypair = Keypair::new();
        let outfile = tmp_file_path("test_keypair_of.json", &keypair.pubkey());
        let _ = write_keypair_file(&keypair, &outfile).unwrap();

        let matches = app().get_matches_from(vec!["test", "--single", &outfile]);
        assert_eq!(
            keypair_of(&matches, "single").unwrap().pubkey(),
            keypair.pubkey()
        );
        assert!(keypair_of(&matches, "multiple").is_none());

        let matches = app().get_matches_from(vec!["test", "--single", "random_keypair_file.json"]);
        assert!(keypair_of(&matches, "single").is_none());

        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_pubkey_of() {
        let keypair = Keypair::new();
        let outfile = tmp_file_path("test_pubkey_of.json", &keypair.pubkey());
        let _ = write_keypair_file(&keypair, &outfile).unwrap();

        let matches = app().get_matches_from(vec!["test", "--single", &outfile]);
        assert_eq!(pubkey_of(&matches, "single"), Some(keypair.pubkey()));
        assert_eq!(pubkey_of(&matches, "multiple"), None);

        let matches =
            app().get_matches_from(vec!["test", "--single", &keypair.pubkey().to_string()]);
        assert_eq!(pubkey_of(&matches, "single"), Some(keypair.pubkey()));

        let matches = app().get_matches_from(vec!["test", "--single", "random_keypair_file.json"]);
        assert_eq!(pubkey_of(&matches, "single"), None);

        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_pubkeys_of() {
        let keypair = Keypair::new();
        let outfile = tmp_file_path("test_pubkeys_of.json", &keypair.pubkey());
        let _ = write_keypair_file(&keypair, &outfile).unwrap();

        let matches = app().get_matches_from(vec![
            "test",
            "--multiple",
            &keypair.pubkey().to_string(),
            "--multiple",
            &outfile,
        ]);
        assert_eq!(
            pubkeys_of(&matches, "multiple"),
            Some(vec![keypair.pubkey(), keypair.pubkey()])
        );
        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_pubkeys_sigs_of() {
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let sig1 = Keypair::new().sign_message(&[0u8]);
        let sig2 = Keypair::new().sign_message(&[1u8]);
        let signer1 = format!("{key1}={sig1}");
        let signer2 = format!("{key2}={sig2}");
        let matches =
            app().get_matches_from(vec!["test", "--multiple", &signer1, "--multiple", &signer2]);
        assert_eq!(
            pubkeys_sigs_of(&matches, "multiple"),
            Some(vec![(key1, sig1), (key2, sig2)])
        );
    }

    #[test]
    fn test_parse_pubkey_signature() {
        let command = Command::new("test").arg(
            Arg::new("pubkeysig")
                .long("pubkeysig")
                .takes_value(true)
                .value_parser(clap::value_parser!(PubkeySignature)),
        );

        // success case
        let matches = command
            .clone()
            .try_get_matches_from(vec![
                "test",
                "--pubkeysig",
                "11111111111111111111111111111111=4TpFuec1u4BZfxgHg2VQXwvBHANZuNSJHmgrU34GViLAM5uYZ8t7uuhWMHN4k9r41B2p9mwnHjPGwTmTxyvCZw63"
                ]
            )
            .unwrap();

        let expected = PubkeySignature {
            pubkey: Pubkey::from_str("11111111111111111111111111111111").unwrap(),
            signature: Signature::from_str("4TpFuec1u4BZfxgHg2VQXwvBHANZuNSJHmgrU34GViLAM5uYZ8t7uuhWMHN4k9r41B2p9mwnHjPGwTmTxyvCZw63").unwrap(),
        };

        assert_eq!(
            *matches.get_one::<PubkeySignature>("pubkeysig").unwrap(),
            expected,
        );

        // validation fails
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--pubkeysig", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }
}
