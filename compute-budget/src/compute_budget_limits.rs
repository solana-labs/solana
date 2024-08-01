use {
    crate::prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
    solana_sdk::{entrypoint::HEAP_LENGTH, fee::FeeBudgetLimits},
    std::num::NonZeroU32,
};

/// Roughly 0.5us/page, where page is 32K; given roughly 15CU/us, the
/// default heap page cost = 0.5 * 15 ~= 8CU/page
pub const DEFAULT_HEAP_COST: u64 = 8;
pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;
pub const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;
pub const MIN_HEAP_FRAME_BYTES: u32 = HEAP_LENGTH as u32;

/// The total accounts data a transaction can load is limited to 64MiB to not break
/// anyone in Mainnet-beta today. It can be set by set_loaded_accounts_data_size_limit instruction
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: NonZeroU32 =
    unsafe { NonZeroU32::new_unchecked(64 * 1024 * 1024) };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputeBudgetLimits {
    pub updated_heap_bytes: u32,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub loaded_accounts_bytes: NonZeroU32,
}

impl Default for ComputeBudgetLimits {
    fn default() -> Self {
        ComputeBudgetLimits {
            updated_heap_bytes: MIN_HEAP_FRAME_BYTES,
            compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
            compute_unit_price: 0,
            loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
        }
    }
}

impl From<ComputeBudgetLimits> for FeeBudgetLimits {
    fn from(val: ComputeBudgetLimits) -> Self {
        let prioritization_fee_details = PrioritizationFeeDetails::new(
            PrioritizationFeeType::ComputeUnitPrice(val.compute_unit_price),
            u64::from(val.compute_unit_limit),
        );
        let prioritization_fee = prioritization_fee_details.get_fee();

        FeeBudgetLimits {
            loaded_accounts_data_size_limit: val.loaded_accounts_bytes,
            heap_cost: DEFAULT_HEAP_COST,
            compute_unit_limit: u64::from(val.compute_unit_limit),
            prioritization_fee,
        }
    }
}
