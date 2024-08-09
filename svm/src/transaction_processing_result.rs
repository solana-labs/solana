use {
    crate::transaction_execution_result::ExecutedTransaction,
    solana_sdk::transaction::Result as TransactionResult,
};

pub type TransactionProcessingResult = TransactionResult<ProcessedTransaction>;
pub type ProcessedTransaction = ExecutedTransaction;

pub trait TransactionProcessingResultExtensions {
    fn was_processed(&self) -> bool;
    fn was_processed_with_successful_result(&self) -> bool;
    fn processed_transaction(&self) -> Option<&ProcessedTransaction>;
    fn processed_transaction_mut(&mut self) -> Option<&mut ProcessedTransaction>;
    fn flattened_result(&self) -> TransactionResult<()>;
}

impl TransactionProcessingResultExtensions for TransactionProcessingResult {
    fn was_processed(&self) -> bool {
        self.is_ok()
    }

    fn was_processed_with_successful_result(&self) -> bool {
        match self {
            Ok(processed_tx) => processed_tx.was_successful(),
            Err(_) => false,
        }
    }

    fn processed_transaction(&self) -> Option<&ProcessedTransaction> {
        match self {
            Ok(processed_tx) => Some(processed_tx),
            Err(_) => None,
        }
    }

    fn processed_transaction_mut(&mut self) -> Option<&mut ProcessedTransaction> {
        match self {
            Ok(processed_tx) => Some(processed_tx),
            Err(_) => None,
        }
    }

    fn flattened_result(&self) -> TransactionResult<()> {
        self.as_ref()
            .map_err(|err| err.clone())
            .and_then(|processed_tx| processed_tx.execution_details.status.clone())
    }
}
