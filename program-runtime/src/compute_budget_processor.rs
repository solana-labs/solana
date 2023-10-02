//! Process compute_budget instructions to extract and sanitize limits.
use {
    crate::compute_budget::DEFAULT_HEAP_COST,
    solana_sdk::{
        borsh0_10::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
        feature_set::{
            add_set_tx_loaded_accounts_data_size_instruction, remove_deprecated_request_unit_ix,
            FeatureSet,
        },
        fee::FeeBudgetLimits,
        instruction::{CompiledInstruction, InstructionError},
        pubkey::Pubkey,
        transaction::TransactionError,
    },
};

const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;
pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// The total accounts data a transaction can load is limited to 64MiB to not break
/// anyone in Mainnet-beta today. It can be set by set_loaded_accounts_data_size_limit instruction
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: u32 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeBudgetLimits {
    pub updated_heap_bytes: u32,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub loaded_accounts_bytes: u32,
}

impl Default for ComputeBudgetLimits {
    fn default() -> Self {
        ComputeBudgetLimits {
            updated_heap_bytes: u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap(),
            compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
            compute_unit_price: 0,
            loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
        }
    }
}

impl Into<FeeBudgetLimits> for ComputeBudgetLimits {
    fn into(self) -> FeeBudgetLimits {
        FeeBudgetLimits {
            // NOTE - usize::from(u32).unwrap() may fail if target is 16-bit and
            // `loaded_accounts_bytes` is greater than u16::MAX. In that case, panic is proper.
            loaded_accounts_data_size_limit: usize::try_from(self.loaded_accounts_bytes).unwrap(),
            heap_cost: DEFAULT_HEAP_COST,
            compute_unit_limit: u64::from(self.compute_unit_limit),
            prioritization_fee: self.compute_unit_price,
        }
    }
}

// Processing compute_budget could be part of tx sanitizing, failed to process
// these instructions will drop the transaction eventually without execution,
// may as well fail it early.
// If succeeded, the transaction's specific limits/requests (could be default)
// are retrieved and returned,
pub fn process_compute_budget_instructions<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
    feature_set: &FeatureSet,
) -> Result<ComputeBudgetLimits, TransactionError> {
    let support_request_units_deprecated =
        !feature_set.is_active(&remove_deprecated_request_unit_ix::id());
    let support_set_loaded_accounts_data_size_limit_ix =
        feature_set.is_active(&add_set_tx_loaded_accounts_data_size_instruction::id());

    let mut num_non_compute_budget_instructions: u32 = 0;
    let mut updated_compute_unit_limit = None;
    let mut updated_compute_unit_price = None;
    let mut requested_heap_size = None;
    let mut updated_loaded_accounts_data_size_limit = None;

    for (i, (program_id, instruction)) in instructions.enumerate() {
        if compute_budget::check_id(program_id) {
            let invalid_instruction_data_error = TransactionError::InstructionError(
                i as u8,
                InstructionError::InvalidInstructionData,
            );
            let duplicate_instruction_error = TransactionError::DuplicateInstruction(i as u8);

            match try_from_slice_unchecked(&instruction.data) {
                Ok(ComputeBudgetInstruction::RequestUnitsDeprecated {
                    units: compute_unit_limit,
                    additional_fee,
                }) if support_request_units_deprecated => {
                    if updated_compute_unit_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    if updated_compute_unit_price.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_limit = Some(compute_unit_limit);
                    updated_compute_unit_price =
                        support_deprecated_requested_units(additional_fee, compute_unit_limit);
                }
                Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                    if requested_heap_size.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    if sanitize_requested_heap_size(bytes) {
                        requested_heap_size = Some(bytes);
                    } else {
                        return Err(invalid_instruction_data_error);
                    }
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitLimit(compute_unit_limit)) => {
                    if updated_compute_unit_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_limit = Some(compute_unit_limit);
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                    if updated_compute_unit_price.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_price = Some(micro_lamports);
                }
                Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes))
                    if support_set_loaded_accounts_data_size_limit_ix =>
                {
                    if updated_loaded_accounts_data_size_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_loaded_accounts_data_size_limit = Some(bytes);
                }
                _ => return Err(invalid_instruction_data_error),
            }
        } else {
            // only include non-request instructions in default max calc
            num_non_compute_budget_instructions =
                num_non_compute_budget_instructions.saturating_add(1);
        }
    }

    // sanitize limits
    let updated_heap_bytes = requested_heap_size
        .unwrap_or(u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap()) // loader's default heap_size
        .min(MAX_HEAP_FRAME_BYTES);

    let compute_unit_limit = updated_compute_unit_limit
        .unwrap_or_else(|| {
            num_non_compute_budget_instructions
                .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
        })
        .min(MAX_COMPUTE_UNIT_LIMIT);

    let compute_unit_price = updated_compute_unit_price.unwrap_or(0).min(u64::MAX);

    let loaded_accounts_bytes = updated_loaded_accounts_data_size_limit
        .unwrap_or(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES)
        .min(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES);

    Ok(ComputeBudgetLimits {
        updated_heap_bytes,
        compute_unit_limit,
        compute_unit_price,
        loaded_accounts_bytes,
    })
}

fn sanitize_requested_heap_size(bytes: u32) -> bool {
    (u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap()..=MAX_HEAP_FRAME_BYTES).contains(&bytes)
        && bytes % 1024 == 0
}

// Supports request_units_derpecated ix, returns cu_price if available.
fn support_deprecated_requested_units(additional_fee: u32, compute_unit_limit: u32) -> Option<u64> {
    // TODO: remove support of 'Deprecated' after feature remove_deprecated_request_unit_ix::id() is activated
    type MicroLamports = u128;
    const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

    let micro_lamport_fee: MicroLamports =
        (additional_fee as u128).saturating_mul(MICRO_LAMPORTS_PER_LAMPORT as u128);
    micro_lamport_fee
        .checked_div(compute_unit_limit as u128)
        .map(|cu_price| u64::try_from(cu_price).unwrap_or(u64::MAX))
}
