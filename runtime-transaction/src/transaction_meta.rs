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
/// for example: message hash, simple-vote-tx flag, limits set by instructions
pub trait StaticMeta {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;
    fn compute_unit_limit(&self) -> u32;
    fn compute_unit_price(&self) -> u64;
    fn loaded_accounts_bytes(&self) -> u32;
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
    pub(crate) compute_unit_limit: u32,
    pub(crate) compute_unit_price: u64,
    pub(crate) loaded_accounts_bytes: u32,
}

impl TransactionMeta {
    pub(crate) fn set_message_hash(&mut self, message_hash: Hash) {
        self.message_hash = message_hash;
    }

    pub(crate) fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }

    pub(crate) fn set_compute_unit_limit(&mut self, compute_unit_limit: u32) {
        self.compute_unit_limit = compute_unit_limit;
    }

    pub(crate) fn set_compute_unit_price(&mut self, compute_unit_price: u64) {
        self.compute_unit_price = compute_unit_price;
    }

    pub(crate) fn set_loaded_accounts_bytes(&mut self, loaded_accounts_bytes: u32) {
        self.loaded_accounts_bytes = loaded_accounts_bytes;
    }
}
