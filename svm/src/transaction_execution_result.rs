// Re-exported since these have moved to `solana_sdk`.
#[deprecated(
    since = "1.18.0",
    note = "Please use `solana_sdk::inner_instruction` types instead"
)]
pub use solana_sdk::inner_instruction::{InnerInstruction, InnerInstructionsList};
use {
    crate::account_loader::LoadedTransaction,
    solana_program_runtime::loaded_programs::ProgramCacheEntry,
    solana_sdk::{
        pubkey::Pubkey,
        transaction::{self, TransactionError},
        transaction_context::TransactionReturnData,
    },
    std::{collections::HashMap, sync::Arc},
};

#[derive(Debug, Default, Clone)]
pub struct TransactionLoadedAccountsStats {
    pub loaded_accounts_data_size: u32,
    pub loaded_accounts_count: usize,
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
    Executed(Box<ExecutedTransaction>),
    NotExecuted(TransactionError),
}

#[derive(Debug, Clone)]
pub struct ExecutedTransaction {
    pub loaded_transaction: LoadedTransaction,
    pub execution_details: TransactionExecutionDetails,
    pub programs_modified_by_tx: HashMap<Pubkey, Arc<ProgramCacheEntry>>,
}

impl ExecutedTransaction {
    pub fn was_successful(&self) -> bool {
        self.execution_details.status.is_ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionExecutionDetails {
    pub status: transaction::Result<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    /// The change in accounts data len for this transaction.
    /// NOTE: This value is valid IFF `status` is `Ok`.
    pub accounts_data_len_delta: i64,
}

impl TransactionExecutionDetails {
    pub fn was_successful(&self) -> bool {
        self.status.is_ok()
    }
}
