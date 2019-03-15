use crate::update_manifest::SignedUpdateManifest;
use serde_derive::{Deserialize, Serialize};
use serde_yaml;
use solana_sdk::pubkey::Pubkey;
use std::fs::{create_dir_all, File};
use std::io::{Error, ErrorKind, Write};
use std::path::Path;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Config {
    pub data_dir: String,
    pub json_rpc_url: String,
    pub update_pubkey: Pubkey,
    pub current_install: Option<SignedUpdateManifest>,
}

impl Config {
    fn _load(config_file: &str) -> Result<Self, Error> {
        let file = File::open(config_file.to_string())?;
        let config = serde_yaml::from_reader(file)
            .map_err(|err| Error::new(ErrorKind::Other, format!("{:?}", err)))?;
        Ok(config)
    }

    pub fn load(config_file: &str) -> Result<Self, String> {
        Self::_load(config_file).map_err(|err| format!("Unable to load {}: {:?}", config_file, err))
    }

    fn _save(&self, config_file: &str) -> Result<(), Error> {
        let serialized = serde_yaml::to_string(self)
            .map_err(|err| Error::new(ErrorKind::Other, format!("{:?}", err)))?;

        if let Some(outdir) = Path::new(&config_file).parent() {
            create_dir_all(outdir)?;
        }
        let mut file = File::create(config_file)?;
        file.write_all(&serialized.into_bytes())?;

        Ok(())
    }

    pub fn save(&self, config_file: &str) -> Result<(), String> {
        self._save(config_file)
            .map_err(|err| format!("Unable to save {}: {:?}", config_file, err))
    }
}
