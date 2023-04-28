use crate::banking_stage::scheduler_messages::TransactionId;

pub struct TransactionIdGenerator {
    index: u64,
}

impl Default for TransactionIdGenerator {
    fn default() -> Self {
        Self { index: u64::MAX }
    }
}

impl TransactionIdGenerator {
    pub fn next(&mut self) -> TransactionId {
        let index = self.index;
        self.index = self.index.wrapping_sub(1);
        TransactionId::new(index)
    }
}
