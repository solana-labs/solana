use crate::keypair::ASK_KEYWORD;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Signature};
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

pub fn is_semver(semver: &str) -> Result<(), String> {
    match semver::Version::parse(&semver) {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

pub fn is_release_channel(channel: &str) -> Result<(), String> {
    match channel {
        "edge" | "beta" | "stable" => Ok(()),
        _ => Err(format!("Invalid release channel {}", channel)),
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
