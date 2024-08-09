use {
    crate::transaction_execution_result::{
        TransactionExecutionDetails, TransactionLoadedAccountsStats,
    },
    solana_sdk::{
        fee::FeeDetails, rent_debits::RentDebits, transaction::Result as TransactionResult,
    },
};

pub type TransactionCommitResult = TransactionResult<CommittedTransaction>;

#[derive(Clone, Debug)]
pub struct CommittedTransaction {
    pub loaded_account_stats: TransactionLoadedAccountsStats,
    pub execution_details: TransactionExecutionDetails,
    pub fee_details: FeeDetails,
    pub rent_debits: RentDebits,
}

pub trait TransactionCommitResultExtensions {
    fn was_committed(&self) -> bool;
    fn was_executed_successfully(&self) -> bool;
    fn transaction_result(&self) -> TransactionResult<()>;
}

impl TransactionCommitResultExtensions for TransactionCommitResult {
    fn was_committed(&self) -> bool {
        self.is_ok()
    }

    fn was_executed_successfully(&self) -> bool {
        match self {
            Ok(committed_tx) => committed_tx.execution_details.status.is_ok(),
            Err(_) => false,
        }
    }

    fn transaction_result(&self) -> TransactionResult<()> {
        self.as_ref()
            .map_err(|err| err.clone())
            .and_then(|committed_tx| committed_tx.execution_details.status.clone())
    }
}
