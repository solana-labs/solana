use {
    serde::{Deserialize, Serialize},
    solana_measure::measure::Measure,
    solana_program_runtime::{
        instruction_recorder::InstructionRecorder,
        invoke_context::{
            BuiltinProgram, Executors, InstructionAccount, InvokeContext, TransactionAccountRefCell,
        },
        log_collector::LogCollector,
        timings::ExecuteDetailsTimings,
    },
    solana_sdk::{
        account::WritableAccount,
        compute_budget::ComputeBudget,
        feature_set::{prevent_calling_precompiles_as_programs, FeatureSet},
        hash::Hash,
        message::Message,
        precompiles::is_precompile,
        pubkey::Pubkey,
        rent::Rent,
        sysvar::instructions,
        transaction::TransactionError,
    },
    std::{cell::RefCell, rc::Rc, sync::Arc},
};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct MessageProcessor {}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for MessageProcessor {
    fn example() -> Self {
        // MessageProcessor's fields are #[serde(skip)]-ed and not Serialize
        // so, just rely on Default anyway.
        MessageProcessor::default()
    }
}

/// Resultant information gathered from calling process_message()
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProcessedMessageInfo {
    /// The amount that the accounts data len has changed
    pub accounts_data_len_delta: i64,
}

impl MessageProcessor {
    /// Process a message.
    /// This method calls each instruction in the message over the set of loaded accounts.
    /// For each instruction it calls the program entrypoint method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if every instruction succeeds.
    #[allow(clippy::too_many_arguments)]
    pub fn process_message(
        builtin_programs: &[BuiltinProgram],
        message: &Message,
        program_indices: &[Vec<usize>],
        accounts: &[TransactionAccountRefCell],
        rent: Rent,
        log_collector: Option<Rc<RefCell<LogCollector>>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorder: Option<Rc<RefCell<InstructionRecorder>>>,
        feature_set: Arc<FeatureSet>,
        compute_budget: ComputeBudget,
        timings: &mut ExecuteDetailsTimings,
        sysvars: &[(Pubkey, Vec<u8>)],
        blockhash: Hash,
        lamports_per_signature: u64,
    ) -> Result<ProcessedMessageInfo, TransactionError> {
        let mut invoke_context = InvokeContext::new(
            rent,
            accounts,
            builtin_programs,
            sysvars,
            log_collector,
            compute_budget,
            executors,
            instruction_recorder,
            feature_set,
            blockhash,
            lamports_per_signature,
        );

        debug_assert_eq!(program_indices.len(), message.instructions.len());
        for (instruction_index, (instruction, program_indices)) in message
            .instructions
            .iter()
            .zip(program_indices.iter())
            .enumerate()
        {
            let program_id = instruction.program_id(&message.account_keys);
            if invoke_context
                .feature_set
                .is_active(&prevent_calling_precompiles_as_programs::id())
                && is_precompile(program_id, |id| invoke_context.feature_set.is_active(id))
            {
                // Precompiled programs don't have an instruction processor
                continue;
            }

            // Fixup the special instructions key if present
            // before the account pre-values are taken care of
            for (pubkey, account) in accounts.iter().take(message.account_keys.len()) {
                if instructions::check_id(pubkey) {
                    let mut mut_account_ref = account.borrow_mut();
                    instructions::store_current_index(
                        mut_account_ref.data_as_mut_slice(),
                        instruction_index as u16,
                    );
                    break;
                }
            }

            let instruction_accounts = instruction
                .accounts
                .iter()
                .map(|account_index| {
                    let account_index = *account_index as usize;
                    InstructionAccount {
                        index: account_index,
                        is_signer: message.is_signer(account_index),
                        is_writable: message.is_writable(account_index),
                    }
                })
                .collect::<Vec<_>>();
            let mut time = Measure::start("execute_instruction");
            let compute_meter_consumption = invoke_context
                .process_instruction(
                    &instruction.data,
                    &instruction_accounts,
                    None,
                    program_indices,
                )
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
            time.stop();
            timings.accumulate_program(
                instruction.program_id(&message.account_keys),
                time.as_us(),
                compute_meter_consumption,
            );
            timings.accumulate(&invoke_context.timings);
        }
        Ok(ProcessedMessageInfo::default())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::rent_collector::RentCollector,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            instruction::{AccountMeta, Instruction, InstructionError},
            keyed_account::keyed_account_at_index,
            message::Message,
            native_loader::{self, create_loadable_account_for_test},
            secp256k1_instruction::new_secp256k1_instruction,
            secp256k1_program,
        },
    };

    #[derive(Debug, Serialize, Deserialize)]
    enum MockInstruction {
        NoopSuccess,
        NoopFail,
        ModifyOwned,
        ModifyNotOwned,
        ModifyReadonly,
    }

    #[test]
    fn test_process_message_readonly_handling() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            Correct,
            AttemptCredit { lamports: u64 },
            AttemptDataChange { data: u8 },
        }

        fn mock_system_process_instruction(
            first_instruction_account: usize,
            data: &[u8],
            invoke_context: &mut InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::Correct => Ok(()),
                    MockSystemInstruction::AttemptCredit { lamports } => {
                        keyed_account_at_index(keyed_accounts, first_instruction_account)?
                            .account
                            .borrow_mut()
                            .checked_sub_lamports(lamports)?;
                        keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?
                            .account
                            .borrow_mut()
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                    // Change data in a read-only account
                    MockSystemInstruction::AttemptDataChange { data } => {
                        keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?
                            .account
                            .borrow_mut()
                            .set_data(vec![data]);
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let mock_system_program_id = Pubkey::new(&[2u8; 32]);
        let rent_collector = RentCollector::default();
        let builtin_programs = &[BuiltinProgram {
            program_id: mock_system_program_id,
            process_instruction: mock_system_process_instruction,
        }];

        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                RefCell::new(AccountSharedData::new(100, 1, &mock_system_program_id)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                RefCell::new(AccountSharedData::new(0, 1, &mock_system_program_id)),
            ),
            (
                mock_system_program_id,
                RefCell::new(create_loadable_account_for_test("mock_system_program")),
            ),
        ];
        let program_indices = vec![vec![2]];

        let executors = Rc::new(RefCell::new(Executors::default()));

        let account_metas = vec![
            AccountMeta::new(accounts[0].0, true),
            AccountMeta::new_readonly(accounts[1].0, false),
        ];
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::Correct,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );

        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert!(result.is_ok());
        assert_eq!(accounts[0].1.borrow().lamports(), 100);
        assert_eq!(accounts[1].1.borrow().lamports(), 0);

        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::AttemptCredit { lamports: 50 },
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );

        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );

        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::AttemptDataChange { data: 50 },
                account_metas,
            )],
            Some(&accounts[0].0),
        );

        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors,
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyDataModified
            ))
        );
    }

    #[test]
    fn test_process_message_duplicate_accounts() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            BorrowFail,
            MultiBorrowMut,
            DoWork { lamports: u64, data: u8 },
        }

        fn mock_system_process_instruction(
            first_instruction_account: usize,
            data: &[u8],
            invoke_context: &mut InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::BorrowFail => {
                        let from_account =
                            keyed_account_at_index(keyed_accounts, first_instruction_account)?
                                .try_account_ref_mut()?;
                        let dup_account =
                            keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?
                                .try_account_ref_mut()?;
                        if from_account.lamports() != dup_account.lamports() {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::MultiBorrowMut => {
                        let from_lamports = {
                            let from_account =
                                keyed_account_at_index(keyed_accounts, first_instruction_account)?
                                    .try_account_ref_mut()?;
                            from_account.lamports()
                        };
                        let dup_lamports = {
                            let dup_account = keyed_account_at_index(
                                keyed_accounts,
                                first_instruction_account + 2,
                            )?
                            .try_account_ref_mut()?;
                            dup_account.lamports()
                        };
                        if from_lamports != dup_lamports {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::DoWork { lamports, data } => {
                        {
                            let mut to_account = keyed_account_at_index(
                                keyed_accounts,
                                first_instruction_account + 1,
                            )?
                            .try_account_ref_mut()?;
                            let mut dup_account = keyed_account_at_index(
                                keyed_accounts,
                                first_instruction_account + 2,
                            )?
                            .try_account_ref_mut()?;
                            dup_account.checked_sub_lamports(lamports)?;
                            to_account.checked_add_lamports(lamports)?;
                            dup_account.set_data(vec![data]);
                        }
                        keyed_account_at_index(keyed_accounts, first_instruction_account)?
                            .try_account_ref_mut()?
                            .checked_sub_lamports(lamports)?;
                        keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?
                            .try_account_ref_mut()?
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let mock_program_id = Pubkey::new(&[2u8; 32]);
        let rent_collector = RentCollector::default();
        let builtin_programs = &[BuiltinProgram {
            program_id: mock_program_id,
            process_instruction: mock_system_process_instruction,
        }];

        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                RefCell::new(AccountSharedData::new(100, 1, &mock_program_id)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                RefCell::new(AccountSharedData::new(0, 1, &mock_program_id)),
            ),
            (
                mock_program_id,
                RefCell::new(create_loadable_account_for_test("mock_system_program")),
            ),
        ];
        let program_indices = vec![vec![2]];

        let executors = Rc::new(RefCell::new(Executors::default()));

        let account_metas = vec![
            AccountMeta::new(accounts[0].0, true),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new(accounts[0].0, false),
        ];

        // Try to borrow mut the same account
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::BorrowFail,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );
        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::AccountBorrowFailed
            ))
        );

        // Try to borrow mut the same account in a safe way
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::MultiBorrowMut,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );
        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert!(result.is_ok());

        // Do work on the same account but at different location in keyed_accounts[]
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::DoWork {
                    lamports: 10,
                    data: 42,
                },
                account_metas,
            )],
            Some(&accounts[0].0),
        );
        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &program_indices,
            &accounts,
            rent_collector.rent,
            None,
            executors,
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert!(result.is_ok());
        assert_eq!(accounts[0].1.borrow().lamports(), 80);
        assert_eq!(accounts[1].1.borrow().lamports(), 20);
        assert_eq!(accounts[0].1.borrow().data(), &vec![42]);
    }

    #[test]
    fn test_precompile() {
        let mock_program_id = Pubkey::new_unique();
        fn mock_process_instruction(
            _first_instruction_account: usize,
            _data: &[u8],
            _invoke_context: &mut InvokeContext,
        ) -> Result<(), InstructionError> {
            Err(InstructionError::Custom(0xbabb1e))
        }
        let builtin_programs = &[BuiltinProgram {
            program_id: mock_program_id,
            process_instruction: mock_process_instruction,
        }];

        let mut secp256k1_account = AccountSharedData::new(1, 0, &native_loader::id());
        secp256k1_account.set_executable(true);
        let mut mock_program_account = AccountSharedData::new(1, 0, &native_loader::id());
        mock_program_account.set_executable(true);
        let accounts = vec![
            (secp256k1_program::id(), RefCell::new(secp256k1_account)),
            (mock_program_id, RefCell::new(mock_program_account)),
        ];

        let message = Message::new(
            &[
                new_secp256k1_instruction(
                    &libsecp256k1::SecretKey::random(&mut rand::thread_rng()),
                    b"hello",
                ),
                Instruction::new_with_bytes(mock_program_id, &[], vec![]),
            ],
            None,
        );

        let result = MessageProcessor::process_message(
            builtin_programs,
            &message,
            &[vec![0], vec![1]],
            &accounts,
            RentCollector::default().rent,
            None,
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            &mut ExecuteDetailsTimings::default(),
            &[],
            Hash::default(),
            0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                1,
                InstructionError::Custom(0xbabb1e)
            ))
        );
    }
}
