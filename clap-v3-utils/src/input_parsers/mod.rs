use {
    crate::{
        input_validators::normalize_to_url_if_moniker,
        keypair::{keypair_from_seed_phrase, ASK_KEYWORD, SKIP_SEED_PHRASE_VALIDATION_ARG},
    },
    chrono::DateTime,
    clap::ArgMatches,
    solana_sdk::{
        clock::UnixTimestamp,
        commitment_config::CommitmentConfig,
        genesis_config::ClusterType,
        native_token::sol_to_lamports,
        pubkey::{Pubkey, MAX_SEED_LEN},
        signature::{read_keypair_file, Keypair, Signer},
    },
    std::str::FromStr,
};

pub mod signer;
#[deprecated(
    since = "1.17.0",
    note = "Please use the functions in `solana_clap_v3_utils::input_parsers::signer` directly instead"
)]
pub use signer::{
    pubkey_of_signer, pubkeys_of_multiple_signers, pubkeys_sigs_of, resolve_signer, signer_of,
    STDOUT_OUTFILE_TOKEN,
};

// Return parsed values from matches at `name`
pub fn values_of<T>(matches: &ArgMatches, name: &str) -> Option<Vec<T>>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .values_of(name)
        .map(|xs| xs.map(|x| x.parse::<T>().unwrap()).collect())
}

// Return a parsed value from matches at `name`
pub fn value_of<T>(matches: &ArgMatches, name: &str) -> Option<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .value_of(name)
        .and_then(|value| value.parse::<T>().ok())
}

pub fn unix_timestamp_from_rfc3339_datetime(
    matches: &ArgMatches,
    name: &str,
) -> Option<UnixTimestamp> {
    matches.value_of(name).and_then(|value| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|date_time| date_time.timestamp())
    })
}

#[deprecated(
    since = "1.17.0",
    note = "please use `Amount::parse_decimal` and `Amount::sol_to_lamport` instead"
)]
pub fn lamports_of_sol(matches: &ArgMatches, name: &str) -> Option<u64> {
    value_of(matches, name).map(sol_to_lamports)
}

pub fn cluster_type_of(matches: &ArgMatches, name: &str) -> Option<ClusterType> {
    value_of(matches, name)
}

pub fn commitment_of(matches: &ArgMatches, name: &str) -> Option<CommitmentConfig> {
    matches
        .value_of(name)
        .map(|value| CommitmentConfig::from_str(value).unwrap_or_default())
}

pub fn parse_url(arg: &str) -> Result<String, String> {
    url::Url::parse(arg)
        .map_err(|err| err.to_string())
        .and_then(|url| {
            url.has_host()
                .then_some(arg.to_string())
                .ok_or("no host provided".to_string())
        })
}

pub fn parse_url_or_moniker(arg: &str) -> Result<String, String> {
    parse_url(&normalize_to_url_if_moniker(arg))
}

pub fn parse_pow2(arg: &str) -> Result<usize, String> {
    arg.parse::<usize>()
        .map_err(|e| format!("Unable to parse, provided: {arg}, err: {e}"))
        .and_then(|v| {
            v.is_power_of_two()
                .then_some(v)
                .ok_or(format!("Must be a power of 2: {v}"))
        })
}

pub fn parse_percentage(arg: &str) -> Result<u8, String> {
    arg.parse::<u8>()
        .map_err(|e| format!("Unable to parse input percentage, provided: {arg}, err: {e}"))
        .and_then(|v| {
            (v <= 100).then_some(v).ok_or(format!(
                "Percentage must be in range of 0 to 100, provided: {v}"
            ))
        })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Amount {
    Decimal(f64),
    Raw(u64),
    All,
}
impl Amount {
    pub fn parse(arg: &str) -> Result<Amount, String> {
        if arg == "ALL" {
            Ok(Amount::All)
        } else {
            Self::parse_decimal(arg).or(Self::parse_raw(arg)
                .map_err(|_| format!("Unable to parse input amount, provided: {arg}")))
        }
    }

    pub fn parse_decimal(arg: &str) -> Result<Amount, String> {
        arg.parse::<f64>()
            .map(Amount::Decimal)
            .map_err(|_| format!("Unable to parse input amount, provided: {arg}"))
    }

    pub fn parse_raw(arg: &str) -> Result<Amount, String> {
        arg.parse::<u64>()
            .map(Amount::Raw)
            .map_err(|_| format!("Unable to parse input amount, provided: {arg}"))
    }

    pub fn parse_decimal_or_all(arg: &str) -> Result<Amount, String> {
        if arg == "ALL" {
            Ok(Amount::All)
        } else {
            Self::parse_decimal(arg).map_err(|_| {
                format!("Unable to parse input amount as float or 'ALL' keyword, provided: {arg}")
            })
        }
    }

    pub fn to_raw_amount(&self, decimals: u8) -> Self {
        match self {
            Amount::Decimal(amount) => {
                Amount::Raw((amount * 10_usize.pow(decimals as u32) as f64) as u64)
            }
            Amount::Raw(amount) => Amount::Raw(*amount),
            Amount::All => Amount::All,
        }
    }

    pub fn sol_to_lamport(&self) -> Amount {
        const NATIVE_SOL_DECIMALS: u8 = 9;
        self.to_raw_amount(NATIVE_SOL_DECIMALS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawTokenAmount {
    Amount(u64),
    All,
}

pub fn parse_rfc3339_datetime(arg: &str) -> Result<String, String> {
    DateTime::parse_from_rfc3339(arg)
        .map(|_| arg.to_string())
        .map_err(|e| format!("{e}"))
}

pub fn parse_derivation(arg: &str) -> Result<String, String> {
    let value = arg.replace('\'', "");
    let mut parts = value.split('/');
    let account = parts.next().unwrap();
    account
        .parse::<u32>()
        .map_err(|e| format!("Unable to parse derivation, provided: {account}, err: {e}"))
        .and_then(|_| {
            if let Some(change) = parts.next() {
                change.parse::<u32>().map_err(|e| {
                    format!("Unable to parse derivation, provided: {change}, err: {e}")
                })
            } else {
                Ok(0)
            }
        })?;
    Ok(arg.to_string())
}

pub fn parse_structured_seed(arg: &str) -> Result<String, String> {
    let (prefix, value) = arg
        .split_once(':')
        .ok_or("Seed must contain ':' as delimiter")
        .unwrap();
    if prefix.is_empty() || value.is_empty() {
        Err(String::from("Seed prefix or value is empty"))
    } else {
        match prefix {
            "string" | "pubkey" | "hex" | "u8" => Ok(arg.to_string()),
            _ => {
                let len = prefix.len();
                if len != 5 && len != 6 {
                    Err(format!("Wrong prefix length {len} {prefix}:{value}"))
                } else {
                    let sign = &prefix[0..1];
                    let type_size = &prefix[1..len.saturating_sub(2)];
                    let byte_order = &prefix[len.saturating_sub(2)..len];
                    if sign != "u" && sign != "i" {
                        Err(format!("Wrong prefix sign {sign} {prefix}:{value}"))
                    } else if type_size != "16"
                        && type_size != "32"
                        && type_size != "64"
                        && type_size != "128"
                    {
                        Err(format!(
                            "Wrong prefix type size {type_size} {prefix}:{value}"
                        ))
                    } else if byte_order != "le" && byte_order != "be" {
                        Err(format!(
                            "Wrong prefix byte order {byte_order} {prefix}:{value}"
                        ))
                    } else {
                        Ok(arg.to_string())
                    }
                }
            }
        }
    }
}

pub fn parse_derived_address_seed(arg: &str) -> Result<String, String> {
    (arg.len() <= MAX_SEED_LEN)
        .then_some(arg.to_string())
        .ok_or(format!(
            "Address seed must not be longer than {MAX_SEED_LEN} bytes"
        ))
}

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

// Return a pubkey for an argument that can itself be parsed into a pubkey,
// or is a filename that can be read as a keypair
pub fn pubkey_of(matches: &ArgMatches, name: &str) -> Option<Pubkey> {
    value_of(matches, name).or_else(|| keypair_of(matches, name).map(|keypair| keypair.pubkey()))
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        clap::{Arg, Command},
        solana_sdk::{hash::Hash, pubkey::Pubkey},
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

    #[test]
    fn test_values_of() {
        let matches = app().get_matches_from(vec!["test", "--multiple", "50", "--multiple", "39"]);
        assert_eq!(values_of(&matches, "multiple"), Some(vec![50, 39]));
        assert_eq!(values_of::<u64>(&matches, "single"), None);

        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let matches = app().get_matches_from(vec![
            "test",
            "--multiple",
            &pubkey0.to_string(),
            "--multiple",
            &pubkey1.to_string(),
        ]);
        assert_eq!(
            values_of(&matches, "multiple"),
            Some(vec![pubkey0, pubkey1])
        );
    }

    #[test]
    fn test_value_of() {
        let matches = app().get_matches_from(vec!["test", "--single", "50"]);
        assert_eq!(value_of(&matches, "single"), Some(50));
        assert_eq!(value_of::<u64>(&matches, "multiple"), None);

        let pubkey = solana_sdk::pubkey::new_rand();
        let matches = app().get_matches_from(vec!["test", "--single", &pubkey.to_string()]);
        assert_eq!(value_of(&matches, "single"), Some(pubkey));
    }

    #[test]
    fn test_parse_pubkey() {
        let command = Command::new("test").arg(
            Arg::new("pubkey")
                .long("pubkey")
                .takes_value(true)
                .value_parser(clap::value_parser!(Pubkey)),
        );

        // success case
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--pubkey", "11111111111111111111111111111111"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Pubkey>("pubkey").unwrap(),
            Pubkey::from_str("11111111111111111111111111111111").unwrap(),
        );

        // validation fails
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--pubkey", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_hash() {
        let command = Command::new("test").arg(
            Arg::new("hash")
                .long("hash")
                .takes_value(true)
                .value_parser(clap::value_parser!(Hash)),
        );

        // success case
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--hash", "11111111111111111111111111111111"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Hash>("hash").unwrap(),
            Hash::from_str("11111111111111111111111111111111").unwrap(),
        );

        // validation fails
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--hash", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_token_decimal() {
        let command = Command::new("test").arg(
            Arg::new("amount")
                .long("amount")
                .takes_value(true)
                .value_parser(Amount::parse_decimal),
        );

        // success cases
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "11223344"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Amount>("amount").unwrap(),
            Amount::Decimal(11223344_f64),
        );

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "0.11223344"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Amount>("amount").unwrap(),
            Amount::Decimal(0.11223344),
        );

        // validation fail cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "all"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_token_decimal_or_all() {
        let command = Command::new("test").arg(
            Arg::new("amount")
                .long("amount")
                .takes_value(true)
                .value_parser(Amount::parse_decimal_or_all),
        );

        // success cases
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "11223344"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Amount>("amount").unwrap(),
            Amount::Decimal(11223344_f64),
        );

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "0.11223344"])
            .unwrap();
        assert_eq!(
            *matches.get_one::<Amount>("amount").unwrap(),
            Amount::Decimal(0.11223344),
        );

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "ALL"])
            .unwrap();
        assert_eq!(*matches.get_one::<Amount>("amount").unwrap(), Amount::All,);

        // validation fail cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--amount", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_sol_to_lamports() {
        let command = Command::new("test").arg(
            Arg::new("amount")
                .long("amount")
                .takes_value(true)
                .value_parser(Amount::parse_decimal_or_all),
        );

        let test_cases = vec![
            ("50", 50_000_000_000),
            ("1.5", 1_500_000_000),
            ("0.03", 30_000_000),
        ];

        for (arg, expected_lamport) in test_cases {
            let matches = command
                .clone()
                .try_get_matches_from(vec!["test", "--amount", arg])
                .unwrap();
            assert_eq!(
                matches
                    .get_one::<Amount>("amount")
                    .unwrap()
                    .sol_to_lamport(),
                Amount::Raw(expected_lamport),
            );
        }
    }

    #[test]
    fn test_derivation() {
        let command = Command::new("test").arg(
            Arg::new("derivation")
                .long("derivation")
                .takes_value(true)
                .value_parser(parse_derivation),
        );

        let test_arguments = vec![
            ("2", true),
            ("0", true),
            ("65537", true),
            ("0/2", true),
            ("a", false),
            ("4294967296", false),
            ("a/b", false),
            ("0/4294967296", false),
        ];

        for (arg, should_accept) in test_arguments {
            if should_accept {
                let matches = command
                    .clone()
                    .try_get_matches_from(vec!["test", "--derivation", arg])
                    .unwrap();
                assert_eq!(matches.get_one::<String>("derivation").unwrap(), arg);
            }
        }
    }
}
