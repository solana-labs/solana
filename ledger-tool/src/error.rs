use {solana_ledger::blockstore::BlockstoreError, thiserror::Error};

pub type Result<T> = std::result::Result<T, LedgerToolError>;

#[derive(Error, Debug)]
pub enum LedgerToolError {
    #[error("{0}")]
    Blockstore(#[from] BlockstoreError),

    #[error("{0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    BadArgument(String),
}
