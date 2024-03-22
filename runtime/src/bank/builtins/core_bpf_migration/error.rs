use {solana_sdk::pubkey::Pubkey, thiserror::Error};

/// Errors returned by a Core BPF migration.
#[derive(Debug, Error, PartialEq)]
pub enum CoreBpfMigrationError {
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
}
