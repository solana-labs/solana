use std::collections::HashMap;

pub enum Clusters {
    Testnet,
    MainnetBeta,
    Devnet,
    Development,
    Custom,
}

impl Clusters {
    pub fn get_cluster(moniker: &str) -> Self {
        match moniker {
            "t" | "testnet" => Clusters::Testnet,
            "m" | "mainnet-beta" => Clusters::MainnetBeta,
            "d" | "devnet" => Clusters::Devnet,
            "l" | "localhost" => Clusters::Development,
            _ => Clusters::Custom,
        }
    }

    fn as_str(&self) -> String {
        match self {
            Clusters::Testnet => "t",
            Clusters::MainnetBeta => "m",
            Clusters::Devnet => "d",
            Clusters::Development => "l",
            Clusters::Custom => "c",
        }
        .to_string()
    }

    fn get_public_url(&self) -> String {
        match self {
            Clusters::Testnet => "https://api.testnet.solana.com",
            Clusters::MainnetBeta => "https://api.mainnet-beta.solana.com",
            Clusters::Devnet => "https://api.devnet.solana.com",
            Clusters::Development => "http://localhost:8899",
            Clusters::Custom => "",
        }
        .to_string()
    }

    pub fn get_rpc_url(&self, custom: &HashMap<String, String>) -> String {
        custom
            .get(&self.as_str())
            .unwrap_or(&self.get_public_url())
            .to_string()
    }
}
