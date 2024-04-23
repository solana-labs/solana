use {
    crate::transaction_error_metrics::TransactionErrorMetrics,
    solana_program_runtime::loaded_programs::ProgramCacheMatchCriteria,
    solana_sdk::{
        account::AccountSharedData, feature_set::FeatureSet, hash::Hash, message::SanitizedMessage,
        pubkey::Pubkey, rent_collector::RentCollector, transaction,
    },
    std::sync::Arc,
};

/// Runtime callbacks for transaction processing.
pub trait TransactionProcessingCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize>;

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData>;

    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64);

    fn get_rent_collector(&self) -> &RentCollector;

    fn get_feature_set(&self) -> Arc<FeatureSet>;

    fn check_account_access(
        &self,
        _message: &SanitizedMessage,
        _account_index: usize,
        _account: &AccountSharedData,
        _error_counters: &mut TransactionErrorMetrics,
    ) -> transaction::Result<()> {
        Ok(())
    }

    fn get_program_match_criteria(&self, _program: &Pubkey) -> ProgramCacheMatchCriteria {
        ProgramCacheMatchCriteria::NoCriteria
    }

    fn add_builtin_account(&self, _name: &str, _program_id: &Pubkey) {}
}
