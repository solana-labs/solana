use {
    solana_compute_budget::compute_budget_limits::*,
    solana_sdk::{
        borsh1::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        instruction::InstructionError,
        pubkey::Pubkey,
        saturating_add_assign,
        transaction::{Result, TransactionError},
    },
    solana_svm_transaction::instruction::SVMInstruction,
    std::num::NonZeroU32,
};

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Default, Debug)]
pub(crate) struct ComputeBudgetInstructionDetails {
    // compute-budget instruction details:
    // the first field in tuple is instruction index, second field is the unsanitized value set by user
    requested_compute_unit_limit: Option<(u8, u32)>,
    requested_compute_unit_price: Option<(u8, u64)>,
    requested_heap_size: Option<(u8, u32)>,
    requested_loaded_accounts_data_size_limit: Option<(u8, u32)>,
    num_non_compute_budget_instructions: u32,
}

impl ComputeBudgetInstructionDetails {
    pub fn try_from<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, SVMInstruction<'a>)>,
    ) -> Result<Self> {
        let mut compute_budget_instruction_details = ComputeBudgetInstructionDetails::default();
        for (i, (program_id, instruction)) in instructions.enumerate() {
            compute_budget_instruction_details.process_instruction(
                i as u8,
                program_id,
                &instruction,
            )?;
        }

        Ok(compute_budget_instruction_details)
    }

    pub fn sanitize_and_convert_to_compute_budget_limits(&self) -> Result<ComputeBudgetLimits> {
        // Sanitize requested heap size
        let updated_heap_bytes =
            if let Some((index, requested_heap_size)) = self.requested_heap_size {
                if Self::sanitize_requested_heap_size(requested_heap_size) {
                    requested_heap_size
                } else {
                    return Err(TransactionError::InstructionError(
                        index,
                        InstructionError::InvalidInstructionData,
                    ));
                }
            } else {
                MIN_HEAP_FRAME_BYTES
            }
            .min(MAX_HEAP_FRAME_BYTES);

        // Calculate compute unit limit
        let compute_unit_limit = self
            .requested_compute_unit_limit
            .map_or_else(
                || {
                    self.num_non_compute_budget_instructions
                        .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
                },
                |(_index, requested_compute_unit_limit)| requested_compute_unit_limit,
            )
            .min(MAX_COMPUTE_UNIT_LIMIT);

        let compute_unit_price = self
            .requested_compute_unit_price
            .map_or(0, |(_index, requested_compute_unit_price)| {
                requested_compute_unit_price
            });

        let loaded_accounts_bytes =
            if let Some((_index, requested_loaded_accounts_data_size_limit)) =
                self.requested_loaded_accounts_data_size_limit
            {
                NonZeroU32::new(requested_loaded_accounts_data_size_limit)
                    .ok_or(TransactionError::InvalidLoadedAccountsDataSizeLimit)?
            } else {
                MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES
            }
            .min(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES);

        Ok(ComputeBudgetLimits {
            updated_heap_bytes,
            compute_unit_limit,
            compute_unit_price,
            loaded_accounts_bytes,
        })
    }

    fn process_instruction(
        &mut self,
        index: u8,
        program_id: &Pubkey,
        instruction: &SVMInstruction,
    ) -> Result<()> {
        if compute_budget::check_id(program_id) {
            let invalid_instruction_data_error =
                TransactionError::InstructionError(index, InstructionError::InvalidInstructionData);
            let duplicate_instruction_error = TransactionError::DuplicateInstruction(index);

            match try_from_slice_unchecked(instruction.data) {
                Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                    if self.requested_heap_size.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    self.requested_heap_size = Some((index, bytes));
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitLimit(compute_unit_limit)) => {
                    if self.requested_compute_unit_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    self.requested_compute_unit_limit = Some((index, compute_unit_limit));
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                    if self.requested_compute_unit_price.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    self.requested_compute_unit_price = Some((index, micro_lamports));
                }
                Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes)) => {
                    if self.requested_loaded_accounts_data_size_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    self.requested_loaded_accounts_data_size_limit = Some((index, bytes));
                }
                _ => return Err(invalid_instruction_data_error),
            }
        } else {
            saturating_add_assign!(self.num_non_compute_budget_instructions, 1);
        }

        Ok(())
    }

    fn sanitize_requested_heap_size(bytes: u32) -> bool {
        (MIN_HEAP_FRAME_BYTES..=MAX_HEAP_FRAME_BYTES).contains(&bytes) && bytes % 1024 == 0
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::instruction::{CompiledInstruction, Instruction},
    };

    fn setup_test_instruction(
        index: u8,
        instruction: Instruction,
    ) -> (Pubkey, CompiledInstruction) {
        (
            instruction.program_id,
            CompiledInstruction {
                program_id_index: index,
                data: instruction.data.clone(),
                accounts: vec![],
            },
        )
    }

    #[test]
    fn test_process_instruction_request_heap() {
        let mut index = 0;
        let mut expected_details = ComputeBudgetInstructionDetails::default();
        let mut compute_budget_instruction_details = ComputeBudgetInstructionDetails::default();

        // irrelevant instruction makes no change
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions = 1;
        assert_eq!(compute_budget_instruction_details, expected_details);

        // valid instruction
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::request_heap_frame(40 * 1024),
        );
        expected_details.requested_heap_size = Some((index, 40 * 1024));
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        assert_eq!(compute_budget_instruction_details, expected_details);

        // duplicate instruction results error
        index += 1;
        let expected_err = Err(TransactionError::DuplicateInstruction(index));
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::request_heap_frame(50 * 1024),
        );
        assert_eq!(
            compute_budget_instruction_details.process_instruction(
                index,
                &program_id,
                &SVMInstruction::from(&ix)
            ),
            expected_err
        );
        assert_eq!(compute_budget_instruction_details, expected_details);

        // irrelevant instruction makes no change
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions += 1;
        assert_eq!(compute_budget_instruction_details, expected_details);
    }

    #[test]
    fn test_process_instruction_compute_unit_limit() {
        let mut index = 0;
        let mut expected_details = ComputeBudgetInstructionDetails::default();
        let mut compute_budget_instruction_details = ComputeBudgetInstructionDetails::default();

        // irrelevant instruction makes no change
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions = 1;
        assert_eq!(compute_budget_instruction_details, expected_details);

        // valid instruction,
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::set_compute_unit_limit(u32::MAX),
        );
        expected_details.requested_compute_unit_limit = Some((index, u32::MAX));
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        assert_eq!(compute_budget_instruction_details, expected_details);

        // duplicate instruction results error
        index += 1;
        let expected_err = Err(TransactionError::DuplicateInstruction(index));
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
        );
        assert_eq!(
            compute_budget_instruction_details.process_instruction(
                index,
                &program_id,
                &SVMInstruction::from(&ix)
            ),
            expected_err
        );
        assert_eq!(compute_budget_instruction_details, expected_details);

        // irrelevant instruction makes no change
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions += 1;
        assert_eq!(compute_budget_instruction_details, expected_details);
    }

    #[test]
    fn test_process_instruction_compute_unit_price() {
        let mut index = 0;
        let mut expected_details = ComputeBudgetInstructionDetails::default();
        let mut compute_budget_instruction_details = ComputeBudgetInstructionDetails::default();

        // irrelevant instruction makes no change
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions = 1;
        assert_eq!(compute_budget_instruction_details, expected_details);

        // valid instruction,
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
        );
        expected_details.requested_compute_unit_price = Some((index, u64::MAX));
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        assert_eq!(compute_budget_instruction_details, expected_details);

        // duplicate instruction results error
        index += 1;
        let expected_err = Err(TransactionError::DuplicateInstruction(index));
        let (program_id, ix) =
            setup_test_instruction(index, ComputeBudgetInstruction::set_compute_unit_price(0));
        assert_eq!(
            compute_budget_instruction_details.process_instruction(
                index,
                &program_id,
                &SVMInstruction::from(&ix)
            ),
            expected_err
        );
        assert_eq!(compute_budget_instruction_details, expected_details);

        // irrelevant instruction makes no change
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions += 1;
        assert_eq!(compute_budget_instruction_details, expected_details);
    }

    #[test]
    fn test_process_instruction_loaded_accounts_data_size_limit() {
        let mut index = 0;
        let mut expected_details = ComputeBudgetInstructionDetails::default();
        let mut compute_budget_instruction_details = ComputeBudgetInstructionDetails::default();

        // irrelevant instruction makes no change
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions = 1;
        assert_eq!(compute_budget_instruction_details, expected_details);

        // valid instruction,
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(u32::MAX),
        );
        expected_details.requested_loaded_accounts_data_size_limit = Some((index, u32::MAX));
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        assert_eq!(compute_budget_instruction_details, expected_details);

        // duplicate instruction results error
        index += 1;
        let expected_err = Err(TransactionError::DuplicateInstruction(index));
        let (program_id, ix) = setup_test_instruction(
            index,
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(0),
        );
        assert_eq!(
            compute_budget_instruction_details.process_instruction(
                index,
                &program_id,
                &SVMInstruction::from(&ix)
            ),
            expected_err
        );
        assert_eq!(compute_budget_instruction_details, expected_details);

        // irrelevant instruction makes no change
        index += 1;
        let (program_id, ix) = setup_test_instruction(
            index,
            Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
        );
        assert!(compute_budget_instruction_details
            .process_instruction(index, &program_id, &SVMInstruction::from(&ix))
            .is_ok());
        expected_details.num_non_compute_budget_instructions += 1;
        assert_eq!(compute_budget_instruction_details, expected_details);
    }

    #[test]
    fn test_sanitize_and_convert_to_compute_budget_limits() {
        // empty details, default ComputeBudgetLimits with 0 compute_unit_limits
        let instruction_details = ComputeBudgetInstructionDetails::default();
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );

        let num_non_compute_budget_instructions = 4;

        // no compute-budget instructions, all default ComputeBudgetLimits except cu-limit
        let instruction_details = ComputeBudgetInstructionDetails {
            num_non_compute_budget_instructions,
            ..ComputeBudgetInstructionDetails::default()
        };
        let expected_compute_unit_limit =
            num_non_compute_budget_instructions * DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT;
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            Ok(ComputeBudgetLimits {
                compute_unit_limit: expected_compute_unit_limit,
                ..ComputeBudgetLimits::default()
            })
        );

        let expected_heap_size_err = Err(TransactionError::InstructionError(
            3,
            InstructionError::InvalidInstructionData,
        ));
        // invalid: requested_heap_size can't be zero
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, 0)),
            requested_compute_unit_price: Some((2, 0)),
            requested_heap_size: Some((3, 0)),
            requested_loaded_accounts_data_size_limit: Some((4, 1024)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            expected_heap_size_err
        );

        // invalid: requested_heap_size can't be less than MIN_HEAP_FRAME_BYTES
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, 0)),
            requested_compute_unit_price: Some((2, 0)),
            requested_heap_size: Some((3, MIN_HEAP_FRAME_BYTES - 1)),
            requested_loaded_accounts_data_size_limit: Some((4, 1024)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            expected_heap_size_err
        );

        // invalid: requested_heap_size can't be more than MAX_HEAP_FRAME_BYTES
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, 0)),
            requested_compute_unit_price: Some((2, 0)),
            requested_heap_size: Some((3, MAX_HEAP_FRAME_BYTES + 1)),
            requested_loaded_accounts_data_size_limit: Some((4, 1024)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            expected_heap_size_err
        );

        // invalid: requested_heap_size must be round by 1024
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, 0)),
            requested_compute_unit_price: Some((2, 0)),
            requested_heap_size: Some((3, MIN_HEAP_FRAME_BYTES + 1024 + 1)),
            requested_loaded_accounts_data_size_limit: Some((4, 1024)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            expected_heap_size_err
        );

        // invalid: loaded_account_data_size can't be zero
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, 0)),
            requested_compute_unit_price: Some((2, 0)),
            requested_heap_size: Some((3, 40 * 1024)),
            requested_loaded_accounts_data_size_limit: Some((4, 0)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            Err(TransactionError::InvalidLoadedAccountsDataSizeLimit)
        );

        // valid: acceptable MAX
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, u32::MAX)),
            requested_compute_unit_price: Some((2, u64::MAX)),
            requested_heap_size: Some((3, MAX_HEAP_FRAME_BYTES)),
            requested_loaded_accounts_data_size_limit: Some((4, u32::MAX)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            Ok(ComputeBudgetLimits {
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                compute_unit_price: u64::MAX,
                loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
            })
        );

        // valid
        let val: u32 = 1024 * 40;
        let instruction_details = ComputeBudgetInstructionDetails {
            requested_compute_unit_limit: Some((1, val)),
            requested_compute_unit_price: Some((2, val as u64)),
            requested_heap_size: Some((3, val)),
            requested_loaded_accounts_data_size_limit: Some((4, val)),
            num_non_compute_budget_instructions,
        };
        assert_eq!(
            instruction_details.sanitize_and_convert_to_compute_budget_limits(),
            Ok(ComputeBudgetLimits {
                updated_heap_bytes: val,
                compute_unit_limit: val,
                compute_unit_price: val as u64,
                loaded_accounts_bytes: NonZeroU32::new(val).unwrap(),
            })
        );
    }
}
