use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_sdk::{
        pubkey::Pubkey,
        signer::keypair::{read_keypair_file, Keypair},
    },
    thiserror::Error,
    yaml_rust::YamlLoader,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to read solana config file: ({0})")]
    ConfigRead(std::io::Error),
    #[error("failed to parse solana config file: ({0})")]
    ConfigParse(#[from] yaml_rust::ScanError),
    #[error("invalid config: ({0})")]
    InvalidConfig(String),

    #[error("solana client error: ({0})")]
    Client(#[from] solana_client::client_error::ClientError),

    #[error("error in public key derivation: ({0})")]
    KeyDerivation(#[from] solana_sdk::pubkey::PubkeyError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The schema for greeting storage in greeting accounts. This is what
/// is serialized into the account and updated when hellos are sent.
#[derive(BorshSerialize, BorshDeserialize)]
struct GreetingSchema {
    counter: u32,
}

/// Parses and returns the Solana yaml config on the system.
pub fn get_config(config: &Option<&str>) -> Result<yaml_rust::Yaml> {
    let path = match config {
        Some(path) => std::path::PathBuf::from(path),
        None => match home::home_dir() {
            Some(mut path) => {
                path.push(".config/solana/cli/config.yml");
                path
            }
            None => {
                return Err(Error::ConfigRead(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "failed to locate homedir and thus can not locate solana config",
                )));
            }
        },
    };
    let config = std::fs::read_to_string(path).map_err(Error::ConfigRead)?;
    let mut config = YamlLoader::load_from_str(&config)?;
    match config.len() {
        1 => Ok(config.remove(0)),
        l => Err(Error::InvalidConfig(format!(
            "expected one yaml document got ({})",
            l
        ))),
    }
}

/// Gets the RPC url for the cluster that this machine is configured
/// to communicate with.
pub fn get_rpc_url(config: &Option<&str>) -> Result<String> {
    let config = get_config(config)?;
    match config["json_rpc_url"].as_str() {
        Some(s) => Ok(s.to_string()),
        None => Err(Error::InvalidConfig(
            "missing `json_rpc_url` field".to_string(),
        )),
    }
}

/// Gets the "player" or local solana wallet that has been configured
/// on the machine.
pub fn get_player(config: &Option<&str>) -> Result<Keypair> {
    let config = get_config(config)?;
    if let Some(path) = config["keypair_path"].as_str() {
        read_keypair_file(path).map_err(|e| {
            Error::InvalidConfig(format!("failed to read keypair file ({}): ({})", path, e))
        })
    } else {
        Err(Error::InvalidConfig(
            "missing `keypair_path` field".to_string(),
        ))
    }
}

/// Gets the seed used to generate greeting accounts. If you'd like to
/// force this program to generate a new greeting account and thus
/// restart the counter you can change this value.
pub fn get_greeting_seed() -> &'static str {
    "hello"
}

/// Derives and returns the greeting account public key for a given
/// PLAYER, PROGRAM combination.
pub fn get_greeting_public_key(player: &Pubkey, program: &Pubkey) -> Result<Pubkey> {
    Ok(Pubkey::create_with_seed(
        player,
        get_greeting_seed(),
        program,
    )?)
}
