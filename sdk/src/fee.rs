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

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeeDetails {
    transaction_fee: u64,
    prioritization_fee: u64,
    remove_rounding_in_fee_calculation: bool,
}

impl FeeDetails {
    #[cfg(feature = "dev-context-only-utils")]
    pub fn new_for_tests(
        transaction_fee: u64,
        prioritization_fee: u64,
        remove_rounding_in_fee_calculation: bool,
    ) -> Self {
        Self {
            transaction_fee,
            prioritization_fee,
            remove_rounding_in_fee_calculation,
        }
    }

    pub fn total_fee(&self) -> u64 {
        let total_fee = self.transaction_fee.saturating_add(self.prioritization_fee);
        if self.remove_rounding_in_fee_calculation {
            total_fee
        } else {
            // backward compatible behavior
            (total_fee as f64).round() as u64
        }
    }

    pub fn accumulate(&mut self, fee_details: &FeeDetails) {
        self.transaction_fee = self
            .transaction_fee
            .saturating_add(fee_details.transaction_fee);
        self.prioritization_fee = self
            .prioritization_fee
            .saturating_add(fee_details.prioritization_fee)
    }

    pub fn transaction_fee(&self) -> u64 {
        self.transaction_fee
    }

    pub fn prioritization_fee(&self) -> u64 {
        self.prioritization_fee
    }
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
        include_loaded_account_data_size_in_fee: bool,
        remove_rounding_in_fee_calculation: bool,
    ) -> u64 {
        self.calculate_fee_details(
            message,
            lamports_per_signature,
            budget_limits,
            include_loaded_account_data_size_in_fee,
            remove_rounding_in_fee_calculation,
        )
        .total_fee()
    }

    /// Calculate fee details for `SanitizedMessage`
    #[cfg(not(target_os = "solana"))]
    pub fn calculate_fee_details(
        &self,
        message: &SanitizedMessage,
        lamports_per_signature: u64,
        budget_limits: &FeeBudgetLimits,
        include_loaded_account_data_size_in_fee: bool,
        remove_rounding_in_fee_calculation: bool,
    ) -> FeeDetails {
        // Backward compatibility - lamports_per_signature == 0 means to clear
        // transaction fee to zero
        if lamports_per_signature == 0 {
            return FeeDetails::default();
        }

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

        FeeDetails {
            transaction_fee: signature_fee
                .saturating_add(write_lock_fee)
                .saturating_add(compute_fee),
            prioritization_fee: budget_limits.prioritization_fee,
            remove_rounding_in_fee_calculation,
        }
    }
}

impl Default for FeeStructure {
    fn default() -> Self {
        Self::new(0.000005, 0.0, vec![(1_400_000, 0.0)])
    }
}

#[cfg(all(RUSTC_WITH_SPECIALIZATION, feature = "frozen-abi"))]
impl ::solana_frozen_abi::abi_example::AbiExample for FeeStructure {
    fn example() -> Self {
        FeeStructure::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_memory_usage_cost() {
        let heap_cost = 99;
        const K: usize = 1024;

        // accounts data size are priced in block of 32K, ...

        // ... requesting less than 32K should still be charged as one block
        assert_eq!(
            heap_cost,
            FeeStructure::calculate_memory_usage_cost(31 * K, heap_cost)
        );

        // ... requesting exact 32K should be charged as one block
        assert_eq!(
            heap_cost,
            FeeStructure::calculate_memory_usage_cost(32 * K, heap_cost)
        );

        // ... requesting slightly above 32K should be charged as 2 block
        assert_eq!(
            heap_cost * 2,
            FeeStructure::calculate_memory_usage_cost(33 * K, heap_cost)
        );

        // ... requesting exact 64K should be charged as 2 block
        assert_eq!(
            heap_cost * 2,
            FeeStructure::calculate_memory_usage_cost(64 * K, heap_cost)
        );
    }

    #[test]
    fn test_total_fee_rounding() {
        // round large `f64` can lost precision, see feature gate:
        // "Removing unwanted rounding in fee calculation #34982"

        let transaction_fee = u64::MAX - 11;
        let prioritization_fee = 1;
        let expected_large_fee = u64::MAX - 10;

        let details_with_rounding = FeeDetails {
            transaction_fee,
            prioritization_fee,
            remove_rounding_in_fee_calculation: false,
        };
        let details_without_rounding = FeeDetails {
            transaction_fee,
            prioritization_fee,
            remove_rounding_in_fee_calculation: true,
        };

        assert_eq!(details_without_rounding.total_fee(), expected_large_fee);
        assert_ne!(details_with_rounding.total_fee(), expected_large_fee);
    }
}
