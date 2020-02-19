pub const JSON_RPC_URL: &str = "http://devnet.solana.com:8899";

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
        "x86_64-apple-darwin" => Some("GRUP8YUGASLdu2gBwHstFgeVH28qppfuCaTzq5Yo7wRo"), // SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR_x86_64_apple_darwin
        "x86_64-unknown-linux-gnu" => Some("FnKt2ES9iUJkjoprf2rL62xxBAxZLVgyA4SFexPGotFE"), // SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR_x86_64_unknown_linux_gnu
        "x86_64-pc-windows-msvc" => Some("2Lrj5xDCHDmqwCgGwjVqAUUM84vLpj5dReYeoXL9vSXV"), // SOLANA_INSTALL_UPDATE_MANIFEST_KEYPAIR_x86_64_pc_windows_msvc
        _ => None,
    }
}
