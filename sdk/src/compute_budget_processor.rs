//! Process compute_budget instructions outside of program-runtime to
//! extract and sanitize limits.
use crate::{
    borsh0_10::try_from_slice_unchecked,
    compute_budget::{self, ComputeBudgetInstruction},
    entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
    instruction::{CompiledInstruction, InstructionError},
    pubkey::Pubkey,
    sanitize::SanitizeError,
};

// TODO - remove the dup in program-runtime::compute_budget.rs
const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;
const MAX_HEAP_FRAME_BYTES: usize = 256 * 1024;

// Contains transaction meta data, currently, set up compute_budget instructions
// to commit or request "resource" allocation for transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionMeta {
    pub updated_heap_bytes: usize,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub accounts_loaded_bytes: usize,
    // NOTE -
    // is_simple_vote can be consolidated into Meta here.
    //
    // other potential compute_budget ix or TLV ix related data here:
    //    pub accounts_written_bytes: usize,
    //    pub storage_bytes: usize,
    //    pub network_usage_bytes: usize,
    // below are meta data _after_ all accounts are loaded
    //    pub durable_nonce_account: Pubkey,
    //    pub cost_basis: something of cost_model,
}

impl Default for TransactionMeta {
    fn default() -> Self {
        TransactionMeta {
            updated_heap_bytes: MIN_HEAP_FRAME_BYTES,
            compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
            compute_unit_price: 0,
            accounts_loaded_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
        }
    }
}

impl TransactionMeta {
    // Processing compute_budget could be part of tx sanitizing, failed to process
    // these instructions will drop the transaction eventually without execution,
    // may as well fail it early.
    // If succeeded, the transaction's specific limits/requests (could be default)
    // are retrieved and returned,
    pub fn process_compute_budget_instruction<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        support_request_units_deprecated: bool,
        enable_request_heap_frame_ix: bool,
        support_set_loaded_accounts_data_size_limit_ix: bool,
    ) -> Result<TransactionMeta, SanitizeError> {
        let mut num_non_compute_budget_instructions: u32 = 0;
        let mut updated_compute_unit_limit = None;
        let mut updated_compute_unit_price = None;
        let mut updated_heap_size = None;
        let mut updated_loaded_accounts_data_size_limit = None;

        for (i, (program_id, instruction)) in instructions.enumerate() {
            if compute_budget::check_id(program_id) {
                let invalid_instruction_data_error = SanitizeError::InstructionError(
                    i as u8,
                    InstructionError::InvalidInstructionData,
                );
                let duplicate_instruction_error = SanitizeError::DuplicateInstruction(i as u8);

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
                        updated_compute_unit_price = Self::support_deprecated_requested_units(
                            additional_fee,
                            compute_unit_limit,
                        );
                    }
                    Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                        if updated_heap_size.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        if Self::sanitize_updated_heap_size(
                            bytes as usize,
                            enable_request_heap_frame_ix,
                        ) {
                            updated_heap_size = Some(bytes as usize);
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
                    Ok(ComputeBudgetInstruction::SetComputeUnitPrice(compute_unit_price)) => {
                        if updated_compute_unit_price.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        updated_compute_unit_price = Some(compute_unit_price);
                    }
                    Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes))
                        if support_set_loaded_accounts_data_size_limit_ix =>
                    {
                        if updated_loaded_accounts_data_size_limit.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        updated_loaded_accounts_data_size_limit = Some(bytes as usize);
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
        let updated_heap_bytes = updated_heap_size
            .unwrap_or(MIN_HEAP_FRAME_BYTES) // loader's default heap_size
            .min(MAX_HEAP_FRAME_BYTES);

        let compute_unit_limit = updated_compute_unit_limit
            .unwrap_or_else(|| {
                num_non_compute_budget_instructions
                    .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
            })
            .min(MAX_COMPUTE_UNIT_LIMIT);

        let compute_unit_price = updated_compute_unit_price.unwrap_or(0).min(u64::MAX);

        let accounts_loaded_bytes = updated_loaded_accounts_data_size_limit
            .unwrap_or(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES)
            .min(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES);

        Ok(TransactionMeta {
            updated_heap_bytes,
            compute_unit_limit,
            compute_unit_price,
            accounts_loaded_bytes,
        })
    }

    fn sanitize_updated_heap_size(bytes: usize, enable_request_heap_frame_ix: bool) -> bool {
        enable_request_heap_frame_ix
            && (MIN_HEAP_FRAME_BYTES..=MAX_HEAP_FRAME_BYTES).contains(&bytes)
            && bytes % 1024 == 0
    }

    // Supports request_units_derpecated ix, returns cu_price if available.
    fn support_deprecated_requested_units(
        additional_fee: u32,
        compute_unit_limit: u32,
    ) -> Option<u64> {
        // TODO: remove support of 'Deprecated' after feature remove_deprecated_request_unit_ix::id() is activated
        type MicroLamports = u128;
        const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

        let micro_lamport_fee: MicroLamports =
            (additional_fee as u128).saturating_mul(MICRO_LAMPORTS_PER_LAMPORT as u128);
        micro_lamport_fee
            .checked_div(compute_unit_limit as u128)
            .map(|cu_price| u64::try_from(cu_price).unwrap_or(u64::MAX))
    }
}
