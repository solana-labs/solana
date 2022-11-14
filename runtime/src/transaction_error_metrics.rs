use solana_sdk::{clock::Slot, saturating_add_assign};

#[derive(Debug, Default)]
pub struct TransactionErrorMetrics {
    pub total: usize,
    pub account_in_use: usize,
    pub too_many_account_locks: usize,
    pub account_loaded_twice: usize,
    pub account_not_found: usize,
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub call_chain_too_deep: usize,
    pub already_processed: usize,
    pub instruction_error: usize,
    pub insufficient_funds: usize,
    pub invalid_account_for_fee: usize,
    pub invalid_account_index: usize,
    pub invalid_program_for_execution: usize,
    pub not_allowed_during_cluster_maintenance: usize,
    pub invalid_writable_account: usize,
    pub invalid_rent_paying_account: usize,
    pub would_exceed_max_block_cost_limit: usize,
    pub would_exceed_max_account_cost_limit: usize,
    pub would_exceed_max_vote_cost_limit: usize,
    pub would_exceed_account_data_block_limit: usize,
    pub max_loaded_accounts_data_size_exceeded: usize,
    pub invalid_loaded_accounts_data_size_limit: usize,
}

impl TransactionErrorMetrics {
    pub fn new() -> Self {
        Self { ..Self::default() }
    }

    pub fn accumulate(&mut self, other: &TransactionErrorMetrics) {
        saturating_add_assign!(self.total, other.total);
        saturating_add_assign!(self.account_in_use, other.account_in_use);
        saturating_add_assign!(self.too_many_account_locks, other.too_many_account_locks);
        saturating_add_assign!(self.account_loaded_twice, other.account_loaded_twice);
        saturating_add_assign!(self.account_not_found, other.account_not_found);
        saturating_add_assign!(self.blockhash_not_found, other.blockhash_not_found);
        saturating_add_assign!(self.blockhash_too_old, other.blockhash_too_old);
        saturating_add_assign!(self.call_chain_too_deep, other.call_chain_too_deep);
        saturating_add_assign!(self.already_processed, other.already_processed);
        saturating_add_assign!(self.instruction_error, other.instruction_error);
        saturating_add_assign!(self.insufficient_funds, other.insufficient_funds);
        saturating_add_assign!(self.invalid_account_for_fee, other.invalid_account_for_fee);
        saturating_add_assign!(self.invalid_account_index, other.invalid_account_index);
        saturating_add_assign!(
            self.invalid_program_for_execution,
            other.invalid_program_for_execution
        );
        saturating_add_assign!(
            self.not_allowed_during_cluster_maintenance,
            other.not_allowed_during_cluster_maintenance
        );
        saturating_add_assign!(
            self.invalid_writable_account,
            other.invalid_writable_account
        );
        saturating_add_assign!(
            self.invalid_rent_paying_account,
            other.invalid_rent_paying_account
        );
        saturating_add_assign!(
            self.would_exceed_max_block_cost_limit,
            other.would_exceed_max_block_cost_limit
        );
        saturating_add_assign!(
            self.would_exceed_max_account_cost_limit,
            other.would_exceed_max_account_cost_limit
        );
        saturating_add_assign!(
            self.would_exceed_max_vote_cost_limit,
            other.would_exceed_max_vote_cost_limit
        );
        saturating_add_assign!(
            self.would_exceed_account_data_block_limit,
            other.would_exceed_account_data_block_limit
        );
        saturating_add_assign!(
            self.max_loaded_accounts_data_size_exceeded,
            other.max_loaded_accounts_data_size_exceeded
        );
        saturating_add_assign!(
            self.invalid_loaded_accounts_data_size_limit,
            other.invalid_loaded_accounts_data_size_limit
        );
    }

    pub fn report(&self, id: u32, slot: Slot) {
        datapoint_info!(
            "banking_stage-leader_slot_transaction_errors",
            ("id", id as i64, i64),
            ("slot", slot as i64, i64),
            ("total", self.total as i64, i64),
            ("account_in_use", self.account_in_use as i64, i64),
            (
                "too_many_account_locks",
                self.too_many_account_locks as i64,
                i64
            ),
            (
                "account_loaded_twice",
                self.account_loaded_twice as i64,
                i64
            ),
            ("account_not_found", self.account_not_found as i64, i64),
            ("blockhash_not_found", self.blockhash_not_found as i64, i64),
            ("blockhash_too_old", self.blockhash_too_old as i64, i64),
            ("call_chain_too_deep", self.call_chain_too_deep as i64, i64),
            ("already_processed", self.already_processed as i64, i64),
            ("instruction_error", self.instruction_error as i64, i64),
            ("insufficient_funds", self.insufficient_funds as i64, i64),
            (
                "invalid_account_for_fee",
                self.invalid_account_for_fee as i64,
                i64
            ),
            (
                "invalid_account_index",
                self.invalid_account_index as i64,
                i64
            ),
            (
                "invalid_program_for_execution",
                self.invalid_program_for_execution as i64,
                i64
            ),
            (
                "not_allowed_during_cluster_maintenance",
                self.not_allowed_during_cluster_maintenance as i64,
                i64
            ),
            (
                "invalid_writable_account",
                self.invalid_writable_account as i64,
                i64
            ),
            (
                "invalid_rent_paying_account",
                self.invalid_rent_paying_account as i64,
                i64
            ),
            (
                "would_exceed_max_block_cost_limit",
                self.would_exceed_max_block_cost_limit as i64,
                i64
            ),
            (
                "would_exceed_max_account_cost_limit",
                self.would_exceed_max_account_cost_limit as i64,
                i64
            ),
            (
                "would_exceed_max_vote_cost_limit",
                self.would_exceed_max_vote_cost_limit as i64,
                i64
            ),
            (
                "would_exceed_account_data_block_limit",
                self.would_exceed_account_data_block_limit as i64,
                i64
            ),
            (
                "max_loaded_accounts_data_size_exceeded",
                self.max_loaded_accounts_data_size_exceeded as i64,
                i64
            ),
            (
                "invalid_loaded_accounts_data_size_limit",
                self.invalid_loaded_accounts_data_size_limit as i64,
                i64
            ),
        );
    }
}
