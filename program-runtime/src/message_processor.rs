use {
    crate::{
        compute_budget::ComputeBudget,
        invoke_context::InvokeContext,
        loaded_programs::LoadedProgramsForTxBatch,
        log_collector::LogCollector,
        sysvar_cache::SysvarCache,
        timings::{ExecuteDetailsTimings, ExecuteTimings},
    },
    serde::{Deserialize, Serialize},
    solana_measure::measure::Measure,
    solana_sdk::{
        account::WritableAccount,
        feature_set::FeatureSet,
        hash::Hash,
        message::SanitizedMessage,
        precompiles::is_precompile,
        rent::Rent,
        saturating_add_assign,
        sysvar::instructions,
        transaction::TransactionError,
        transaction_context::{IndexOfAccount, InstructionAccount, TransactionContext},
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
    /// The change in accounts data len
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
        message: &SanitizedMessage,
        program_indices: &[Vec<IndexOfAccount>],
        transaction_context: &mut TransactionContext,
        rent: Rent,
        log_collector: Option<Rc<RefCell<LogCollector>>>,
        programs_loaded_for_tx_batch: &LoadedProgramsForTxBatch,
        programs_modified_by_tx: &mut LoadedProgramsForTxBatch,
        programs_updated_only_for_global_cache: &mut LoadedProgramsForTxBatch,
        feature_set: Arc<FeatureSet>,
        compute_budget: ComputeBudget,
        timings: &mut ExecuteTimings,
        sysvar_cache: &SysvarCache,
        blockhash: Hash,
        lamports_per_signature: u64,
        current_accounts_data_len: u64,
        accumulated_consumed_units: &mut u64,
    ) -> Result<ProcessedMessageInfo, TransactionError> {
        let mut invoke_context = InvokeContext::new(
            transaction_context,
            rent,
            sysvar_cache,
            log_collector,
            compute_budget,
            programs_loaded_for_tx_batch,
            programs_modified_by_tx,
            programs_updated_only_for_global_cache,
            feature_set,
            blockhash,
            lamports_per_signature,
            current_accounts_data_len,
        );

        debug_assert_eq!(program_indices.len(), message.instructions().len());
        for (instruction_index, ((program_id, instruction), program_indices)) in message
            .program_instructions_iter()
            .zip(program_indices.iter())
            .enumerate()
        {
            let is_precompile =
                is_precompile(program_id, |id| invoke_context.feature_set.is_active(id));

            // Fixup the special instructions key if present
            // before the account pre-values are taken care of
            if let Some(account_index) = invoke_context
                .transaction_context
                .find_index_of_account(&instructions::id())
            {
                let mut mut_account_ref = invoke_context
                    .transaction_context
                    .get_account_at_index(account_index)
                    .map_err(|_| TransactionError::InvalidAccountIndex)?
                    .borrow_mut();
                instructions::store_current_index(
                    mut_account_ref.data_as_mut_slice(),
                    instruction_index as u16,
                );
            }

            let mut instruction_accounts = Vec::with_capacity(instruction.accounts.len());
            for (instruction_account_index, index_in_transaction) in
                instruction.accounts.iter().enumerate()
            {
                let index_in_callee = instruction
                    .accounts
                    .get(0..instruction_account_index)
                    .ok_or(TransactionError::InvalidAccountIndex)?
                    .iter()
                    .position(|account_index| account_index == index_in_transaction)
                    .unwrap_or(instruction_account_index)
                    as IndexOfAccount;
                let index_in_transaction = *index_in_transaction as usize;
                instruction_accounts.push(InstructionAccount {
                    index_in_transaction: index_in_transaction as IndexOfAccount,
                    index_in_caller: index_in_transaction as IndexOfAccount,
                    index_in_callee,
                    is_signer: message.is_signer(index_in_transaction),
                    is_writable: message.is_writable(index_in_transaction),
                });
            }

            let result = if is_precompile {
                invoke_context
                    .transaction_context
                    .get_next_instruction_context()
                    .map(|instruction_context| {
                        instruction_context.configure(
                            program_indices,
                            &instruction_accounts,
                            &instruction.data,
                        );
                    })
                    .and_then(|_| {
                        invoke_context.transaction_context.push()?;
                        invoke_context.transaction_context.pop()
                    })
            } else {
                let mut time = Measure::start("execute_instruction");
                let mut compute_units_consumed = 0;
                let result = invoke_context.process_instruction(
                    &instruction.data,
                    &instruction_accounts,
                    program_indices,
                    &mut compute_units_consumed,
                    timings,
                );
                time.stop();
                *accumulated_consumed_units =
                    accumulated_consumed_units.saturating_add(compute_units_consumed);
                timings.details.accumulate_program(
                    program_id,
                    time.as_us(),
                    compute_units_consumed,
                    result.is_err(),
                );
                invoke_context.timings = {
                    timings.details.accumulate(&invoke_context.timings);
                    ExecuteDetailsTimings::default()
                };
                saturating_add_assign!(
                    timings.execute_accessories.process_instructions.total_us,
                    time.as_us()
                );
                result
            };

            result
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
        }
        Ok(ProcessedMessageInfo {
            accounts_data_len_delta: invoke_context.get_accounts_data_meter().delta(),
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            declare_process_instruction, loaded_programs::LoadedProgram,
            message_processor::MessageProcessor,
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            instruction::{AccountMeta, Instruction, InstructionError},
            message::{AccountKeys, LegacyMessage, Message},
            native_loader::{self, create_loadable_account_for_test},
            pubkey::Pubkey,
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
            TransferLamports { lamports: u64 },
            ChangeData { data: u8 },
        }

        declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            if let Ok(instruction) = bincode::deserialize(instruction_data) {
                match instruction {
                    MockSystemInstruction::Correct => Ok(()),
                    MockSystemInstruction::TransferLamports { lamports } => {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?
                            .checked_sub_lamports(lamports)?;
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 1)?
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                    MockSystemInstruction::ChangeData { data } => {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 1)?
                            .set_data(vec![data])?;
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        });

        let writable_pubkey = Pubkey::new_unique();
        let readonly_pubkey = Pubkey::new_unique();
        let mock_system_program_id = Pubkey::new_unique();

        let accounts = vec![
            (
                writable_pubkey,
                AccountSharedData::new(100, 1, &mock_system_program_id),
            ),
            (
                readonly_pubkey,
                AccountSharedData::new(0, 1, &mock_system_program_id),
            ),
            (
                mock_system_program_id,
                create_loadable_account_for_test("mock_system_program"),
            ),
        ];
        let mut transaction_context =
            TransactionContext::new(accounts, Some(Rent::default()), 1, 3);
        let program_indices = vec![vec![2]];
        let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        programs_loaded_for_tx_batch.replenish(
            mock_system_program_id,
            Arc::new(LoadedProgram::new_builtin(0, 0, MockBuiltin::vm)),
        );
        let account_keys = (0..transaction_context.get_number_of_accounts())
            .map(|index| {
                *transaction_context
                    .get_key_of_account_at_index(index)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let account_metas = vec![
            AccountMeta::new(writable_pubkey, true),
            AccountMeta::new_readonly(readonly_pubkey, false),
        ];

        let message =
            SanitizedMessage::Legacy(LegacyMessage::new(Message::new_with_compiled_instructions(
                1,
                0,
                2,
                account_keys.clone(),
                Hash::default(),
                AccountKeys::new(&account_keys, None).compile_instructions(&[
                    Instruction::new_with_bincode(
                        mock_system_program_id,
                        &MockSystemInstruction::Correct,
                        account_metas.clone(),
                    ),
                ]),
            )));
        let sysvar_cache = SysvarCache::default();
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );
        assert!(result.is_ok());
        assert_eq!(
            transaction_context
                .get_account_at_index(0)
                .unwrap()
                .borrow()
                .lamports(),
            100
        );
        assert_eq!(
            transaction_context
                .get_account_at_index(1)
                .unwrap()
                .borrow()
                .lamports(),
            0
        );

        let message =
            SanitizedMessage::Legacy(LegacyMessage::new(Message::new_with_compiled_instructions(
                1,
                0,
                2,
                account_keys.clone(),
                Hash::default(),
                AccountKeys::new(&account_keys, None).compile_instructions(&[
                    Instruction::new_with_bincode(
                        mock_system_program_id,
                        &MockSystemInstruction::TransferLamports { lamports: 50 },
                        account_metas.clone(),
                    ),
                ]),
            )));
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );

        let message =
            SanitizedMessage::Legacy(LegacyMessage::new(Message::new_with_compiled_instructions(
                1,
                0,
                2,
                account_keys.clone(),
                Hash::default(),
                AccountKeys::new(&account_keys, None).compile_instructions(&[
                    Instruction::new_with_bincode(
                        mock_system_program_id,
                        &MockSystemInstruction::ChangeData { data: 50 },
                        account_metas,
                    ),
                ]),
            )));
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
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

        declare_process_instruction!(MockBuiltin, 1, |invoke_context| {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            let mut to_account =
                instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
            if let Ok(instruction) = bincode::deserialize(instruction_data) {
                match instruction {
                    MockSystemInstruction::BorrowFail => {
                        let from_account = instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?;
                        let dup_account = instruction_context
                            .try_borrow_instruction_account(transaction_context, 2)?;
                        if from_account.get_lamports() != dup_account.get_lamports() {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::MultiBorrowMut => {
                        let lamports_a = instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?
                            .get_lamports();
                        let lamports_b = instruction_context
                            .try_borrow_instruction_account(transaction_context, 2)?
                            .get_lamports();
                        if lamports_a != lamports_b {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::DoWork { lamports, data } => {
                        let mut dup_account = instruction_context
                            .try_borrow_instruction_account(transaction_context, 2)?;
                        dup_account.checked_sub_lamports(lamports)?;
                        to_account.checked_add_lamports(lamports)?;
                        dup_account.set_data(vec![data])?;
                        drop(dup_account);
                        let mut from_account = instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?;
                        from_account.checked_sub_lamports(lamports)?;
                        to_account.checked_add_lamports(lamports)?;
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        });
        let mock_program_id = Pubkey::from([2u8; 32]);
        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new(100, 1, &mock_program_id),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new(0, 1, &mock_program_id),
            ),
            (
                mock_program_id,
                create_loadable_account_for_test("mock_system_program"),
            ),
        ];
        let mut transaction_context =
            TransactionContext::new(accounts, Some(Rent::default()), 1, 3);
        let program_indices = vec![vec![2]];
        let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        programs_loaded_for_tx_batch.replenish(
            mock_program_id,
            Arc::new(LoadedProgram::new_builtin(0, 0, MockBuiltin::vm)),
        );
        let account_metas = vec![
            AccountMeta::new(
                *transaction_context.get_key_of_account_at_index(0).unwrap(),
                true,
            ),
            AccountMeta::new(
                *transaction_context.get_key_of_account_at_index(1).unwrap(),
                false,
            ),
            AccountMeta::new(
                *transaction_context.get_key_of_account_at_index(0).unwrap(),
                false,
            ),
        ];

        // Try to borrow mut the same account
        let message = SanitizedMessage::Legacy(LegacyMessage::new(Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::BorrowFail,
                account_metas.clone(),
            )],
            Some(transaction_context.get_key_of_account_at_index(0).unwrap()),
        )));
        let sysvar_cache = SysvarCache::default();
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::AccountBorrowFailed
            ))
        );

        // Try to borrow mut the same account in a safe way
        let message = SanitizedMessage::Legacy(LegacyMessage::new(Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::MultiBorrowMut,
                account_metas.clone(),
            )],
            Some(transaction_context.get_key_of_account_at_index(0).unwrap()),
        )));
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );
        assert!(result.is_ok());

        // Do work on the same transaction account but at different instruction accounts
        let message = SanitizedMessage::Legacy(LegacyMessage::new(Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::DoWork {
                    lamports: 10,
                    data: 42,
                },
                account_metas,
            )],
            Some(transaction_context.get_key_of_account_at_index(0).unwrap()),
        )));
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &program_indices,
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );
        assert!(result.is_ok());
        assert_eq!(
            transaction_context
                .get_account_at_index(0)
                .unwrap()
                .borrow()
                .lamports(),
            80
        );
        assert_eq!(
            transaction_context
                .get_account_at_index(1)
                .unwrap()
                .borrow()
                .lamports(),
            20
        );
        assert_eq!(
            transaction_context
                .get_account_at_index(0)
                .unwrap()
                .borrow()
                .data(),
            &vec![42]
        );
    }

    #[test]
    fn test_precompile() {
        let mock_program_id = Pubkey::new_unique();
        declare_process_instruction!(MockBuiltin, 1, |_invoke_context| {
            Err(InstructionError::Custom(0xbabb1e))
        });

        let mut secp256k1_account = AccountSharedData::new(1, 0, &native_loader::id());
        secp256k1_account.set_executable(true);
        let mut mock_program_account = AccountSharedData::new(1, 0, &native_loader::id());
        mock_program_account.set_executable(true);
        let accounts = vec![
            (secp256k1_program::id(), secp256k1_account),
            (mock_program_id, mock_program_account),
        ];
        let mut transaction_context =
            TransactionContext::new(accounts, Some(Rent::default()), 1, 2);

        // Since libsecp256k1 is still using the old version of rand, this test
        // copies the `random` implementation at:
        // https://docs.rs/libsecp256k1/latest/src/libsecp256k1/lib.rs.html#430
        let secret_key = {
            use rand::RngCore;
            let mut rng = rand::thread_rng();
            loop {
                let mut ret = [0u8; libsecp256k1::util::SECRET_KEY_SIZE];
                rng.fill_bytes(&mut ret);
                if let Ok(key) = libsecp256k1::SecretKey::parse(&ret) {
                    break key;
                }
            }
        };
        let message = SanitizedMessage::Legacy(LegacyMessage::new(Message::new(
            &[
                new_secp256k1_instruction(&secret_key, b"hello"),
                Instruction::new_with_bytes(mock_program_id, &[], vec![]),
            ],
            None,
        )));
        let sysvar_cache = SysvarCache::default();
        let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        programs_loaded_for_tx_batch.replenish(
            mock_program_id,
            Arc::new(LoadedProgram::new_builtin(0, 0, MockBuiltin::vm)),
        );
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let result = MessageProcessor::process_message(
            &message,
            &[vec![0], vec![1]],
            &mut transaction_context,
            Rent::default(),
            None,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::default(),
            &mut ExecuteTimings::default(),
            &sysvar_cache,
            Hash::default(),
            0,
            0,
            &mut 0,
        );

        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                1,
                InstructionError::Custom(0xbabb1e)
            ))
        );
        assert_eq!(transaction_context.get_instruction_trace_length(), 2);
    }
}
