use {solana_sdk::pubkey::Pubkey, thiserror::Error};

/// Errors returned by a Core BPF migration.
#[derive(Debug, Error)]
pub enum CoreBpfMigrationError {
    /// Bincode serialization error
    #[error("Bincode serialization error: {0:?}")]
    BincodeError(#[from] bincode::Error),
    /// Account not found
    #[error("Account not found: {0:?}")]
    AccountNotFound(Pubkey),
    /// Account exists
    #[error("Account exists: {0:?}")]
    AccountExists(Pubkey),
    /// Incorrect account owner
    #[error("Incorrect account owner for {0:?}")]
    IncorrectOwner(Pubkey),
    /// Program has a data account
    #[error("Data account exists for program {0:?}")]
    ProgramHasDataAccount(Pubkey),
    /// Program has no data account
    #[error("Data account does not exist for program {0:?}")]
    ProgramHasNoDataAccount(Pubkey),
    /// Invalid program account
    #[error("Invalid program account: {0:?}")]
    InvalidProgramAccount(Pubkey),
    /// Invalid program data account
    #[error("Invalid program data account: {0:?}")]
    InvalidProgramDataAccount(Pubkey),
    /// Arithmetic overflow
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,
}
