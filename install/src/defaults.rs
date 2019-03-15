pub const JSON_RPC_URL: &str = "https://api.testnet.solana.com/";

lazy_static! {
    pub static ref CONFIG_FILE: Option<String> = {
        dirs::config_dir().map(|mut path| {
            path.push("solana");
            path.push("install.yml");
            path.to_str().unwrap().to_string()
        })
    };
    pub static ref DATA_DIR: Option<String> = {
        dirs::data_dir().map(|mut path| {
            path.push("solana");
            path.to_str().unwrap().to_string()
        })
    };
}

pub fn update_pubkey(target: &str) -> Option<&str> {
    match target {
        "x86_64-apple-darwin" => Some("9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp"), // TODO: This pubkey is invalid
        "x86_64-unknown-linux-gnu" => Some("8XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp"), // TODO: This pubkey is invalid
        _ => None,
    }
}
