// Wallet settings that can be configured for long-term use
use serde_derive::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{self, Write};
use std::path::Path;

lazy_static! {
    pub static ref CONFIG_FILE: Option<String> = {
        dirs::home_dir().map(|mut path| {
            path.extend(&[".config", "solana", "wallet", "config.yml"]);
            path.to_str().unwrap().to_string()
        })
    };
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Config {
    pub url: String,
    pub keypair: String,
}

impl Config {
    pub fn new(url: &str, keypair: &str) -> Self {
        Self {
            url: url.to_string(),
            keypair: keypair.to_string(),
        }
    }

    pub fn load(config_file: &str) -> Result<Self, io::Error> {
        let file = File::open(config_file.to_string())?;
        let config = serde_yaml::from_reader(file)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;
        Ok(config)
    }

    pub fn save(&self, config_file: &str) -> Result<(), io::Error> {
        let serialized = serde_yaml::to_string(self)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

        if let Some(outdir) = Path::new(&config_file).parent() {
            create_dir_all(outdir)?;
        }
        let mut file = File::create(config_file)?;
        file.write_all(&serialized.into_bytes())?;

        Ok(())
    }
}
