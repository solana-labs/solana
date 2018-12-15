use serde_derive::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;
use std::net::SocketAddr;
use untrusted::Input;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Config {
    /// Bind to port or address
    pub bind_port_or_address: Option<String>,

    /// Detect public network address using public servers
    pub use_public_address: bool,

    /// Detect network address from local machine configuration
    pub use_local_address: bool,

    /// Fullnode identity
    pub identity_pkcs8: Vec<u8>,

    /// Fullnode vote account
    pub vote_account_pkcs8: Vec<u8>,
}

impl Config {
    pub fn bind_addr(&self, default_port: u16) -> SocketAddr {
        let mut bind_addr =
            solana_netutil::parse_port_or_addr(&self.bind_port_or_address, default_port);
        if self.use_local_address {
            let ip = solana_netutil::get_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        if self.use_public_address {
            let ip = solana_netutil::get_public_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        bind_addr
    }

    pub fn keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.identity_pkcs8))
            .expect("from_pkcs8 in fullnode::Config keypair")
    }
    pub fn vote_account_keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.vote_account_pkcs8))
            .expect("from_pkcs8 in fullnode::Config vote_account_keypair")
    }
}
