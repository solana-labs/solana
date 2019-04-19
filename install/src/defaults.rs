pub const JSON_RPC_URL: &str = "https://api.testnet.solana.com/";

lazy_static! {
    pub static ref CONFIG_FILE: Option<String> = {
        dirs::home_dir().map(|mut path| {
            path.extend(&[".config", "solana", "install", "config.yml"]);
            path.to_str().unwrap().to_string()
        })
    };
    pub static ref USER_KEYPAIR: Option<String> = {
        dirs::home_dir().map(|mut path| {
            path.extend(&[".config", "solana", "id.json"]);
            path.to_str().unwrap().to_string()
        })
    };
    pub static ref DATA_DIR: Option<String> = {
        dirs::home_dir().map(|mut path| {
            path.extend(&[".local", "share", "solana", "install"]);
            path.to_str().unwrap().to_string()
        })
    };
}

pub fn update_manifest_pubkey(target: &str) -> Option<&str> {
    match target {
        "x86_64-apple-darwin" => None,
        "x86_64-unknown-linux-gnu" => Some("FnKt2ES9iUJkjoprf2rL62xxBAxZLVgyA4SFexPGotFE"), // SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR_x86_64_unknown_linux_gnu
        _ => None,
    }
}
