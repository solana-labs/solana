use {solana_sdk::pubkey::Pubkey, thiserror::Error};

/// Errors returned by `migrate_native_program` functions
#[derive(Debug, Error)]
pub enum MigrateNativeProgramError {
    /// Account not executable
    #[error("Account not executable: {0:?}")]
    AccountNotExecutable(Pubkey),
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
