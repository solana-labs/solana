use crate::banking_stage::scheduler_messages::TransactionBatchId;

pub struct BatchIdGenerator {
    index: u64,
}

impl Default for BatchIdGenerator {
    fn default() -> Self {
        Self { index: u64::MAX }
    }
}

impl BatchIdGenerator {
    pub fn next(&mut self) -> TransactionBatchId {
        let index = self.index;
        self.index = self.index.wrapping_sub(1);
        TransactionBatchId::new(index)
    }
}
