use {
    crate::keypair::{parse_signer_source, SignerSourceKind, ASK_KEYWORD},
    chrono::DateTime,
    solana_sdk::{
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::{Pubkey, MAX_SEED_LEN},
        signature::{read_keypair_file, Signature},
    },
    std::{fmt::Display, ops::RangeBounds, str::FromStr},
};

fn is_parsable_generic<U, T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
    U: FromStr,
    U::Err: Display,
{
    string
        .as_ref()
        .parse::<U>()
        .map(|_| ())
        .map_err(|err| format!("error parsing '{string}': {err}"))
}

// Return an error if string cannot be parsed as type T.
// Takes a String to avoid second type parameter when used as a clap validator
#[deprecated(since = "1.17.0", note = "please use `clap::value_parser!` instead")]
pub fn is_parsable<T>(string: &str) -> Result<(), String>
where
    T: FromStr,
    T::Err: Display,
{
    is_parsable_generic::<T, &str>(string)
}

// Return an error if string cannot be parsed as numeric type T, and value not within specified
// range
#[deprecated(
    since = "1.17.0",
    note = "please use `clap::builder::RangedI64ValueParser` instead"
)]
pub fn is_within_range<T, R>(string: String, range: R) -> Result<(), String>
where
    T: FromStr + Copy + std::fmt::Debug + PartialOrd + std::ops::Add<Output = T> + From<usize>,
    T::Err: Display,
    R: RangeBounds<T> + std::fmt::Debug,
{
    match string.parse::<T>() {
        Ok(input) => {
            if !range.contains(&input) {
                Err(format!("input '{input:?}' out of range {range:?}"))
            } else {
                Ok(())
            }
        }
        Err(err) => Err(format!("error parsing '{string}': {err}")),
    }
}

// Return an error if a pubkey cannot be parsed.
pub fn is_pubkey(string: &str) -> Result<(), String> {
    is_parsable_generic::<Pubkey, _>(string)
}

// Return an error if a hash cannot be parsed.
#[deprecated(
    since = "1.17.0",
    note = "please use `clap::value_parser!(Hash)` instead"
)]
pub fn is_hash<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Hash, _>(string)
}

// Return an error if a keypair file cannot be parsed.
pub fn is_keypair<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    read_keypair_file(string.as_ref())
        .map(|_| ())
        .map_err(|err| format!("{err}"))
}

// Return an error if a keypair file cannot be parsed
pub fn is_keypair_or_ask_keyword<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if string.as_ref() == ASK_KEYWORD {
        return Ok(());
    }
    read_keypair_file(string.as_ref())
        .map(|_| ())
        .map_err(|err| format!("{err}"))
}

// Return an error if a `SignerSourceKind::Prompt` cannot be parsed
pub fn is_prompt_signer_source(string: &str) -> Result<(), String> {
    if string == ASK_KEYWORD {
        return Ok(());
    }
    match parse_signer_source(string)
        .map_err(|err| format!("{err}"))?
        .kind
    {
        SignerSourceKind::Prompt => Ok(()),
        _ => Err(format!(
            "Unable to parse input as `prompt:` URI scheme or `ASK` keyword: {string}"
        )),
    }
}

// Return an error if string cannot be parsed as pubkey string or keypair file location
pub fn is_pubkey_or_keypair<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_pubkey(string.as_ref()).or_else(|_| is_keypair(string))
}

// Return an error if string cannot be parsed as a pubkey string, or a valid Signer that can
// produce a pubkey()
pub fn is_valid_pubkey<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    match parse_signer_source(string.as_ref())
        .map_err(|err| format!("{err}"))?
        .kind
    {
        SignerSourceKind::Filepath(path) => is_keypair(path),
        _ => Ok(()),
    }
}

// Return an error if string cannot be parsed as a valid Signer. This is an alias of
// `is_valid_pubkey`, and does accept pubkey strings, even though a Pubkey is not by itself
// sufficient to sign a transaction.
//
// In the current offline-signing implementation, a pubkey is the valid input for a signer field
// when paired with an offline `--signer` argument to provide a Presigner (pubkey + signature).
// Clap validators can't check multiple fields at once, so the verification that a `--signer` is
// also provided and correct happens in parsing, not in validation.
pub fn is_valid_signer<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_valid_pubkey(string)
}

// Return an error if string cannot be parsed as pubkey=signature string
#[deprecated(
    since = "1.17.0",
    note = "please use `clap::value_parser!(PubkeySignature)` instead"
)]
pub fn is_pubkey_sig<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    let mut signer = string.as_ref().split('=');
    match Pubkey::from_str(
        signer
            .next()
            .ok_or_else(|| "Malformed signer string".to_string())?,
    ) {
        Ok(_) => {
            match Signature::from_str(
                signer
                    .next()
                    .ok_or_else(|| "Malformed signer string".to_string())?,
            ) {
                Ok(_) => Ok(()),
                Err(err) => Err(format!("{err}")),
            }
        }
        Err(err) => Err(format!("{err}")),
    }
}

// Return an error if a url cannot be parsed.
#[deprecated(since = "1.17.0", note = "please use `parse_url` instead")]
pub fn is_url<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    match url::Url::parse(string.as_ref()) {
        Ok(url) => {
            if url.has_host() {
                Ok(())
            } else {
                Err("no host provided".to_string())
            }
        }
        Err(err) => Err(format!("{err}")),
    }
}

#[deprecated(since = "1.17.0", note = "please use `parse_url_or_moniker` instead")]
pub fn is_url_or_moniker<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    match url::Url::parse(&normalize_to_url_if_moniker(string.as_ref())) {
        Ok(url) => {
            if url.has_host() {
                Ok(())
            } else {
                Err("no host provided".to_string())
            }
        }
        Err(err) => Err(format!("{err}")),
    }
}

pub fn normalize_to_url_if_moniker<T: AsRef<str>>(url_or_moniker: T) -> String {
    match url_or_moniker.as_ref() {
        "m" | "mainnet-beta" => "https://api.mainnet-beta.solana.com",
        "t" | "testnet" => "https://api.testnet.solana.com",
        "d" | "devnet" => "https://api.devnet.solana.com",
        "l" | "localhost" => "http://localhost:8899",
        url => url,
    }
    .to_string()
}

#[deprecated(
    since = "1.17.0",
    note = "please use `clap::value_parser!(Epoch)` instead"
)]
pub fn is_epoch<T>(epoch: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Epoch, _>(epoch)
}

#[deprecated(
    since = "1.17.0",
    note = "please use `clap::value_parser!(Slot)` instead"
)]
pub fn is_slot<T>(slot: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Slot, _>(slot)
}

#[deprecated(since = "1.17.0", note = "please use `parse_pow2` instead")]
pub fn is_pow2<T>(bins: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    bins.as_ref()
        .parse::<usize>()
        .map_err(|e| format!("Unable to parse, provided: {bins}, err: {e}"))
        .and_then(|v| {
            if !v.is_power_of_two() {
                Err(format!("Must be a power of 2: {v}"))
            } else {
                Ok(())
            }
        })
}

#[deprecated(
    since = "1.17.0",
    note = "please use `clap_value_parser!(u16)` instead"
)]
pub fn is_port<T>(port: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<u16, _>(port)
}

#[deprecated(since = "1.17.0", note = "please use `parse_percentage` instead")]
pub fn is_valid_percentage<T>(percentage: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    percentage
        .as_ref()
        .parse::<u8>()
        .map_err(|e| format!("Unable to parse input percentage, provided: {percentage}, err: {e}"))
        .and_then(|v| {
            if v > 100 {
                Err(format!(
                    "Percentage must be in range of 0 to 100, provided: {v}"
                ))
            } else {
                Ok(())
            }
        })
}

#[deprecated(since = "1.17.0", note = "please use `Amount::parse_decimal` instead")]
pub fn is_amount<T>(amount: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if amount.as_ref().parse::<u64>().is_ok() || amount.as_ref().parse::<f64>().is_ok() {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as integer or float, provided: {amount}"
        ))
    }
}

#[deprecated(
    since = "1.17.0",
    note = "please use `TokenAmount::parse_decimal` instead"
)]
pub fn is_amount_or_all<T>(amount: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if amount.as_ref().parse::<u64>().is_ok()
        || amount.as_ref().parse::<f64>().is_ok()
        || amount.as_ref() == "ALL"
    {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as integer or float, provided: {amount}"
        ))
    }
}

#[deprecated(since = "1.17.0", note = "please use `parse_rfc3339_datetime` instead")]
pub fn is_rfc3339_datetime<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    DateTime::parse_from_rfc3339(value.as_ref())
        .map(|_| ())
        .map_err(|e| format!("{e}"))
}

#[deprecated(since = "1.17.0", note = "please use `parse_derivation` instead")]
pub fn is_derivation<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    let value = value.as_ref().replace('\'', "");
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
        })
        .map(|_| ())
}

#[deprecated(since = "1.17.0", note = "please use `parse_structured_seed` instead")]
pub fn is_structured_seed<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    let (prefix, value) = value
        .as_ref()
        .split_once(':')
        .ok_or("Seed must contain ':' as delimiter")
        .unwrap();
    if prefix.is_empty() || value.is_empty() {
        Err(String::from("Seed prefix or value is empty"))
    } else {
        match prefix {
            "string" | "pubkey" | "hex" | "u8" => Ok(()),
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
                        Ok(())
                    }
                }
            }
        }
    }
}

#[deprecated(
    since = "1.17.0",
    note = "please use `parse_derived_address_seed` instead"
)]
pub fn is_derived_address_seed<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    let value = value.as_ref();
    if value.len() > MAX_SEED_LEN {
        Err(format!(
            "Address seed must not be longer than {MAX_SEED_LEN} bytes"
        ))
    } else {
        Ok(())
    }
}
