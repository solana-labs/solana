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
    crate::compute_budget_instruction_details::ComputeBudgetInstructionDetails,
    solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
    solana_sdk::{feature_set::FeatureSet, hash::Hash, transaction::Result},
};

/// metadata can be extracted statically from sanitized transaction,
/// for example: message hash, simple-vote-tx flag, limits set by instructions
pub trait StaticMeta {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;
    fn compute_budget_limits(&self, feature_set: &FeatureSet) -> Result<ComputeBudgetLimits>;
}

/// Statically loaded meta is a supertrait of Dynamically loaded meta, when
/// transaction transited successfully into dynamically loaded, it should
/// have both meta data populated and available.
/// Dynamic metadata available after accounts addresses are loaded from
/// on-chain ALT, examples are: transaction usage costs, nonce account.
pub trait DynamicMeta: StaticMeta {}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Default)]
pub struct TransactionMeta {
    pub(crate) message_hash: Hash,
    pub(crate) is_simple_vote_tx: bool,
    pub(crate) compute_budget_instruction_details: ComputeBudgetInstructionDetails,
}
