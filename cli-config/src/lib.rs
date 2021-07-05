#[macro_use]
extern crate lazy_static;

mod config;
pub use config::{Config, CONFIG_FILE};

use std::{
    fs::{create_dir_all, File},
    io::{self, Write},
    path::Path,
};

pub fn load_config_file<T, P>(config_file: P) -> Result<T, io::Error>
where
    T: serde::de::DeserializeOwned,
    P: AsRef<Path>,
{
    let file = File::open(config_file)?;
    let config = serde_yaml::from_reader(file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;
    Ok(config)
}

pub fn save_config_file<T, P>(config: &T, config_file: P) -> Result<(), io::Error>
where
    T: serde::ser::Serialize,
    P: AsRef<Path>,
{
    let serialized = serde_yaml::to_string(config)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    if let Some(outdir) = config_file.as_ref().parent() {
        create_dir_all(outdir)?;
    }
    let mut file = File::create(config_file)?;
    file.write_all(&serialized.into_bytes())?;

    Ok(())
}
