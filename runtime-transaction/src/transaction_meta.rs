//! Transaction Meta contains data that follows a transaction through the
//! execution pipeline in runtime. Metadata can include limits specified by
//! compute-budget instructions, simple-vote flag, transactino costs,
//! durable nonce account etc; Metadata can be lazily populated as
//! transaction goes through execution path.
//!

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionMeta {
    pub is_simple_vote_tx: bool,
}

impl TransactionMeta {
    pub fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }
}
