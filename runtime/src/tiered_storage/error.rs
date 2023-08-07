use {std::path::PathBuf, thiserror::Error};

#[derive(Error, Debug)]
pub enum TieredStorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("MagicNumberMismatch: expected {0}, found {1}")]
    MagicNumberMismatch(u64, u64),

    #[error("AttemptToUpdateReadOnly: attempted to update read-only file {0}")]
    AttemptToUpdateReadOnly(PathBuf),

    #[error("UnknownFormat: the tiered storage format is unavailable for file {0}")]
    UnknownFormat(PathBuf),

    #[error("Unsupported: the feature is not yet supported")]
    Unsupported(),
}
