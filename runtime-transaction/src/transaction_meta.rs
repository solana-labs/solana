//! Transaction Meta contains data that follows a transaction through the
//! execution pipeline in runtime. Examples of metadata could be limits
//! specified by compute-budget instructions, simple-vote flag, transaction
//! costs, durable nonce account etc;
//!
//! The premiss is metadata must be valid and available WITHIN SAME EPOCH
//! as long as the transaction itself is valid and available.
//! Hence they are not Option<T> type. Their visibility at different states
//! are defined in traits.
//!
use {
    solana_program_runtime::compute_budget_processor::ComputeBudgetLimits,
    solana_sdk::{hash::Hash, slot_history::Slot},
};

/// metadata can be extracted statically from sanitized transaction,
/// for example: message hash, simple-vote-tx flag, compute budget limits,
pub trait StaticMeta {
    fn message_hash(&self) -> &Hash;
    fn is_simple_vote_tx(&self) -> bool;

    // cached compute_budget_limits expires at the last slot of epoch when it is
    // extracted.
    fn compute_budget_limits(&self, current_slot: Slot) -> Option<&ComputeBudgetLimits>;
}

/// Statically loaded meta is a supertrait of Dynamically loaded meta, when
/// transaction transited successfully into dynamically loaded, it should
/// have both meta data populated and available.
/// Dynamic metadata available after accounts addresses are loaded from
/// on-chain ALT, examples are: transaction usage costs, nonce account.
pub trait DynamicMeta: StaticMeta {}

#[derive(Debug, Default)]
pub struct TransactionMeta {
    pub(crate) message_hash: Hash,
    pub(crate) is_simple_vote_tx: bool,

    // currently resolving compute_budget_limits requires feature_set,
    // which could change between epochs. Therefore resolved compute_budget_limits
    // needs to be stamped with the last slot of current epoch.
    pub(crate) compute_budget_limits_ttl: ComputeBudgetLimitsTTL,
}

#[derive(Debug, Default)]
pub(crate) struct ComputeBudgetLimitsTTL {
    pub(crate) compute_budget_limits: ComputeBudgetLimits,
    pub(crate) max_age_slot: Slot,
}

impl TransactionMeta {
    pub fn set_message_hash(&mut self, message_hash: Hash) {
        self.message_hash = message_hash;
    }

    pub fn set_is_simple_vote_tx(&mut self, is_simple_vote_tx: bool) {
        self.is_simple_vote_tx = is_simple_vote_tx;
    }

    pub fn set_compute_budget_limits(
        &mut self,
        compute_budget_limits: ComputeBudgetLimits,
        max_age_slot: Slot,
    ) {
        self.compute_budget_limits_ttl = ComputeBudgetLimitsTTL {
            compute_budget_limits,
            max_age_slot,
        };
    }
}
