use {
    crate::{
        compute_budget::DEFAULT_HEAP_COST,
        prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
    },
    solana_sdk::{
        borsh1::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
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

impl From<ComputeBudgetLimits> for FeeBudgetLimits {
    fn from(val: ComputeBudgetLimits) -> Self {
        let prioritization_fee_details = PrioritizationFeeDetails::new(
            PrioritizationFeeType::ComputeUnitPrice(val.compute_unit_price),
            u64::from(val.compute_unit_limit),
        );
        let prioritization_fee = prioritization_fee_details.get_fee();

        FeeBudgetLimits {
            // NOTE - usize::from(u32).unwrap() may fail if target is 16-bit and
            // `loaded_accounts_bytes` is greater than u16::MAX. In that case, panic is proper.
            loaded_accounts_data_size_limit: usize::try_from(val.loaded_accounts_bytes).unwrap(),
            heap_cost: DEFAULT_HEAP_COST,
            compute_unit_limit: u64::from(val.compute_unit_limit),
            prioritization_fee,
        }
    }
}

/// Processing compute_budget could be part of tx sanitizing, failed to process
/// these instructions will drop the transaction eventually without execution,
/// may as well fail it early.
/// If succeeded, the transaction's specific limits/requests (could be default)
/// are retrieved and returned,
pub fn process_compute_budget_instructions<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
) -> Result<ComputeBudgetLimits, TransactionError> {
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
                Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes)) => {
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

    let compute_unit_price = updated_compute_unit_price.unwrap_or(0);

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
        ( $instructions: expr, $expected_result: expr) => {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            let result =
                process_compute_budget_instructions(tx.message().program_instructions_iter());
            assert_eq!($expected_result, result);
        };
    }

    #[test]
    fn test_process_instructions() {
        // Units
        test!(
            &[],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::set_compute_unit_price(42)
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                compute_unit_price: 42,
                ..ComputeBudgetLimits::default()
            })
        );

        // HeapFrame
        test!(
            &[],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: 40 * 1024,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(31 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
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
            ))
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
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT * 7,
                ..ComputeBudgetLimits::default()
            })
        );

        // Combined
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_price: u64::MAX,
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_price: u64::MAX,
                compute_unit_limit: 1,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
        );

        // Duplicates
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT - 1),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES as u32),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_price(0),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );
    }

    #[test]
    fn test_process_loaded_accounts_data_size_limit_instruction() {
        test!(
            &[],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );

        // Assert when set_loaded_accounts_data_size_limit presents,
        // budget is set with data_size
        let data_size = 1;
        let expected_result = Ok(ComputeBudgetLimits {
            compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
            loaded_accounts_bytes: data_size,
            ..ComputeBudgetLimits::default()
        });

        test!(
            &[
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            expected_result
        );

        // Assert when set_loaded_accounts_data_size_limit presents, with greater than max value
        // budget is set to max data size
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES + 1;
        let expected_result = Ok(ComputeBudgetLimits {
            compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
            loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
            ..ComputeBudgetLimits::default()
        });

        test!(
            &[
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            expected_result
        );

        // Assert when set_loaded_accounts_data_size_limit is not presented
        // budget is set to default data size
        let expected_result = Ok(ComputeBudgetLimits {
            compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
            loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
            ..ComputeBudgetLimits::default()
        });

        test!(
            &[Instruction::new_with_bincode(
                Pubkey::new_unique(),
                &0_u8,
                vec![]
            ),],
            expected_result
        );

        // Assert when set_loaded_accounts_data_size_limit presents more than once,
        // return DuplicateInstruction
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES;
        let expected_result = Err(TransactionError::DuplicateInstruction(2));

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
            ],
            expected_result
        );
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

        let result =
            process_compute_budget_instructions(transaction.message().program_instructions_iter());

        // assert process_instructions will be successful with default,
        // and the default compute_unit_limit is 2 times default: one for bpf ix, one for
        // builtin ix.
        assert_eq!(
            result,
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 2 * DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
    }
}
