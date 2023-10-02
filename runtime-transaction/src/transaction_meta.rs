//! Transaction Meta contains data that follows a transaction through the
//! execution pipeline in runtime. Examples of metadata could be limits
//! specified by compute-budget instructions, simple-vote flag, transaction
//! costs, durable nonce account etc;
//!
//! The premiss is if anything qualifies as metadata, then it must be valid
//! and available as long as the transaction itself is valid and available.
//! Hence they are not Option<T> type. Their visibility at different states
//! are defined in traits.
//!
use solana_sdk::hash::Hash;

/// metadata can be extracted statically from sanitized transaction,
/// for example: message hash, simple-vote-tx flag, compute budget limits,
pub trait StaticMeta {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;
    fn updated_heap_bytes(&self) -> usize;
    fn compute_unit_limit(&self) -> u32;
    fn compute_unit_price(&self) -> u64;
    fn loaded_accounts_bytes(&self) -> usize;
}

/// Statically loaded meta is a supertrait of Dynamically loaded meta, when
/// transaction transited successfully into dynamically loaded, it should
/// have both meta data populated and available.
/// Dynamic metadata available after accounts addresses are loaded from
/// on-chain ALT, examples are: transaction usage costs, nonce account.
pub trait DynamicMeta: StaticMeta {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionMeta {
    pub message_hash: Hash,
    pub is_simple_vote_tx: bool,
    pub updated_heap_bytes: usize,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub loaded_accounts_bytes: usize,
}

impl TransactionMeta {
    pub fn set_message_hash(&mut self, message_hash: Hash) {
        self.message_hash = message_hash;
    }

    pub fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }

    pub fn set_compute_budget_limits(&mut self, compute_budget_limits: &ComputeBudgetLimits) {
        self.updated_heap_bytes = compute_budget_limits.updated_heap_bytes;
        self.compute_unit_limit = compute_budget_limits.compute_unit_limit;
        self.compute_unit_price = compute_budget_limits.compute_unit_price;
        self.loaded_accounts_bytes = compute_budget_limits.loaded_accounts_bytes;
    }
}
