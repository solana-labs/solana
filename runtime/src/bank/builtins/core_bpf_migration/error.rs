use {
    solana_sdk::{instruction::InstructionError, pubkey::Pubkey},
    thiserror::Error,
};

/// Errors returned by a Core BPF migration.
#[derive(Debug, Error)]
pub enum CoreBpfMigrationError {
    /// Solana instruction error
    #[error("Solana instruction error: {0:?}")]
    InstructionError(#[from] InstructionError),
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
    /// Invalid buffer account
    #[error("Invalid buffer account: {0:?}")]
    InvalidBufferAccount(Pubkey),
    /// Arithmetic overflow
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,
}
