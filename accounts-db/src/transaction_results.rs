// Re-exported since these have moved to `solana_sdk`.
#[deprecated(
    since = "1.18.0",
    note = "Please use `solana_sdk::inner_instruction` types instead"
)]
pub use solana_sdk::inner_instruction::{InnerInstruction, InnerInstructionsList};
use {
    solana_program_runtime::loaded_programs::LoadedProgramsForTxBatch,
    solana_sdk::{
        nonce_info::{NonceFull, NonceInfo},
        rent_debits::RentDebits,
        transaction::{self, TransactionError},
        transaction_context::TransactionReturnData,
    },
};

pub struct TransactionResults {
    pub fee_collection_results: Vec<transaction::Result<()>>,
    pub execution_results: Vec<TransactionExecutionResult>,
    pub rent_debits: Vec<RentDebits>,
}

/// Type safe representation of a transaction execution attempt which
/// differentiates between a transaction that was executed (will be
/// committed to the ledger) and a transaction which wasn't executed
/// and will be dropped.
///
/// Note: `Result<TransactionExecutionDetails, TransactionError>` is not
/// used because it's easy to forget that the inner `details.status` field
/// is what should be checked to detect a successful transaction. This
/// enum provides a convenience method `Self::was_executed_successfully` to
/// make such checks hard to do incorrectly.
#[derive(Debug, Clone)]
pub enum TransactionExecutionResult {
    Executed {
        details: TransactionExecutionDetails,
        programs_modified_by_tx: Box<LoadedProgramsForTxBatch>,
    },
    NotExecuted(TransactionError),
}

impl TransactionExecutionResult {
    pub fn was_executed_successfully(&self) -> bool {
        match self {
            Self::Executed { details, .. } => details.status.is_ok(),
            Self::NotExecuted { .. } => false,
        }
    }

    pub fn was_executed(&self) -> bool {
        match self {
            Self::Executed { .. } => true,
            Self::NotExecuted(_) => false,
        }
    }

    pub fn details(&self) -> Option<&TransactionExecutionDetails> {
        match self {
            Self::Executed { details, .. } => Some(details),
            Self::NotExecuted(_) => None,
        }
    }

    pub fn flattened_result(&self) -> transaction::Result<()> {
        match self {
            Self::Executed { details, .. } => details.status.clone(),
            Self::NotExecuted(err) => Err(err.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionExecutionDetails {
    pub status: transaction::Result<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub durable_nonce_fee: Option<DurableNonceFee>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    /// The change in accounts data len for this transaction.
    /// NOTE: This value is valid IFF `status` is `Ok`.
    pub accounts_data_len_delta: i64,
}

#[derive(Debug, Clone)]
pub enum DurableNonceFee {
    Valid(u64),
    Invalid,
}

impl From<&NonceFull> for DurableNonceFee {
    fn from(nonce: &NonceFull) -> Self {
        match nonce.lamports_per_signature() {
            Some(lamports_per_signature) => Self::Valid(lamports_per_signature),
            None => Self::Invalid,
        }
    }
}

impl DurableNonceFee {
    pub fn lamports_per_signature(&self) -> Option<u64> {
        match self {
            Self::Valid(lamports_per_signature) => Some(*lamports_per_signature),
            Self::Invalid => None,
        }
    }
}
