#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
#[cfg(feature = "frozen-abi")]
use solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample};
use {
    core::fmt, solana_instruction::error::InstructionError, solana_sanitize::SanitizeError, std::io,
};

pub type TransactionResult<T> = Result<T, TransactionError>;

/// Reasons a transaction might be rejected.
#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TransactionError {
    /// An account is already being processed in another transaction in a way
    /// that does not support parallelism
    AccountInUse,

    /// A `Pubkey` appears twice in the transaction's `account_keys`.  Instructions can reference
    /// `Pubkey`s more than once but the message must contain a list with no duplicate keys
    AccountLoadedTwice,

    /// Attempt to debit an account but found no record of a prior credit.
    AccountNotFound,

    /// Attempt to load a program that does not exist
    ProgramAccountNotFound,

    /// The from `Pubkey` does not have sufficient balance to pay the fee to schedule the transaction
    InsufficientFundsForFee,

    /// This account may not be used to pay transaction fees
    InvalidAccountForFee,

    /// The bank has seen this transaction before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `recent_blockhash`, or as a double-spend attack.
    AlreadyProcessed,

    /// The bank has not seen the given `recent_blockhash` or the transaction is too old and
    /// the `recent_blockhash` has been discarded.
    BlockhashNotFound,

    /// An error occurred while processing an instruction. The first element of the tuple
    /// indicates the instruction index in which the error occurred.
    InstructionError(u8, InstructionError),

    /// Loader call chain is too deep
    CallChainTooDeep,

    /// Transaction requires a fee but has no signature present
    MissingSignatureForFee,

    /// Transaction contains an invalid account reference
    InvalidAccountIndex,

    /// Transaction did not pass signature verification
    SignatureFailure,

    /// This program may not be used for executing instructions
    InvalidProgramForExecution,

    /// Transaction failed to sanitize accounts offsets correctly
    /// implies that account locks are not taken for this TX, and should
    /// not be unlocked.
    SanitizeFailure,

    ClusterMaintenance,

    /// Transaction processing left an account with an outstanding borrowed reference
    AccountBorrowOutstanding,

    /// Transaction would exceed max Block Cost Limit
    WouldExceedMaxBlockCostLimit,

    /// Transaction version is unsupported
    UnsupportedVersion,

    /// Transaction loads a writable account that cannot be written
    InvalidWritableAccount,

    /// Transaction would exceed max account limit within the block
    WouldExceedMaxAccountCostLimit,

    /// Transaction would exceed account data limit within the block
    WouldExceedAccountDataBlockLimit,

    /// Transaction locked too many accounts
    TooManyAccountLocks,

    /// Address lookup table not found
    AddressLookupTableNotFound,

    /// Attempted to lookup addresses from an account owned by the wrong program
    InvalidAddressLookupTableOwner,

    /// Attempted to lookup addresses from an invalid account
    InvalidAddressLookupTableData,

    /// Address table lookup uses an invalid index
    InvalidAddressLookupTableIndex,

    /// Transaction leaves an account with a lower balance than rent-exempt minimum
    InvalidRentPayingAccount,

    /// Transaction would exceed max Vote Cost Limit
    WouldExceedMaxVoteCostLimit,

    /// Transaction would exceed total account data limit
    WouldExceedAccountDataTotalLimit,

    /// Transaction contains a duplicate instruction that is not allowed
    DuplicateInstruction(u8),

    /// Transaction results in an account with insufficient funds for rent
    InsufficientFundsForRent {
        account_index: u8,
    },

    /// Transaction exceeded max loaded accounts data size cap
    MaxLoadedAccountsDataSizeExceeded,

    /// LoadedAccountsDataSizeLimit set for transaction must be greater than 0.
    InvalidLoadedAccountsDataSizeLimit,

    /// Sanitized transaction differed before/after feature activiation. Needs to be resanitized.
    ResanitizationNeeded,

    /// Program execution is temporarily restricted on an account.
    ProgramExecutionTemporarilyRestricted {
        account_index: u8,
    },

    /// The total balance before the transaction does not equal the total balance after the transaction
    UnbalancedTransaction,

    /// Program cache hit max limit.
    ProgramCacheHitMaxLimit,
}

impl std::error::Error for TransactionError {}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::AccountInUse
             => f.write_str("Account in use"),
            Self::AccountLoadedTwice
             => f.write_str("Account loaded twice"),
            Self::AccountNotFound
             => f.write_str("Attempt to debit an account but found no record of a prior credit."),
            Self::ProgramAccountNotFound
             => f.write_str("Attempt to load a program that does not exist"),
            Self::InsufficientFundsForFee
             => f.write_str("Insufficient funds for fee"),
            Self::InvalidAccountForFee
             => f.write_str("This account may not be used to pay transaction fees"),
            Self::AlreadyProcessed
             => f.write_str("This transaction has already been processed"),
            Self::BlockhashNotFound
             => f.write_str("Blockhash not found"),
            Self::InstructionError(idx, err) =>  write!(f, "Error processing Instruction {idx}: {err}"),
            Self::CallChainTooDeep
             => f.write_str("Loader call chain is too deep"),
            Self::MissingSignatureForFee
             => f.write_str("Transaction requires a fee but has no signature present"),
            Self::InvalidAccountIndex
             => f.write_str("Transaction contains an invalid account reference"),
            Self::SignatureFailure
             => f.write_str("Transaction did not pass signature verification"),
            Self::InvalidProgramForExecution
             => f.write_str("This program may not be used for executing instructions"),
            Self::SanitizeFailure
             => f.write_str("Transaction failed to sanitize accounts offsets correctly"),
            Self::ClusterMaintenance
             => f.write_str("Transactions are currently disabled due to cluster maintenance"),
            Self::AccountBorrowOutstanding
             => f.write_str("Transaction processing left an account with an outstanding borrowed reference"),
            Self::WouldExceedMaxBlockCostLimit
             => f.write_str("Transaction would exceed max Block Cost Limit"),
            Self::UnsupportedVersion
             => f.write_str("Transaction version is unsupported"),
            Self::InvalidWritableAccount
             => f.write_str("Transaction loads a writable account that cannot be written"),
            Self::WouldExceedMaxAccountCostLimit
             => f.write_str("Transaction would exceed max account limit within the block"),
            Self::WouldExceedAccountDataBlockLimit
             => f.write_str("Transaction would exceed account data limit within the block"),
            Self::TooManyAccountLocks
             => f.write_str("Transaction locked too many accounts"),
            Self::AddressLookupTableNotFound
             => f.write_str("Transaction loads an address table account that doesn't exist"),
            Self::InvalidAddressLookupTableOwner
             => f.write_str("Transaction loads an address table account with an invalid owner"),
            Self::InvalidAddressLookupTableData
             => f.write_str("Transaction loads an address table account with invalid data"),
            Self::InvalidAddressLookupTableIndex
             => f.write_str("Transaction address table lookup uses an invalid index"),
            Self::InvalidRentPayingAccount
             => f.write_str("Transaction leaves an account with a lower balance than rent-exempt minimum"),
            Self::WouldExceedMaxVoteCostLimit
             => f.write_str("Transaction would exceed max Vote Cost Limit"),
            Self::WouldExceedAccountDataTotalLimit
             => f.write_str("Transaction would exceed total account data limit"),
            Self::DuplicateInstruction(idx) =>  write!(f, "Transaction contains a duplicate instruction ({idx}) that is not allowed"),
            Self::InsufficientFundsForRent {
                account_index
            } =>  write!(f,"Transaction results in an account ({account_index}) with insufficient funds for rent"),
            Self::MaxLoadedAccountsDataSizeExceeded
             => f.write_str("Transaction exceeded max loaded accounts data size cap"),
            Self::InvalidLoadedAccountsDataSizeLimit
             => f.write_str("LoadedAccountsDataSizeLimit set for transaction must be greater than 0."),
            Self::ResanitizationNeeded
             => f.write_str("ResanitizationNeeded"),
            Self::ProgramExecutionTemporarilyRestricted {
                account_index
            } =>  write!(f,"Execution of the program referenced by account at index {account_index} is temporarily restricted."),
            Self::UnbalancedTransaction
             => f.write_str("Sum of account balances before and after transaction do not match"),
            Self::ProgramCacheHitMaxLimit
             => f.write_str("Program cache hit max limit"),
        }
    }
}

impl From<SanitizeError> for TransactionError {
    fn from(_: SanitizeError) -> Self {
        Self::SanitizeFailure
    }
}

#[cfg(not(target_os = "solana"))]
impl From<SanitizeMessageError> for TransactionError {
    fn from(err: SanitizeMessageError) -> Self {
        match err {
            SanitizeMessageError::AddressLoaderError(err) => Self::from(err),
            _ => Self::SanitizeFailure,
        }
    }
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AddressLoaderError {
    /// Address loading from lookup tables is disabled
    Disabled,

    /// Failed to load slot hashes sysvar
    SlotHashesSysvarNotFound,

    /// Attempted to lookup addresses from a table that does not exist
    LookupTableAccountNotFound,

    /// Attempted to lookup addresses from an account owned by the wrong program
    InvalidAccountOwner,

    /// Attempted to lookup addresses from an invalid account
    InvalidAccountData,

    /// Address lookup contains an invalid index
    InvalidLookupIndex,
}

#[cfg(not(target_os = "solana"))]
impl std::error::Error for AddressLoaderError {}

#[cfg(not(target_os = "solana"))]
impl fmt::Display for AddressLoaderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("Address loading from lookup tables is disabled"),
            Self::SlotHashesSysvarNotFound => f.write_str("Failed to load slot hashes sysvar"),
            Self::LookupTableAccountNotFound => {
                f.write_str("Attempted to lookup addresses from a table that does not exist")
            }
            Self::InvalidAccountOwner => f.write_str(
                "Attempted to lookup addresses from an account owned by the wrong program",
            ),
            Self::InvalidAccountData => {
                f.write_str("Attempted to lookup addresses from an invalid account")
            }
            Self::InvalidLookupIndex => f.write_str("Address lookup contains an invalid index"),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl From<AddressLoaderError> for TransactionError {
    fn from(err: AddressLoaderError) -> Self {
        match err {
            AddressLoaderError::Disabled => Self::UnsupportedVersion,
            AddressLoaderError::SlotHashesSysvarNotFound => Self::AccountNotFound,
            AddressLoaderError::LookupTableAccountNotFound => Self::AddressLookupTableNotFound,
            AddressLoaderError::InvalidAccountOwner => Self::InvalidAddressLookupTableOwner,
            AddressLoaderError::InvalidAccountData => Self::InvalidAddressLookupTableData,
            AddressLoaderError::InvalidLookupIndex => Self::InvalidAddressLookupTableIndex,
        }
    }
}

#[cfg(not(target_os = "solana"))]
#[derive(PartialEq, Debug, Eq, Clone)]
pub enum SanitizeMessageError {
    IndexOutOfBounds,
    ValueOutOfBounds,
    InvalidValue,
    AddressLoaderError(AddressLoaderError),
}

#[cfg(not(target_os = "solana"))]
impl std::error::Error for SanitizeMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IndexOutOfBounds => None,
            Self::ValueOutOfBounds => None,
            Self::InvalidValue => None,
            Self::AddressLoaderError(e) => Some(e),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl fmt::Display for SanitizeMessageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IndexOutOfBounds => f.write_str("index out of bounds"),
            Self::ValueOutOfBounds => f.write_str("value out of bounds"),
            Self::InvalidValue => f.write_str("invalid value"),
            Self::AddressLoaderError(e) => {
                write!(f, "{e}")
            }
        }
    }
}
#[cfg(not(target_os = "solana"))]
impl From<AddressLoaderError> for SanitizeMessageError {
    fn from(source: AddressLoaderError) -> Self {
        SanitizeMessageError::AddressLoaderError(source)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<SanitizeError> for SanitizeMessageError {
    fn from(err: SanitizeError) -> Self {
        match err {
            SanitizeError::IndexOutOfBounds => Self::IndexOutOfBounds,
            SanitizeError::ValueOutOfBounds => Self::ValueOutOfBounds,
            SanitizeError::InvalidValue => Self::InvalidValue,
        }
    }
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug)]
pub enum TransportError {
    IoError(io::Error),
    TransactionError(TransactionError),
    Custom(String),
}

#[cfg(not(target_os = "solana"))]
impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::IoError(e) => Some(e),
            TransportError::TransactionError(e) => Some(e),
            TransportError::Custom(_) => None,
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter) -> ::core::fmt::Result {
        match self {
            Self::IoError(e) => f.write_fmt(format_args!("transport io error: {e}")),
            Self::TransactionError(e) => {
                f.write_fmt(format_args!("transport transaction error: {e}"))
            }
            Self::Custom(s) => f.write_fmt(format_args!("transport custom error: {s}")),
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl From<io::Error> for TransportError {
    fn from(e: io::Error) -> Self {
        TransportError::IoError(e)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<TransactionError> for TransportError {
    fn from(e: TransactionError) -> Self {
        TransportError::TransactionError(e)
    }
}

#[cfg(not(target_os = "solana"))]
impl TransportError {
    pub fn unwrap(&self) -> TransactionError {
        if let TransportError::TransactionError(err) = self {
            err.clone()
        } else {
            panic!("unexpected transport error")
        }
    }
}

#[cfg(not(target_os = "solana"))]
pub type TransportResult<T> = std::result::Result<T, TransportError>;
