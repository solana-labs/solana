use {solana_sdk::pubkey::Pubkey, thiserror::Error};

/// Errors returned by `migrate_native_program` functions
#[derive(Debug, Error)]
pub enum MigrateNativeProgramError {
    /// Account not executable
    #[error("Account not executable: {0:?}")]
    AccountNotExecutable(Pubkey),
    /// Account is executable
    #[error("Account is executable: {0:?}")]
    AccountIsExecutable(Pubkey),
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
    /// Invalid program data account
    #[error("Invalid program data account: {0:?}")]
    InvalidProgramDataAccount(Pubkey),
    /// Arithmetic overflow
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,
    /// Failed to deserialize
    #[error("Failed to deserialize: {0}")]
    FailedToDeserialize(#[from] Box<bincode::ErrorKind>),
}
