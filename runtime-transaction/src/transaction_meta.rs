//! Transaction Meta contains data that follows a transaction through the
//! execution pipeline in runtime. Examples of metadata could be limits
//! specified by compute-budget instructions, simple-vote flag, transaction
//! costs, durable nonce account etc;
//!
//! The premise is if anything qualifies as metadata, then it must be valid
//! and available as long as the transaction itself is valid and available.
//! Hence they are not Option<T> type. Their visibility at different states
//! are defined in traits.
//!
//! The StaticMeta and DynamicMeta traits are accessor traits on the
//! RuntimeTransaction types, not the TransactionMeta itself.
//!
use solana_sdk::hash::Hash;

/// metadata can be extracted statically from sanitized transaction,
/// for example: message hash, simple-vote-tx flag, compute budget limits,
pub trait StaticMeta {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;
}

/// Statically loaded meta is a supertrait of Dynamically loaded meta, when
/// transaction transited successfully into dynamically loaded, it should
/// have both meta data populated and available.
/// Dynamic metadata available after accounts addresses are loaded from
/// on-chain ALT, examples are: transaction usage costs, nonce account.
pub trait DynamicMeta: StaticMeta {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionMeta {
    pub(crate) message_hash: Hash,
    pub(crate) is_simple_vote_tx: bool,
}

impl TransactionMeta {
    pub fn set_message_hash(&mut self, message_hash: Hash) {
        self.message_hash = message_hash;
    }

    pub fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }
}
