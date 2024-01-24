use solana_sdk::transaction::SanitizedTransaction;

pub struct Task {
    transaction: SanitizedTransaction,
    index: usize,
}

impl Task {
    pub fn create_task(transaction: SanitizedTransaction, index: usize) -> Self {
        Task { transaction, index }
    }

    pub fn task_index(&self) -> usize {
        self.index
    }

    pub fn transaction(&self) -> &SanitizedTransaction {
        &self.transaction
    }
}
