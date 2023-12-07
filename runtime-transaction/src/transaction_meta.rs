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
use {
    solana_program_runtime::compute_budget_processor::ComputeBudgetLimits,
    solana_sdk::{hash::Hash, slot_history::Slot},
};

/// private trait to avoid exposing `ComputeBudgetLimits` to callsite of RuntimeTransaction.
pub(crate) trait RequestedLimits {
    fn requested_limits(&self, current_slot: Option<Slot>) -> Option<&ComputeBudgetLimits>;
}

/// metadata can be extracted statically from sanitized transaction,
/// for example: message hash, simple-vote-tx flag, limits set by instructions
#[allow(private_bounds)]
pub trait StaticMeta: RequestedLimits {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;

    // get fields' value from RequestedLimitsWithExpiry,
    // `current_slot`, sets to None to skip expiry check, otherwise if current_slot is
    // before expiry, Some(value) is returned, otherwise return None.
    fn compute_unit_limit(&self, current_slot: Option<Slot>) -> Option<u32>;
    fn compute_unit_price(&self, current_slot: Option<Slot>) -> Option<u64>;
    fn loaded_accounts_bytes(&self, current_slot: Option<Slot>) -> Option<u32>;
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
    pub(crate) requested_limits: RequestedLimitsWithExpiry,
}

/// Processing compute_budget_instructions with feature_set resolves RequestedLimits,
/// and the resolved values expire at the end of the current epoch, as feature_set
/// may change between epochs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RequestedLimitsWithExpiry {
    pub(crate) expiry: Slot,
    pub(crate) compute_budget_limits: ComputeBudgetLimits,
}

impl TransactionMeta {
    pub(crate) fn set_message_hash(&mut self, message_hash: Hash) {
        self.message_hash = message_hash;
    }

    pub(crate) fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }
}
