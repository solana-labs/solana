use {
    crate::keypair::{parse_signer_source, SignerSourceKind, ASK_KEYWORD},
    chrono::DateTime,
    solana_sdk::{
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::{Pubkey, MAX_SEED_LEN},
        signature::{read_keypair_file, Signature},
    },
    std::{fmt::Display, str::FromStr},
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
pub fn is_parsable<T>(string: String) -> Result<(), String>
where
    T: FromStr,
    T::Err: Display,
{
    is_parsable_generic::<T, String>(string)
}

// Return an error if string cannot be parsed as numeric type T, and value not within specified
// range
pub fn is_within_range<T>(string: String, range_min: T, range_max: T) -> Result<(), String>
where
    T: FromStr + Copy + std::fmt::Debug + PartialOrd + std::ops::Add<Output = T> + From<usize>,
    T::Err: Display,
{
    match string.parse::<T>() {
        Ok(input) => {
            let range = range_min..range_max + 1.into();
            if !range.contains(&input) {
                Err(format!(
                    "input '{input:?}' out of range ({range_min:?}..{range_max:?}]"
                ))
            } else {
                Ok(())
            }
        }
        Err(err) => Err(format!("error parsing '{string}': {err}")),
    }
}

// Return an error if a pubkey cannot be parsed.
pub fn is_pubkey<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Pubkey, _>(string)
}

// Return an error if a hash cannot be parsed.
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
pub fn is_prompt_signer_source<T>(string: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if string.as_ref() == ASK_KEYWORD {
        return Ok(());
    }
    match parse_signer_source(string.as_ref())
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

pub fn is_epoch<T>(epoch: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Epoch, _>(epoch)
}

pub fn is_slot<T>(slot: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<Slot, _>(slot)
}

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

pub fn is_port<T>(port: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    is_parsable_generic::<u16, _>(port)
}

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

pub fn is_rfc3339_datetime<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    DateTime::parse_from_rfc3339(value.as_ref())
        .map(|_| ())
        .map_err(|e| format!("{e}"))
}

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

pub fn is_niceness_adjustment_valid<T>(value: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    let adjustment = value
        .as_ref()
        .parse::<i8>()
        .map_err(|err| format!("error parsing niceness adjustment value '{value}': {err}"))?;
    if solana_perf::thread::is_renice_allowed(adjustment) {
        Ok(())
    } else {
        Err(String::from(
            "niceness adjustment supported only on Linux; negative adjustment \
             (priority increase) requires root or CAP_SYS_NICE (see `man 7 capabilities` \
             for details)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_derivation() {
        assert_eq!(is_derivation("2"), Ok(()));
        assert_eq!(is_derivation("0"), Ok(()));
        assert_eq!(is_derivation("65537"), Ok(()));
        assert_eq!(is_derivation("0/2"), Ok(()));
        assert_eq!(is_derivation("0'/2'"), Ok(()));
        assert!(is_derivation("a").is_err());
        assert!(is_derivation("4294967296").is_err());
        assert!(is_derivation("a/b").is_err());
        assert!(is_derivation("0/4294967296").is_err());
    }

    #[test]
    fn test_is_niceness_adjustment_valid() {
        assert_eq!(is_niceness_adjustment_valid("0"), Ok(()));
        assert!(is_niceness_adjustment_valid("128").is_err());
        assert!(is_niceness_adjustment_valid("-129").is_err());
    }
}
