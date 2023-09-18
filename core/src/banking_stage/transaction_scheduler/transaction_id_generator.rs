use crate::banking_stage::scheduler_messages::TransactionId;

/// Simple sequential ID generator for `TransactionId`s.
/// These IDs uniquely identify transactions during the scheduling process.
#[derive(Default)]
pub struct TransactionIdGenerator {
    next_id: u64,
}

impl TransactionIdGenerator {
    pub fn next(&mut self) -> TransactionId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        TransactionId::new(id)
    }
}
