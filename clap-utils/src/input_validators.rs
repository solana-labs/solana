use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;

// Return an error if a pubkey cannot be parsed.
pub fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
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

// Return an error if string cannot be parsed as pubkey string or keypair file location
pub fn is_pubkey_or_keypair(string: String) -> Result<(), String> {
    is_pubkey(string.clone()).or_else(|_| is_keypair(string))
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
