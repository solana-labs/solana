use {
    crate::prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
    solana_sdk::{
        borsh::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
        instruction::{CompiledInstruction, InstructionError},
        pubkey::Pubkey,
        transaction::TransactionError,
    },
};

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

#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Maximum cross-program invocation depth allowed
    pub max_invoke_depth: usize,
    /// Base number of compute units consumed to call SHA256
    pub sha256_base_cost: u64,
    /// Incremental number of units consumed by SHA256 (based on bytes)
    pub sha256_byte_cost: u64,
    /// Maximum number of slices hashed per syscall
    pub sha256_max_slices: u64,
    /// Maximum BPF to BPF call depth
    pub max_call_depth: usize,
    /// Size of a stack frame in bytes, must match the size specified in the LLVM BPF backend
    pub stack_frame_size: usize,
    /// Number of compute units consumed by logging a `Pubkey`
    pub log_pubkey_units: u64,
    /// Maximum cross-program invocation instruction size
    pub max_cpi_instruction_size: usize,
    /// Number of account data bytes per conpute unit charged during a cross-program invocation
    pub cpi_bytes_per_unit: u64,
    /// Base number of compute units consumed to get a sysvar
    pub sysvar_base_cost: u64,
    /// Number of compute units consumed to call secp256k1_recover
    pub secp256k1_recover_cost: u64,
    /// Number of compute units consumed to do a syscall without any work
    pub syscall_base_cost: u64,
    /// Number of compute units consumed to call zktoken_crypto_op
    pub zk_token_elgamal_op_cost: u64, // to be replaced by curve25519 operations
    /// Number of compute units consumed to validate a curve25519 edwards point
    pub curve25519_edwards_validate_point_cost: u64,
    /// Number of compute units consumed to add two curve25519 edwards points
    pub curve25519_edwards_add_cost: u64,
    /// Number of compute units consumed to subtract two curve25519 edwards points
    pub curve25519_edwards_subtract_cost: u64,
    /// Number of compute units consumed to multiply a curve25519 edwards point
    pub curve25519_edwards_multiply_cost: u64,
    /// Number of compute units consumed to validate a curve25519 ristretto point
    pub curve25519_ristretto_validate_point_cost: u64,
    /// Number of compute units consumed to add two curve25519 ristretto points
    pub curve25519_ristretto_add_cost: u64,
    /// Number of compute units consumed to subtract two curve25519 ristretto points
    pub curve25519_ristretto_subtract_cost: u64,
    /// Number of compute units consumed to multiply a curve25519 ristretto point
    pub curve25519_ristretto_multiply_cost: u64,
    /// Optional program heap region size, if `None` then loader default
    pub heap_size: Option<usize>,
    /// Number of compute units per additional 32k heap above the default (~.5
    /// us per 32k at 15 units/us rounded up)
    pub heap_cost: u64,
    /// Memory operation syscall base cost
    pub mem_op_base_cost: u64,
    /// Number of compute units consumed to call alt_bn128_addition
    pub alt_bn128_addition_cost: u64,
    /// Number of compute units consumed to call alt_bn128_multiplication.
    pub alt_bn128_multiplication_cost: u64,
    /// Number of compute units consumed to pairing one pair of input elements.
    /// Total cost will be one_pair_cost * (num_elems - 1)
    pub alt_bn128_pairing_one_pair_cost: u64,
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
            max_invoke_depth: 4,
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
            zk_token_elgamal_op_cost: 25_000,
            curve25519_edwards_validate_point_cost: 5_000, // TODO: precisely determine curve25519 costs
            curve25519_edwards_add_cost: 5_000,
            curve25519_edwards_subtract_cost: 5_000,
            curve25519_edwards_multiply_cost: 10_000,
            curve25519_ristretto_validate_point_cost: 5_000,
            curve25519_ristretto_add_cost: 5_000,
            curve25519_ristretto_subtract_cost: 5_000,
            curve25519_ristretto_multiply_cost: 10_000,
            heap_size: None,
            heap_cost: 8,
            mem_op_base_cost: 10,
            alt_bn128_addition_cost: 334,
            alt_bn128_multiplication_cost: 11_550,
            alt_bn128_pairing_one_pair_cost: 136_011,
        }
    }

    pub fn process_instructions<'a>(
        &mut self,
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        requestable_heap_size: bool,
        default_units_per_instruction: bool,
        support_set_compute_unit_price_ix: bool,
    ) -> Result<PrioritizationFeeDetails, TransactionError> {
        let mut num_non_compute_budget_instructions: usize = 0;
        let mut updated_compute_unit_limit = None;
        let mut requested_heap_size = None;
        let mut prioritization_fee = None;

        for (i, (program_id, instruction)) in instructions.enumerate() {
            if compute_budget::check_id(program_id) {
                if support_set_compute_unit_price_ix {
                    let invalid_instruction_data_error = TransactionError::InstructionError(
                        i as u8,
                        InstructionError::InvalidInstructionData,
                    );
                    let duplicate_instruction_error =
                        TransactionError::DuplicateInstruction(i as u8);

                    match try_from_slice_unchecked(&instruction.data) {
                        Ok(ComputeBudgetInstruction::RequestUnitsDeprecated {
                            units: compute_unit_limit,
                            additional_fee,
                        }) => {
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
                        _ => return Err(invalid_instruction_data_error),
                    }
                } else if i < 3 {
                    match try_from_slice_unchecked(&instruction.data) {
                        Ok(ComputeBudgetInstruction::RequestUnitsDeprecated {
                            units: compute_unit_limit,
                            additional_fee,
                        }) => {
                            updated_compute_unit_limit = Some(compute_unit_limit);
                            prioritization_fee =
                                Some(PrioritizationFeeType::Deprecated(additional_fee as u64));
                        }
                        Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                            requested_heap_size = Some((bytes, 0));
                        }
                        _ => {
                            return Err(TransactionError::InstructionError(
                                0,
                                InstructionError::InvalidInstructionData,
                            ))
                        }
                    }
                }
            } else {
                // only include non-request instructions in default max calc
                num_non_compute_budget_instructions =
                    num_non_compute_budget_instructions.saturating_add(1);
            }
        }

        if let Some((bytes, i)) = requested_heap_size {
            if !requestable_heap_size
                || bytes > MAX_HEAP_FRAME_BYTES
                || bytes < MIN_HEAP_FRAME_BYTES as u32
                || bytes % 1024 != 0
            {
                return Err(TransactionError::InstructionError(
                    i,
                    InstructionError::InvalidInstructionData,
                ));
            }
            self.heap_size = Some(bytes as usize);
        }

        self.compute_unit_limit = if default_units_per_instruction {
            updated_compute_unit_limit.or_else(|| {
                Some(
                    (num_non_compute_budget_instructions as u32)
                        .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT),
                )
            })
        } else {
            updated_compute_unit_limit
        }
        .unwrap_or(MAX_COMPUTE_UNIT_LIMIT)
        .min(MAX_COMPUTE_UNIT_LIMIT) as u64;

        Ok(prioritization_fee
            .map(|fee_type| PrioritizationFeeDetails::new(fee_type, self.compute_unit_limit))
            .unwrap_or_default())
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
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    fn request_units_deprecated(units: u32, additional_fee: u32) -> Instruction {
        Instruction::new_with_borsh(
            compute_budget::id(),
            &ComputeBudgetInstruction::RequestUnitsDeprecated {
                units,
                additional_fee,
            },
            vec![],
        )
    }

    macro_rules! test {
        ( $instructions: expr, $expected_result: expr, $expected_budget: expr, $type_change: expr  ) => {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            let mut compute_budget = ComputeBudget::default();
            let result = compute_budget.process_instructions(
                tx.message().program_instructions_iter(),
                true,
                true,
                $type_change,
            );
            assert_eq!($expected_result, result);
            assert_eq!(compute_budget, $expected_budget);
        };
        ( $instructions: expr, $expected_result: expr, $expected_budget: expr) => {
            test!($instructions, $expected_result, $expected_budget, true);
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT as u64,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1), // ignored
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64 * 3,
                ..ComputeBudget::default()
            },
            false
        );

        // Prioritization fee
        test!(
            &[request_units_deprecated(1, 42)],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::Deprecated(42),
                1,
            )),
            ComputeBudget {
                compute_unit_limit: 1,
                ..ComputeBudget::default()
            },
            false
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

        test!(
            &[request_units_deprecated(1, u32::MAX)],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::Deprecated(u32::MAX as u64),
                1
            )),
            ComputeBudget {
                compute_unit_limit: 1,
                ..ComputeBudget::default()
            },
            false
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                heap_size: Some(40 * 1024),
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(PrioritizationFeeDetails::default()),
            ComputeBudget {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64,
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            }
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default(),
            false
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
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
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            }
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_units_deprecated(MAX_COMPUTE_UNIT_LIMIT, u32::MAX),
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES as u32),
            ],
            Ok(PrioritizationFeeDetails::new(
                PrioritizationFeeType::Deprecated(u32::MAX as u64),
                MAX_COMPUTE_UNIT_LIMIT as u64,
            )),
            ComputeBudget {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT as u64,
                heap_size: Some(MIN_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            },
            false
        );

        // Duplicates
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT - 1),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES as u32),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES as u32),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::set_compute_unit_price(0),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Err(TransactionError::DuplicateInstruction(2)),
            ComputeBudget::default()
        );
    }
}
