//! util to support api `sanitized_transaction.get_transaction_meta() -> Result<TransactionMeta>`
//! for now, which will be replaced by:
//! ```sanitized_transaction.get_transaction_meta() -> &TransactionMeta```
//! when compute_budget instructions are processed suring transaction sanitizing.
//!
use crate::{
    compute_budget_processor::process_compute_budget_instruction,
    feature_set::FeatureSet,
    genesis_config::ClusterType,
    message::SanitizedMessage,
    transaction::{Result, SanitizedTransaction, SanitizedVersionedTransaction},
    transaction_meta::TransactionMeta,
};

pub trait GetTransactionMeta {
    fn get_transaction_meta(
        &self,
        feature_set: &FeatureSet,
        maybe_cluster_type: Option<ClusterType>,
    ) -> Result<TransactionMeta>;
}

impl GetTransactionMeta for SanitizedVersionedTransaction {
    fn get_transaction_meta(
        &self,
        feature_set: &FeatureSet,
        maybe_cluster_type: Option<ClusterType>,
    ) -> Result<TransactionMeta> {
        Ok(process_compute_budget_instruction(
            self.get_message().program_instructions_iter(),
            feature_set,
            maybe_cluster_type,
        )?)
    }
}

impl GetTransactionMeta for SanitizedTransaction {
    fn get_transaction_meta(
        &self,
        feature_set: &FeatureSet,
        maybe_cluster_type: Option<ClusterType>,
    ) -> Result<TransactionMeta> {
        Ok(process_compute_budget_instruction(
            self.message().program_instructions_iter(),
            feature_set,
            maybe_cluster_type,
        )?)
    }
}

// NOTE: bank.get_fee_for_message(&self, message: &SanitizedMessage) requires getting
// transaction meta from message, this may requires `transaction_meta` to be reside
// with Sanitized[Versioned]Message.
impl GetTransactionMeta for SanitizedMessage {
    fn get_transaction_meta(
        &self,
        feature_set: &FeatureSet,
        maybe_cluster_type: Option<ClusterType>,
    ) -> Result<TransactionMeta> {
        Ok(process_compute_budget_instruction(
            self.program_instructions_iter(),
            feature_set,
            maybe_cluster_type,
        )?)
    }
}
