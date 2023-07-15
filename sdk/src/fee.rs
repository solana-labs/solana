//! Fee structures.

use crate::native_token::sol_to_lamports;
#[cfg(not(target_os = "solana"))]
use solana_program::message::SanitizedMessage;

/// A fee and its associated compute unit limit
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct FeeBin {
    /// maximum compute units for which this fee will be charged
    pub limit: u64,
    /// fee in lamports
    pub fee: u64,
}

pub struct FeeBudgetLimits {
    pub loaded_accounts_data_size_limit: usize,
    pub heap_cost: u64,
    pub compute_unit_limit: u64,
    pub prioritization_fee: u64,
}

/// Information used to calculate fees
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FeeStructure {
    /// lamports per signature
    pub lamports_per_signature: u64,
    /// lamports_per_write_lock
    pub lamports_per_write_lock: u64,
    /// Compute unit fee bins
    pub compute_fee_bins: Vec<FeeBin>,
}

pub const ACCOUNT_DATA_COST_PAGE_SIZE: u64 = 32_u64.saturating_mul(1024);

impl FeeStructure {
    pub fn new(
        sol_per_signature: f64,
        sol_per_write_lock: f64,
        compute_fee_bins: Vec<(u64, f64)>,
    ) -> Self {
        let compute_fee_bins = compute_fee_bins
            .iter()
            .map(|(limit, sol)| FeeBin {
                limit: *limit,
                fee: sol_to_lamports(*sol),
            })
            .collect::<Vec<_>>();
        FeeStructure {
            lamports_per_signature: sol_to_lamports(sol_per_signature),
            lamports_per_write_lock: sol_to_lamports(sol_per_write_lock),
            compute_fee_bins,
        }
    }

    pub fn get_max_fee(&self, num_signatures: u64, num_write_locks: u64) -> u64 {
        num_signatures
            .saturating_mul(self.lamports_per_signature)
            .saturating_add(num_write_locks.saturating_mul(self.lamports_per_write_lock))
            .saturating_add(
                self.compute_fee_bins
                    .last()
                    .map(|bin| bin.fee)
                    .unwrap_or_default(),
            )
    }

    pub fn calculate_memory_usage_cost(
        loaded_accounts_data_size_limit: usize,
        heap_cost: u64,
    ) -> u64 {
        (loaded_accounts_data_size_limit as u64)
            .saturating_add(ACCOUNT_DATA_COST_PAGE_SIZE.saturating_sub(1))
            .saturating_div(ACCOUNT_DATA_COST_PAGE_SIZE)
            .saturating_mul(heap_cost)
    }

    /// Calculate fee for `SanitizedMessage`
    #[cfg(not(target_os = "solana"))]
    pub fn calculate_fee(
        &self,
        message: &SanitizedMessage,
        lamports_per_signature: u64,
        budget_limits: &FeeBudgetLimits,
        remove_congestion_multiplier: bool,
        include_loaded_account_data_size_in_fee: bool,
    ) -> u64 {
        // Fee based on compute units and signatures
        let congestion_multiplier = if lamports_per_signature == 0 {
            0.0 // test only
        } else if remove_congestion_multiplier {
            1.0 // multiplier that has no effect
        } else {
            const BASE_CONGESTION: f64 = 5_000.0;
            let current_congestion = BASE_CONGESTION.max(lamports_per_signature as f64);
            BASE_CONGESTION / current_congestion
        };

        let signature_fee = message
            .num_signatures()
            .saturating_mul(self.lamports_per_signature);
        let write_lock_fee = message
            .num_write_locks()
            .saturating_mul(self.lamports_per_write_lock);

        // `compute_fee` covers costs for both requested_compute_units and
        // requested_loaded_account_data_size
        let loaded_accounts_data_size_cost = if include_loaded_account_data_size_in_fee {
            FeeStructure::calculate_memory_usage_cost(
                budget_limits.loaded_accounts_data_size_limit,
                budget_limits.heap_cost,
            )
        } else {
            0_u64
        };
        let total_compute_units =
            loaded_accounts_data_size_cost.saturating_add(budget_limits.compute_unit_limit);
        let compute_fee = self
            .compute_fee_bins
            .iter()
            .find(|bin| total_compute_units <= bin.limit)
            .map(|bin| bin.fee)
            .unwrap_or_else(|| {
                self.compute_fee_bins
                    .last()
                    .map(|bin| bin.fee)
                    .unwrap_or_default()
            });

        ((budget_limits
            .prioritization_fee
            .saturating_add(signature_fee)
            .saturating_add(write_lock_fee)
            .saturating_add(compute_fee) as f64)
            * congestion_multiplier)
            .round() as u64
    }
}

impl Default for FeeStructure {
    fn default() -> Self {
        Self::new(0.000005, 0.0, vec![(1_400_000, 0.0)])
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for FeeStructure {
    fn example() -> Self {
        FeeStructure::default()
    }
}
