use thiserror::Error;

#[derive(Error, Debug)]
pub enum TieredStorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("MagicNumberMismatch: expect {0}, found {1}")]
    MagicNumberMismatch(u64, u64),
}

pub type TieredStorageResult<T> = Result<T, TieredStorageError>;
