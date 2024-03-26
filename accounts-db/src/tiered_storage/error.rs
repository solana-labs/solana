use {super::footer::SanitizeFooterError, std::path::PathBuf, thiserror::Error};

#[derive(Error, Debug)]
pub enum TieredStorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("MagicNumberMismatch: expected {0}, found {1}")]
    MagicNumberMismatch(u64, u64),

    #[error("AttemptToUpdateReadOnly: attempted to update read-only file {0}")]
    AttemptToUpdateReadOnly(PathBuf),

    #[error("UnknownFormat: the tiered storage format is unknown for file {0}")]
    UnknownFormat(PathBuf),

    #[error("Unsupported: the feature is not yet supported")]
    Unsupported(),

    #[error("invalid footer size: {0}, expected: {1}")]
    InvalidFooterSize(u64, u64),

    #[error("invalid footer version: {0}")]
    InvalidFooterVersion(u64),

    #[error("footer is unsanitary: {0}")]
    SanitizeFooter(#[from] SanitizeFooterError),

    #[error("OffsetOutOfBounds: offset {0} is larger than the supported size {1}")]
    OffsetOutOfBounds(usize, usize),

    #[error("OffsetAlignmentError: offset {0} must be multiple of {1}")]
    OffsetAlignmentError(usize, usize),
}
