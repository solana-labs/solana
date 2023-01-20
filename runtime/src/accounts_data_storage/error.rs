use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountsDataStorageError {
    Io(#[from] std::io::Error),
    MagicNumberMismatch,
}

impl std::fmt::Display for AccountsDataStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "accounts_data_storage_error")
    }
}
