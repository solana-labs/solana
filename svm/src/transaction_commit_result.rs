use {
    crate::transaction_execution_result::TransactionLoadedAccountsStats,
    solana_sdk::{
        fee::FeeDetails, inner_instruction::InnerInstructionsList, rent_debits::RentDebits,
        transaction::Result as TransactionResult, transaction_context::TransactionReturnData,
    },
};

pub type TransactionCommitResult = TransactionResult<CommittedTransaction>;

#[derive(Clone, Debug)]
pub struct CommittedTransaction {
    pub status: TransactionResult<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    pub fee_details: FeeDetails,
    pub rent_debits: RentDebits,
    pub loaded_account_stats: TransactionLoadedAccountsStats,
}

pub trait TransactionCommitResultExtensions {
    fn was_committed(&self) -> bool;
    fn was_executed_successfully(&self) -> bool;
}

impl TransactionCommitResultExtensions for TransactionCommitResult {
    fn was_committed(&self) -> bool {
        self.is_ok()
    }

    fn was_executed_successfully(&self) -> bool {
        match self {
            Ok(committed_tx) => committed_tx.status.is_ok(),
            Err(_) => false,
        }
    }
}
