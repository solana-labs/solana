// Wallet settings that can be configured for long-term use
use serde_derive::{Deserialize, Serialize};
use std::io;
use url::Url;

lazy_static! {
    pub static ref CONFIG_FILE: Option<String> = {
        dirs::home_dir().map(|mut path| {
            path.extend(&[".config", "solana", "cli", "config.yml"]);
            path.to_str().unwrap().to_string()
        })
    };
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Config {
    pub json_rpc_url: String,
    pub websocket_url: String,
    pub keypair_path: String,
}

impl Default for Config {
    fn default() -> Self {
        let keypair_path = {
            let mut keypair_path = dirs::home_dir().expect("home directory");
            keypair_path.extend(&[".config", "solana", "id.json"]);
            keypair_path.to_str().unwrap().to_string()
        };
        let json_rpc_url = "https://api.mainnet-beta.solana.com".to_string();

        // Empty websocket_url string indicates the client should
        // `Config::compute_websocket_url(&json_rpc_url)`
        let websocket_url = "".to_string();

        Self {
            json_rpc_url,
            websocket_url,
            keypair_path,
        }
    }
}

impl Config {
    pub fn load(config_file: &str) -> Result<Self, io::Error> {
        crate::load_config_file(config_file)
    }

    pub fn save(&self, config_file: &str) -> Result<(), io::Error> {
        crate::save_config_file(self, config_file)
    }

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
            ws_url.set_port(Some(port + 1)).expect("unable to set port");
        }
        ws_url.to_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn compute_websocket_url() {
        assert_eq!(
            Config::compute_websocket_url(&"http://devnet.solana.com"),
            "ws://devnet.solana.com/".to_string()
        );

        assert_eq!(
            Config::compute_websocket_url(&"https://devnet.solana.com"),
            "wss://devnet.solana.com/".to_string()
        );

        assert_eq!(
            Config::compute_websocket_url(&"http://example.com:8899"),
            "ws://example.com:8900/".to_string()
        );
        assert_eq!(
            Config::compute_websocket_url(&"https://example.com:1234"),
            "wss://example.com:1235/".to_string()
        );

        assert_eq!(Config::compute_websocket_url(&"garbage"), String::new());
    }
}
