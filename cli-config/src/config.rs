// Wallet settings that can be configured for long-term use
use {
    serde_derive::{Deserialize, Serialize},
    std::{collections::HashMap, io, path::Path},
    url::Url,
};

lazy_static! {
    /// The default path to the CLI configuration file.
    ///
    /// This is a [lazy_static] of `Option<String>`, the value of which is
    ///
    /// > `~/.config/solana/cli/config.yml`
    ///
    /// It will only be `None` if it is unable to identify the user's home
    /// directory, which should not happen under typical OS environments.
    ///
    /// [lazy_static]: https://docs.rs/lazy_static
    pub static ref CONFIG_FILE: Option<String> = {
        dirs_next::home_dir().map(|mut path| {
            path.extend([".config", "solana", "cli", "config.yml"]);
            path.to_str().unwrap().to_string()
        })
    };
}

/// The Solana CLI configuration.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Config {
    /// The RPC address of a Solana validator node.
    ///
    /// Typical values for mainnet, devnet, and testnet are [described in the
    /// Solana documentation][rpcdocs].
    ///
    /// For local testing, the typical value is `http://localhost:8899`.
    ///
    /// [rpcdocs]: https://docs.solana.com/cluster/rpc-endpoints
    pub json_rpc_url: String,
    /// The address to connect to for receiving event notifications.
    ///
    /// If it is an empty string then the correct value will be derived
    /// from `json_rpc_url`.
    ///
    /// The default value is the empty string.
    pub websocket_url: String,
    /// The default signing source, which may be a keypair file, but may also
    /// represent several other types of signers, as described in the
    /// documentation for `solana_clap_utils::keypair::signer_from_path`.
    /// Because it represents sources other than a simple path, the name
    /// `keypair_path` is misleading, and exists for backwards compatibility
    /// reasons.
    ///
    /// The signing source can be loaded with either the `signer_from_path`
    /// function, or with `solana_clap_utils::keypair::DefaultSigner`.
    pub keypair_path: String,
    /// A mapping from Solana addresses to human-readable names.
    ///
    /// By default the only value in this map is the system program.
    #[serde(default)]
    pub address_labels: HashMap<String, String>,
    /// The default commitment level.
    ///
    /// By default the value is "confirmed", as defined by
    /// `solana_sdk::commitment_config::CommitmentLevel::Confirmed`.
    #[serde(default)]
    pub commitment: String,
}

impl Default for Config {
    fn default() -> Self {
        let keypair_path = {
            let mut keypair_path = dirs_next::home_dir().expect("home directory");
            keypair_path.extend([".config", "solana", "id.json"]);
            keypair_path.to_str().unwrap().to_string()
        };
        let json_rpc_url = "https://api.mainnet-beta.solana.com".to_string();

        // Empty websocket_url string indicates the client should
        // `Config::compute_websocket_url(&json_rpc_url)`
        let websocket_url = "".to_string();

        let mut address_labels = HashMap::new();
        address_labels.insert(
            "11111111111111111111111111111111".to_string(),
            "System Program".to_string(),
        );

        let commitment = "confirmed".to_string();

        Self {
            json_rpc_url,
            websocket_url,
            keypair_path,
            address_labels,
            commitment,
        }
    }
}

impl Config {
    /// Load a configuration from file.
    ///
    /// # Errors
    ///
    /// This function may return typical file I/O errors.
    pub fn load(config_file: &str) -> Result<Self, io::Error> {
        crate::load_config_file(config_file)
    }

    /// Save a configuration to file.
    ///
    /// If the file's directory does not exist, it will be created. If the file
    /// already exists, it will be overwritten.
    ///
    /// # Errors
    ///
    /// This function may return typical file I/O errors.
    pub fn save(&self, config_file: &str) -> Result<(), io::Error> {
        crate::save_config_file(self, config_file)
    }

    /// Compute the websocket URL from the RPC URL.
    ///
    /// The address is created from the RPC URL by:
    ///
    /// - adding 1 to the port number,
    /// - using the "wss" scheme if the RPC URL has an "https" scheme, or the
    ///   "ws" scheme if the RPC URL has an "http" scheme.
    ///
    /// If `json_rpc_url` cannot be parsed as a URL then this function returns
    /// the empty string.
    pub fn compute_websocket_url(json_rpc_url: &str) -> String {
        let json_rpc_url: Option<Url> = json_rpc_url.parse().ok();
        if json_rpc_url.is_none() {
            return "".to_string();
        }
        let json_rpc_url = json_rpc_url.unwrap();
        let is_secure = json_rpc_url.scheme().to_ascii_lowercase() == "https";
        let mut ws_url = json_rpc_url.clone();
        ws_url
            .set_scheme(if is_secure { "wss" } else { "ws" })
            .expect("unable to set scheme");
        if let Some(port) = json_rpc_url.port() {
            let port = port.checked_add(1).expect("port out of range");
            ws_url.set_port(Some(port)).expect("unable to set port");
        }
        ws_url.to_string()
    }

    /// Load a map of address/name pairs from a YAML file at the given path and
    /// insert them into the configuration.
    pub fn import_address_labels<P>(&mut self, filename: P) -> Result<(), io::Error>
    where
        P: AsRef<Path>,
    {
        let imports: HashMap<String, String> = crate::load_config_file(filename)?;
        for (address, label) in imports.into_iter() {
            self.address_labels.insert(address, label);
        }
        Ok(())
    }

    /// Save the map of address/name pairs contained in the configuration to a
    /// YAML file at the given path.
    pub fn export_address_labels<P>(&self, filename: P) -> Result<(), io::Error>
    where
        P: AsRef<Path>,
    {
        crate::save_config_file(&self.address_labels, filename)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn compute_websocket_url() {
        assert_eq!(
            Config::compute_websocket_url("http://api.devnet.solana.com"),
            "ws://api.devnet.solana.com/".to_string()
        );

        assert_eq!(
            Config::compute_websocket_url("https://api.devnet.solana.com"),
            "wss://api.devnet.solana.com/".to_string()
        );

        assert_eq!(
            Config::compute_websocket_url("http://example.com:8899"),
            "ws://example.com:8900/".to_string()
        );
        assert_eq!(
            Config::compute_websocket_url("https://example.com:1234"),
            "wss://example.com:1235/".to_string()
        );

        assert_eq!(Config::compute_websocket_url("garbage"), String::new());
    }
}
