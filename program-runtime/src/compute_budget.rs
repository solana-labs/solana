use {
    crate::prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
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

/// The total accounts data a transaction can load is limited to 64MiB to not break
/// anyone in Mainnet-beta today. It can be set by set_loaded_accounts_data_size_limit instruction
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: usize = 64 * 1024 * 1024;

pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;
const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for ComputeBudget {
    fn example() -> Self {
        // ComputeBudget is not Serialize so just rely on Default.
        ComputeBudget::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputeBudget {
    /// Number of compute units that a transaction or individual instruction is
    /// allowed to consume. Compute units are consumed by program execution,
    /// resources they use, etc...
    pub compute_unit_limit: u64,
    /// Number of compute units consumed by a log_u64 call
    pub log_64_units: u64,
    /// Number of compute units consumed by a create_program_address call
    pub create_program_address_units: u64,
    /// Number of compute units consumed by an invoke call (not including the cost incurred by
    /// the called program)
    pub invoke_units: u64,
    /// Maximum program instruction invocation stack height. Invocation stack
    /// height starts at 1 for transaction instructions and the stack height is
    /// incremented each time a program invokes an instruction and decremented
    /// when a program returns.
    pub max_invoke_stack_height: usize,
    /// Maximum cross-program invocation and instructions per transaction
    pub max_instruction_trace_length: usize,
    /// Base number of compute units consumed to call SHA256
    pub sha256_base_cost: u64,
    /// Incremental number of units consumed by SHA256 (based on bytes)
    pub sha256_byte_cost: u64,
    /// Maximum number of slices hashed per syscall
    pub sha256_max_slices: u64,
    /// Maximum SBF to BPF call depth
    pub max_call_depth: usize,
    /// Size of a stack frame in bytes, must match the size specified in the LLVM SBF backend
    pub stack_frame_size: usize,
    /// Number of compute units consumed by logging a `Pubkey`
    pub log_pubkey_units: u64,
    /// Maximum cross-program invocation instruction size
    pub max_cpi_instruction_size: usize,
    /// Number of account data bytes per compute unit charged during a cross-program invocation
    pub cpi_bytes_per_unit: u64,
    /// Base number of compute units consumed to get a sysvar
    pub sysvar_base_cost: u64,
    /// Number of compute units consumed to call secp256k1_recover
    pub secp256k1_recover_cost: u64,
    /// Number of compute units consumed to do a syscall without any work
    pub syscall_base_cost: u64,
    /// Number of compute units consumed to validate a curve25519 edwards point
    pub curve25519_edwards_validate_point_cost: u64,
    /// Number of compute units consumed to add two curve25519 edwards points
    pub curve25519_edwards_add_cost: u64,
    /// Number of compute units consumed to subtract two curve25519 edwards points
    pub curve25519_edwards_subtract_cost: u64,
    /// Number of compute units consumed to multiply a curve25519 edwards point
    pub curve25519_edwards_multiply_cost: u64,
    /// Number of compute units consumed for a multiscalar multiplication (msm) of edwards points.
    /// The total cost is calculated as `msm_base_cost + (length - 1) * msm_incremental_cost`.
    pub curve25519_edwards_msm_base_cost: u64,
    /// Number of compute units consumed for a multiscalar multiplication (msm) of edwards points.
    /// The total cost is calculated as `msm_base_cost + (length - 1) * msm_incremental_cost`.
    pub curve25519_edwards_msm_incremental_cost: u64,
    /// Number of compute units consumed to validate a curve25519 ristretto point
    pub curve25519_ristretto_validate_point_cost: u64,
    /// Number of compute units consumed to add two curve25519 ristretto points
    pub curve25519_ristretto_add_cost: u64,
    /// Number of compute units consumed to subtract two curve25519 ristretto points
    pub curve25519_ristretto_subtract_cost: u64,
    /// Number of compute units consumed to multiply a curve25519 ristretto point
    pub curve25519_ristretto_multiply_cost: u64,
    /// Number of compute units consumed for a multiscalar multiplication (msm) of ristretto points.
    /// The total cost is calculated as `msm_base_cost + (length - 1) * msm_incremental_cost`.
    pub curve25519_ristretto_msm_base_cost: u64,
    /// Number of compute units consumed for a multiscalar multiplication (msm) of ristretto points.
    /// The total cost is calculated as `msm_base_cost + (length - 1) * msm_incremental_cost`.
    pub curve25519_ristretto_msm_incremental_cost: u64,
    /// program heap region size, default: solana_sdk::entrypoint::HEAP_LENGTH
    pub heap_size: u32,
    /// Number of compute units per additional 32k heap above the default (~.5
    /// us per 32k at 15 units/us rounded up)
    pub heap_cost: u64,
    /// Memory operation syscall base cost
    pub mem_op_base_cost: u64,
    /// Number of compute units consumed to call alt_bn128_addition
    pub alt_bn128_addition_cost: u64,
    /// Number of compute units consumed to call alt_bn128_multiplication.
    pub alt_bn128_multiplication_cost: u64,
    /// Total cost will be alt_bn128_pairing_one_pair_cost_first
    /// + alt_bn128_pairing_one_pair_cost_other * (num_elems - 1)
    pub alt_bn128_pairing_one_pair_cost_first: u64,
    pub alt_bn128_pairing_one_pair_cost_other: u64,
    /// Big integer modular exponentiation cost
    pub big_modular_exponentiation_cost: u64,
    /// Maximum accounts data size, in bytes, that a transaction is allowed to load; The
    /// value is capped by MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES to prevent overuse of memory.
    pub loaded_accounts_data_size_limit: usize,
    /// Coefficient `a` of the quadratic function which determines the number
    /// of compute units consumed to call poseidon syscall for a given number
    /// of inputs.
    pub poseidon_cost_coefficient_a: u64,
    /// Coefficient `c` of the quadratic function which determines the number
    /// of compute units consumed to call poseidon syscall for a given number
    /// of inputs.
    pub poseidon_cost_coefficient_c: u64,
    /// Number of compute units consumed for accessing the remaining compute units.
    pub get_remaining_compute_units_cost: u64,
    /// Number of compute units consumed to call alt_bn128_g1_compress.
    pub alt_bn128_g1_compress: u64,
    /// Number of compute units consumed to call alt_bn128_g1_decompress.
    pub alt_bn128_g1_decompress: u64,
    /// Number of compute units consumed to call alt_bn128_g2_compress.
    pub alt_bn128_g2_compress: u64,
    /// Number of compute units consumed to call alt_bn128_g2_decompress.
    pub alt_bn128_g2_decompress: u64,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self::new(MAX_COMPUTE_UNIT_LIMIT as u64)
    }
}

impl ComputeBudget {
    pub fn new(compute_unit_limit: u64) -> Self {
        ComputeBudget {
            compute_unit_limit,
            log_64_units: 100,
            create_program_address_units: 1500,
            invoke_units: 1000,
            max_invoke_stack_height: 5,
            max_instruction_trace_length: 64,
            sha256_base_cost: 85,
            sha256_byte_cost: 1,
            sha256_max_slices: 20_000,
            max_call_depth: 64,
            stack_frame_size: 4_096,
            log_pubkey_units: 100,
            max_cpi_instruction_size: 1280, // IPv6 Min MTU size
            cpi_bytes_per_unit: 250,        // ~50MB at 200,000 units
            sysvar_base_cost: 100,
            secp256k1_recover_cost: 25_000,
            syscall_base_cost: 100,
            curve25519_edwards_validate_point_cost: 159,
            curve25519_edwards_add_cost: 473,
            curve25519_edwards_subtract_cost: 475,
            curve25519_edwards_multiply_cost: 2_177,
            curve25519_edwards_msm_base_cost: 2_273,
            curve25519_edwards_msm_incremental_cost: 758,
            curve25519_ristretto_validate_point_cost: 169,
            curve25519_ristretto_add_cost: 521,
            curve25519_ristretto_subtract_cost: 519,
            curve25519_ristretto_multiply_cost: 2_208,
            curve25519_ristretto_msm_base_cost: 2303,
            curve25519_ristretto_msm_incremental_cost: 788,
            heap_size: u32::try_from(solana_sdk::entrypoint::HEAP_LENGTH).unwrap(),
            heap_cost: 8,
            mem_op_base_cost: 10,
            alt_bn128_addition_cost: 334,
            alt_bn128_multiplication_cost: 3_840,
            alt_bn128_pairing_one_pair_cost_first: 36_364,
            alt_bn128_pairing_one_pair_cost_other: 12_121,
            big_modular_exponentiation_cost: 33,
            loaded_accounts_data_size_limit: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
            poseidon_cost_coefficient_a: 61,
            poseidon_cost_coefficient_c: 542,
            get_remaining_compute_units_cost: 100,
            alt_bn128_g1_compress: 30,
            alt_bn128_g1_decompress: 398,
            alt_bn128_g2_compress: 86,
            alt_bn128_g2_decompress: 13610,
        }
    }

    pub fn process_instructions<'a>(
        &mut self,
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        support_request_units_deprecated: bool,
        support_set_loaded_accounts_data_size_limit_ix: bool,
    ) -> Result<PrioritizationFeeDetails, TransactionError> {
        let mut num_non_compute_budget_instructions: u32 = 0;
        let mut updated_compute_unit_limit = None;
        let mut requested_heap_size = None;
        let mut prioritization_fee = None;
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
                        if prioritization_fee.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        updated_compute_unit_limit = Some(compute_unit_limit);
                        prioritization_fee =
                            Some(PrioritizationFeeType::Deprecated(additional_fee as u64));
                    }
                    Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                        if requested_heap_size.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        requested_heap_size = Some((bytes, i as u8));
                    }
                    Ok(ComputeBudgetInstruction::SetComputeUnitLimit(compute_unit_limit)) => {
                        if updated_compute_unit_limit.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        updated_compute_unit_limit = Some(compute_unit_limit);
                    }
                    Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                        if prioritization_fee.is_some() {
                            return Err(duplicate_instruction_error);
                        }
                        prioritization_fee =
                            Some(PrioritizationFeeType::ComputeUnitPrice(micro_lamports));
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

        if let Some((bytes, i)) = requested_heap_size {
            if bytes > MAX_HEAP_FRAME_BYTES
                || bytes < MIN_HEAP_FRAME_BYTES as u32
                || bytes % 1024 != 0
            {
                return Err(TransactionError::InstructionError(
                    i,
                    InstructionError::InvalidInstructionData,
                ));
            }
            self.heap_size = bytes;
        }

        let compute_unit_limit = updated_compute_unit_limit
            .unwrap_or_else(|| {
                num_non_compute_budget_instructions
                    .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
            })
            .min(MAX_COMPUTE_UNIT_LIMIT);
        self.compute_unit_limit = u64::from(compute_unit_limit);

        self.loaded_accounts_data_size_limit = updated_loaded_accounts_data_size_limit
            .unwrap_or(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES)
            .min(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES);

        Ok(prioritization_fee
            .map(|fee_type| PrioritizationFeeDetails::new(fee_type, self.compute_unit_limit))
            .unwrap_or_default())
    }

    pub fn fee_budget_limits<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        feature_set: &FeatureSet,
    ) -> FeeBudgetLimits {
        let mut compute_budget = Self::default();

        let prioritization_fee_details = compute_budget
            .process_instructions(
                instructions,
                !feature_set.is_active(&remove_deprecated_request_unit_ix::id()),
                feature_set.is_active(&add_set_tx_loaded_accounts_data_size_instruction::id()),
            )
            .unwrap_or_default();

        FeeBudgetLimits {
            loaded_accounts_data_size_limit: compute_budget.loaded_accounts_data_size_limit,
            heap_cost: compute_budget.heap_cost,
            compute_unit_limit: compute_budget.compute_unit_limit,
            prioritization_fee: prioritization_fee_details.get_fee(),
        }
    }

    /// Returns cost of the Poseidon hash function for the given number of
    /// inputs is determined by the following quadratic function:
    ///
    /// 61*n^2 + 542
    ///
    /// Which aproximates the results of benchmarks of light-posiedon
    /// library[0]. These results assume 1 CU per 33 ns. Examples:
    ///
    /// * 1 input
    ///   * light-poseidon benchmark: `18,303 / 33 ≈ 555`
    ///   * function: `61*1^2 + 542 = 603`
    /// * 2 inputs
    ///   * light-poseidon benchmark: `25,866 / 33 ≈ 784`
    ///   * function: `61*2^2 + 542 = 786`
    /// * 3 inputs
    ///   * light-poseidon benchmark: `37,549 / 33 ≈ 1,138`
    ///   * function; `61*3^2 + 542 = 1091`
    ///
    /// [0] https://github.com/Lightprotocol/light-poseidon#performance
    pub fn poseidon_cost(&self, nr_inputs: u64) -> Option<u64> {
        let squared_inputs = nr_inputs.checked_pow(2)?;
        let mul_result = self
            .poseidon_cost_coefficient_a
            .checked_mul(squared_inputs)?;
        let final_result = mul_result.checked_add(self.poseidon_cost_coefficient_c)?;

        Some(final_result)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            instruction::Instruction,
            message::Message,
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            system_instruction::{self},
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    macro_rules! test {
        ( $instructions: expr, $expected_result: expr, $expected_budget: expr, $support_set_loaded_accounts_data_size_limit_ix: expr ) => {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            let mut compute_budget = ComputeBudget::default();
            let result = compute_budget.process_instructions(
                tx.message().program_instructions_iter(),
                false, /*not support request_units_deprecated*/
                $support_set_loaded_accounts_data_size_limit_ix,
            );
            assert_eq!($expected_result, result);
            assert_eq!(compute_budget, $expected_budget);
        };
        ( $instructions: expr, $expected_result: expr, $expected_budget: expr) => {
            test!($instructions, $expected_result, $expected_budget, false);
        };
    }

    #[test]
    fn test_process_instructions() {
        // Units
        test!(
            &[],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: 0,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: 1,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT as u64,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT as u64,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: 1,
                ..ComputeBudget::default()
            }
        );

        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::set_compute_unit_price(42)
            ],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::ComputeUnitPrice(42),
                1
            )),
            ComputeBudget {
                compute_unit_limit: 1,
                ..ComputeBudget::default()
            }
        );

        // HeapFrame
        test!(
            &[],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: 0,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                heap_size: 40 * 1024,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(31 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                heap_size: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(1),
            ],
            Err(TransactionError::InstructionError(
                3,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64 * 7,
                ..ComputeBudget::default()
            }
        );

        // Combined
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::ComputeUnitPrice(u64::MAX),
                MAX_COMPUTE_UNIT_LIMIT as u64,
            )),
            ComputeBudget {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT as u64,
                heap_size: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudget::default()
            }
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::ComputeUnitPrice(u64::MAX),
                1
            )),
            ComputeBudget {
                compute_unit_limit: 1,
                heap_size: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudget::default()
            }
        );

        // Duplicates
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT - 1),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES as u32),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_price(0),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );

        // deprecated
        test!(
            &[Instruction::new_with_borsh(
                compute_budget::id(),
                &compute_budget::ComputeBudgetInstruction::RequestUnitsDeprecated {
                    units: 1_000,
                    additional_fee: 10
                },
                vec![]
            )],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
    }

    #[test]
    fn test_process_loaded_accounts_data_size_limit_instruction() {
        // Assert for empty instructions, change value of support_set_loaded_accounts_data_size_limit_ix
        // will not change results, which should all be default
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            test!(
                &[],
                Ok(PrioritizationFeeDetails::default()),
                ComputeBudget {
                    compute_unit_limit: 0,
                    ..ComputeBudget::default()
                },
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents,
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set with data_size
        // else
        //     return InstructionError
        let data_size: usize = 1;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let (expected_result, expected_budget) =
                if support_set_loaded_accounts_data_size_limit_ix {
                    (
                        Ok(PrioritizationFeeDetails::default()),
                        ComputeBudget {
                            compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                            loaded_accounts_data_size_limit: data_size,
                            ..ComputeBudget::default()
                        },
                    )
                } else {
                    (
                        Err(TransactionError::InstructionError(
                            0,
                            InstructionError::InvalidInstructionData,
                        )),
                        ComputeBudget::default(),
                    )
                };

            test!(
                &[
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size as u32),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ],
                expected_result,
                expected_budget,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents, with greater than max value
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set to max data size
        // else
        //     return InstructionError
        let data_size: usize = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES + 1;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let (expected_result, expected_budget) =
                if support_set_loaded_accounts_data_size_limit_ix {
                    (
                        Ok(PrioritizationFeeDetails::default()),
                        ComputeBudget {
                            compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                            loaded_accounts_data_size_limit: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
                            ..ComputeBudget::default()
                        },
                    )
                } else {
                    (
                        Err(TransactionError::InstructionError(
                            0,
                            InstructionError::InvalidInstructionData,
                        )),
                        ComputeBudget::default(),
                    )
                };

            test!(
                &[
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size as u32),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ],
                expected_result,
                expected_budget,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit is not presented
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set to default data size
        // else
        //     return
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let (expected_result, expected_budget) = (
                Ok(PrioritizationFeeDetails::default()),
                ComputeBudget {
                    compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                    loaded_accounts_data_size_limit: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
                    ..ComputeBudget::default()
                },
            );

            test!(
                &[Instruction::new_with_bincode(
                    Pubkey::new_unique(),
                    &0_u8,
                    vec![]
                ),],
                expected_result,
                expected_budget,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents more than once,
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     return DuplicateInstruction
        // else
        //     return InstructionError
        let data_size: usize = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let (expected_result, expected_budget) =
                if support_set_loaded_accounts_data_size_limit_ix {
                    (
                        Err(TransactionError::DuplicateInstruction(2)),
                        ComputeBudget::default(),
                    )
                } else {
                    (
                        Err(TransactionError::InstructionError(
                            1,
                            InstructionError::InvalidInstructionData,
                        )),
                        ComputeBudget::default(),
                    )
                };

            test!(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size as u32),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size as u32),
                ],
                expected_result,
                expected_budget,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }
    }

    #[test]
    fn test_process_mixed_instructions_without_compute_budget() {
        let payer_keypair = Keypair::new();

        let transaction =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                    system_instruction::transfer(&payer_keypair.pubkey(), &Pubkey::new_unique(), 2),
                ],
                Some(&payer_keypair.pubkey()),
                &[&payer_keypair],
                Hash::default(),
            ));

        let mut compute_budget = ComputeBudget::default();
        let result = compute_budget.process_instructions(
            transaction.message().program_instructions_iter(),
            false, //not support request_units_deprecated
            true,  //support_set_loaded_accounts_data_size_limit_ix,
        );

        // assert process_instructions will be successful with default,
        assert_eq!(Ok(PrioritizationFeeDetails::default()), result);
        // assert the default compute_unit_limit is 2 times default: one for bpf ix, one for
        // builtin ix.
        assert_eq!(
            compute_budget,
            ComputeBudget {
                compute_unit_limit: 2 * DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                ..ComputeBudget::default()
            }
        );
    }
}
