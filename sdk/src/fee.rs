//! Fee structures.

use crate::native_token::sol_to_lamports;

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
