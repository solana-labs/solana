use crate::keypair::ASK_KEYWORD;
use chrono::DateTime;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signature},
};
use std::str::FromStr;

// Return an error if a pubkey cannot be parsed.
pub fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if a hash cannot be parsed.
pub fn is_hash(string: String) -> Result<(), String> {
    match string.parse::<Hash>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if a keypair file cannot be parsed.
pub fn is_keypair(string: String) -> Result<(), String> {
    read_keypair_file(&string)
        .map(|_| ())
        .map_err(|err| format!("{:?}", err))
}

// Return an error if a keypair file cannot be parsed
pub fn is_keypair_or_ask_keyword(string: String) -> Result<(), String> {
    if string.as_str() == ASK_KEYWORD {
        return Ok(());
    }
    read_keypair_file(&string)
        .map(|_| ())
        .map_err(|err| format!("{:?}", err))
}

// Return an error if string cannot be parsed as pubkey string or keypair file location
pub fn is_pubkey_or_keypair(string: String) -> Result<(), String> {
    is_pubkey(string.clone()).or_else(|_| is_keypair(string))
}

// Return an error if string cannot be parsed as pubkey or keypair file or keypair ask keyword
pub fn is_pubkey_or_keypair_or_ask_keyword(string: String) -> Result<(), String> {
    is_pubkey(string.clone()).or_else(|_| is_keypair_or_ask_keyword(string))
}

// Return an error if string cannot be parsed as pubkey=signature string
pub fn is_pubkey_sig(string: String) -> Result<(), String> {
    let mut signer = string.split('=');
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
                Err(err) => Err(format!("{:?}", err)),
            }
        }
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if a url cannot be parsed.
pub fn is_url(string: String) -> Result<(), String> {
    match url::Url::parse(&string) {
        Ok(url) => {
            if url.has_host() {
                Ok(())
            } else {
                Err("no host provided".to_string())
            }
        }
        Err(err) => Err(format!("{:?}", err)),
    }
}

pub fn is_port(port: String) -> Result<(), String> {
    port.parse::<u16>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

pub fn is_valid_percentage(percentage: String) -> Result<(), String> {
    percentage
        .parse::<u8>()
        .map_err(|e| {
            format!(
                "Unable to parse input percentage, provided: {}, err: {:?}",
                percentage, e
            )
        })
        .and_then(|v| {
            if v > 100 {
                Err(format!(
                    "Percentage must be in range of 0 to 100, provided: {}",
                    v
                ))
            } else {
                Ok(())
            }
        })
}

pub fn is_amount(amount: String) -> Result<(), String> {
    if amount.parse::<u64>().is_ok() || amount.parse::<f64>().is_ok() {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as integer or float, provided: {}",
            amount
        ))
    }
}

pub fn is_rfc3339_datetime(value: String) -> Result<(), String> {
    DateTime::parse_from_rfc3339(&value)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

pub fn is_derivation(value: String) -> Result<(), String> {
    let value = value.replace("'", "");
    let mut parts = value.split('/');
    let account = parts.next().unwrap();
    account
        .parse::<u16>()
        .map_err(|e| {
            format!(
                "Unable to parse derivation, provided: {}, err: {:?}",
                account, e
            )
        })
        .and_then(|_| {
            if let Some(change) = parts.next() {
                change.parse::<u16>().map_err(|e| {
                    format!(
                        "Unable to parse derivation, provided: {}, err: {:?}",
                        change, e
                    )
                })
            } else {
                Ok(0)
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_derivation() {
        assert_eq!(is_derivation("2".to_string()), Ok(()));
        assert_eq!(is_derivation("0".to_string()), Ok(()));
        assert_eq!(is_derivation("0/2".to_string()), Ok(()));
        assert_eq!(is_derivation("0'/2'".to_string()), Ok(()));
        assert!(is_derivation("a".to_string()).is_err());
        assert!(is_derivation("65537".to_string()).is_err());
        assert!(is_derivation("a/b".to_string()).is_err());
        assert!(is_derivation("0/65537".to_string()).is_err());
    }
}
