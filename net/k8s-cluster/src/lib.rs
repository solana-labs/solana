use {
    std::{
        env,
        fs,
        path::PathBuf,
    },
    lazy_static::lazy_static,
};

lazy_static! {
    static ref SOLANA_ROOT: PathBuf = get_solana_root();
}

pub fn initialize_globals() {
    let _ = *SOLANA_ROOT; // Force initialization of lazy_static
}

pub mod config;
pub mod setup;


pub fn get_solana_root() -> PathBuf {
    if let Ok(current_dir) = env::current_dir() {
        if let Ok(canonical_dir) = fs::canonicalize(&current_dir) {
            return canonical_dir.parent().unwrap_or(&canonical_dir).to_path_buf();
        }
    }
    panic!("Failed to get Solana root directory");
}