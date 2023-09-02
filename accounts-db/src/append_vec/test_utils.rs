//! Helpers for AppendVec tests and benches
use {
    super::StoredMeta,
    rand::{distributions::Alphanumeric, Rng},
    solana_sdk::{account::AccountSharedData, pubkey::Pubkey},
    std::path::PathBuf,
};

pub struct TempFile {
    pub path: PathBuf,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let path = std::mem::replace(&mut self.path, PathBuf::new());
        let _ignored = std::fs::remove_file(path);
    }
}

pub fn get_append_vec_dir() -> String {
    std::env::var("FARF_DIR").unwrap_or_else(|_| "farf/append_vec_tests".to_string())
}

pub fn get_append_vec_path(path: &str) -> TempFile {
    let out_dir = get_append_vec_dir();
    let rand_string: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(30)
        .collect();
    let dir = format!("{out_dir}/{rand_string}");
    let mut buf = PathBuf::new();
    buf.push(format!("{dir}/{path}"));
    std::fs::create_dir_all(dir).expect("Create directory failed");
    TempFile { path: buf }
}

pub fn create_test_account(sample: usize) -> (StoredMeta, AccountSharedData) {
    let data_len = sample % 256;
    let mut account = AccountSharedData::new(sample as u64, 0, &Pubkey::default());
    account.set_data_from_slice(&vec![data_len as u8; data_len]);
    let stored_meta = StoredMeta {
        write_version_obsolete: 0,
        pubkey: Pubkey::default(),
        data_len: data_len as u64,
    };
    (stored_meta, account)
}
