//! Loading and saving the Solana CLI configuration file.
//!
//! The configuration file used by the Solana CLI includes information about the
//! RPC node to connect to, the path to the user's signing source, and more.
//! Other software than the Solana CLI may wish to access the same configuration
//! and signer.
//!
//! The default path to the configuration file can be retrieved from
//! [`CONFIG_FILE`], which is a [lazy_static] of `Option<String>`, the value of
//! which is
//!
//! > `~/.config/solana/cli/config.yml`
//!
//! [`CONFIG_FILE`]: struct@CONFIG_FILE
//! [lazy_static]: https://docs.rs/lazy_static
//!
//! `CONFIG_FILE` will only be `None` if it is unable to identify the user's
//! home directory, which should not happen under typical OS environments.
//!
//! The CLI configuration is defined by the [`Config`] struct, and its value is
//! loaded with [`Config::load`] and saved with [`Config::save`].
//!
//! Two important fields of `Config` are
//!
//! - [`json_rpc_url`], the URL to pass to
//!   `solana_rpc_client::rpc_client::RpcClient`.
//! - [`keypair_path`], a signing source, which may be a keypair file, but
//!   may also represent several other types of signers, as described in
//!   the documentation for `solana_clap_utils::keypair::signer_from_path`.
//!
//! [`json_rpc_url`]: Config::json_rpc_url
//! [`keypair_path`]: Config::keypair_path
//!
//! # Examples
//!
//! Loading and saving the configuration. Note that this uses the [anyhow] crate
//! for error handling.
//!
//! [anyhow]: https://docs.rs/anyhow
//!
//! ```no_run
//! use anyhow::anyhow;
//! use solana_cli_config::{CONFIG_FILE, Config};
//!
//! let config_file = solana_cli_config::CONFIG_FILE.as_ref()
//!     .ok_or_else(|| anyhow!("unable to get config file path"))?;
//! let mut cli_config = Config::load(&config_file)?;
//! // Set the RPC URL to devnet
//! cli_config.json_rpc_url = "https://api.devnet.solana.com".to_string();
//! cli_config.save(&config_file)?;
//! # Ok::<(), anyhow::Error>(())
//! ```

#[macro_use]
extern crate lazy_static;

mod config;
mod config_input;
use std::{
    fs::{create_dir_all, File},
    io::{self, Write},
    path::Path,
};
pub use {
    config::{Config, CONFIG_FILE},
    config_input::{ConfigInput, SettingType},
};

/// Load a value from a file in YAML format.
///
/// Despite the name, this function is generic YAML file deserializer, a thin
/// wrapper around serde.
///
/// Most callers should instead use [`Config::load`].
///
/// # Errors
///
/// This function may return typical file I/O errors.
pub fn load_config_file<T, P>(config_file: P) -> Result<T, io::Error>
where
    T: serde::de::DeserializeOwned,
    P: AsRef<Path>,
{
    let file = File::open(config_file)?;
    let config = serde_yaml::from_reader(file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err:?}")))?;
    Ok(config)
}

/// Save a value to a file in YAML format.
///
/// Despite the name, this function is a generic YAML file serializer, a thin
/// wrapper around serde.
///
/// If the file's directory does not exist, it will be created. If the file
/// already exists, it will be overwritten.
///
/// Most callers should instead use [`Config::save`].
///
/// # Errors
///
/// This function may return typical file I/O errors.
pub fn save_config_file<T, P>(config: &T, config_file: P) -> Result<(), io::Error>
where
    T: serde::ser::Serialize,
    P: AsRef<Path>,
{
    let serialized = serde_yaml::to_string(config)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err:?}")))?;

    if let Some(outdir) = config_file.as_ref().parent() {
        create_dir_all(outdir)?;
    }
    let mut file = File::create(config_file)?;
    file.write_all(b"---\n")?;
    file.write_all(&serialized.into_bytes())?;

    Ok(())
}
