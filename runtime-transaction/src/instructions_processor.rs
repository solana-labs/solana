use {
    crate::compute_budget_instruction_details::*,
    solana_compute_budget::compute_budget_limits::*,
    solana_sdk::{pubkey::Pubkey, transaction::TransactionError},
    solana_svm_transaction::instruction::SVMInstruction,
};

/// Processing compute_budget could be part of tx sanitizing, failed to process
/// these instructions will drop the transaction eventually without execution,
/// may as well fail it early.
/// If succeeded, the transaction's specific limits/requests (could be default)
/// are retrieved and returned,
pub fn process_compute_budget_instructions<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, SVMInstruction<'a>)>,
) -> Result<ComputeBudgetLimits, TransactionError> {
    ComputeBudgetInstructionDetails::try_from(instructions)?
        .sanitize_and_convert_to_compute_budget_limits()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            hash::Hash,
            instruction::{Instruction, InstructionError},
            message::Message,
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            system_instruction::{self},
            transaction::{SanitizedTransaction, Transaction, TransactionError},
        },
        solana_svm_transaction::svm_message::SVMMessage,
        std::num::NonZeroU32,
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
                process_compute_budget_instructions(SVMMessage::program_instructions_iter(&tx));
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
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES),
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
            loaded_accounts_bytes: NonZeroU32::new(data_size).unwrap(),
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
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES.get() + 1;
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
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES.get();
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

        let result = process_compute_budget_instructions(SVMMessage::program_instructions_iter(
            &transaction,
        ));

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
