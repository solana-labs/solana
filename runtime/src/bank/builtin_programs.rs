#[cfg(test)]
mod tests {
    use {
        crate::bank::*,
        solana_sdk::{
            ed25519_program, feature_set::FeatureSet, genesis_config::create_genesis_config,
        },
    };

    #[test]
    fn test_apply_builtin_program_feature_transitions_for_new_epoch() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.finish_init(&genesis_config, None, false);

        // Overwrite precompile accounts to simulate a cluster which already added precompiles.
        for precompile in get_precompiles() {
            bank.store_account(&precompile.program_id, &AccountSharedData::default());
            // Simulate cluster which added ed25519 precompile with a system program owner
            if precompile.program_id == ed25519_program::id() {
                bank.add_precompiled_account_with_owner(
                    &precompile.program_id,
                    solana_sdk::system_program::id(),
                );
            } else {
                bank.add_precompiled_account(&precompile.program_id);
            }
        }

        // Normally feature transitions are applied to a bank that hasn't been
        // frozen yet.  Freeze the bank early to ensure that no account changes
        // are made.
        bank.freeze();

        // Simulate crossing an epoch boundary for a new bank
        let only_apply_transitions_for_new_features = true;
        bank.apply_builtin_program_feature_transitions(
            only_apply_transitions_for_new_features,
            &HashSet::new(),
        );
    }

    #[test]
    fn test_startup_from_snapshot_after_precompile_transition() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.finish_init(&genesis_config, None, false);

        // Overwrite precompile accounts to simulate a cluster which already added precompiles.
        for precompile in get_precompiles() {
            bank.store_account(&precompile.program_id, &AccountSharedData::default());
            bank.add_precompiled_account(&precompile.program_id);
        }

        bank.freeze();

        // Simulate starting up from snapshot finishing the initialization for a frozen bank
        bank.finish_init(&genesis_config, None, false);
    }
}

#[cfg(test)]
mod tests_core_bpf_migration {
    use {
        crate::bank::{
            builtins::{
                core_bpf_migration::{tests::TestContext, CoreBpfMigrationConfig},
                BuiltinPrototype, StatelessBuiltinPrototype, BUILTINS, STATELESS_BUILTINS,
            },
            test_utils::goto_end_of_slot,
            tests::{create_genesis_config, new_bank_from_parent_with_bank_forks},
            Bank,
        },
        solana_program_runtime::loaded_programs::ProgramCacheEntry,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            bpf_loader_upgradeable::{self, get_program_data_address, UpgradeableLoaderState},
            epoch_schedule::EpochSchedule,
            feature::{self, Feature},
            feature_set::FeatureSet,
            instruction::{AccountMeta, Instruction},
            message::Message,
            native_loader,
            native_token::LAMPORTS_PER_SOL,
            pubkey::Pubkey,
            signature::Signer,
            transaction::Transaction,
        },
        std::{fs::File, io::Read, sync::Arc},
        test_case::test_case,
    };

    // CPI mockup to test CPI to newly migrated programs.
    mod cpi_mockup {
        use {
            solana_program_runtime::declare_process_instruction,
            solana_sdk::instruction::Instruction,
        };

        declare_process_instruction!(Entrypoint, 0, |invoke_context| {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;

            let target_program_id = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(0)?,
            )?;

            let instruction = Instruction::new_with_bytes(*target_program_id, &[], Vec::new());

            invoke_context.native_invoke(instruction.into(), &[])
        });
    }

    fn test_elf() -> Vec<u8> {
        let mut elf = Vec::new();
        File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so")
            .unwrap()
            .read_to_end(&mut elf)
            .unwrap();
        elf
    }

    enum TestPrototype<'a> {
        Builtin(&'a BuiltinPrototype),
        Stateless(&'a StatelessBuiltinPrototype),
    }
    impl<'a> TestPrototype<'a> {
        fn deconstruct(&'a self) -> (&'a Pubkey, &'a CoreBpfMigrationConfig) {
            match self {
                Self::Builtin(prototype) => (
                    &prototype.program_id,
                    prototype.core_bpf_migration_config.as_ref().unwrap(),
                ),
                Self::Stateless(prototype) => (
                    &prototype.program_id,
                    prototype.core_bpf_migration_config.as_ref().unwrap(),
                ),
            }
        }
    }

    // This test can't be used to the `compute_budget` program, unless a valid
    // `compute_budget` program is provided as the replacement (source).
    // See program_runtime::compute_budget_processor::process_compute_budget_instructions`.`
    // It also can't test the `bpf_loader_upgradeable` program, as it's used in
    // the SVM's loader to invoke programs.
    // See `solana_svm::account_loader::load_transaction_accounts`.
    #[test_case(TestPrototype::Builtin(&BUILTINS[0]); "system")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[1]); "vote")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[2]); "stake")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[3]); "config")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[4]); "bpf_loader_deprecated")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[5]); "bpf_loader")]
    #[test_case(TestPrototype::Builtin(&BUILTINS[8]); "address_lookup_table")]
    #[test_case(TestPrototype::Stateless(&STATELESS_BUILTINS[0]); "feature_gate")]
    fn test_core_bpf_migration(prototype: TestPrototype) {
        let (mut genesis_config, mint_keypair) =
            create_genesis_config(1_000_000 * LAMPORTS_PER_SOL);
        let slots_per_epoch = 32;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(slots_per_epoch, slots_per_epoch, false);

        let mut root_bank = Bank::new_for_tests(&genesis_config);

        // Set up the CPI mockup to test CPI'ing to the migrated program.
        let cpi_program_id = Pubkey::new_unique();
        let cpi_program_name = "mock_cpi_program";
        root_bank.transaction_processor.add_builtin(
            &root_bank,
            cpi_program_id,
            cpi_program_name,
            ProgramCacheEntry::new_builtin(0, cpi_program_name.len(), cpi_mockup::Entrypoint::vm),
        );

        let (builtin_id, config) = prototype.deconstruct();
        let feature_id = &config.feature_id;
        let source_buffer_address = &config.source_buffer_address;
        let upgrade_authority_address = config.upgrade_authority_address;

        // Add the feature to the bank's inactive feature set.
        // Note this will add the feature ID if it doesn't exist.
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(feature_id);
        root_bank.feature_set = Arc::new(feature_set);

        // Initialize the source buffer account.
        let test_context = TestContext::new(
            &root_bank,
            builtin_id,
            source_buffer_address,
            upgrade_authority_address,
        );

        let (bank, bank_forks) = root_bank.wrap_with_bank_forks_for_tests();

        // Advance to the next epoch without activating the feature.
        let mut first_slot_in_next_epoch = slots_per_epoch + 1;
        let bank = new_bank_from_parent_with_bank_forks(
            &bank_forks,
            bank,
            &Pubkey::default(),
            first_slot_in_next_epoch,
        );

        // Assert the feature was not activated and the program was not
        // migrated.
        assert!(!bank.feature_set.is_active(feature_id));
        assert!(bank.get_account(source_buffer_address).is_some());

        // Store the account to activate the feature.
        bank.store_account_and_update_capitalization(
            feature_id,
            &feature::create_account(&Feature::default(), 42),
        );

        // Advance the bank to cross the epoch boundary and activate the
        // feature.
        goto_end_of_slot(bank.clone());
        first_slot_in_next_epoch += slots_per_epoch;
        let migration_slot = first_slot_in_next_epoch;
        let bank = new_bank_from_parent_with_bank_forks(
            &bank_forks,
            bank,
            &Pubkey::default(),
            first_slot_in_next_epoch,
        );

        // Run the post-migration program checks.
        assert!(bank.feature_set.is_active(feature_id));
        test_context.run_program_checks(&bank, migration_slot);

        // Advance one slot so that the new BPF builtin program becomes
        // effective in the program cache.
        goto_end_of_slot(bank.clone());
        let next_slot = bank.slot() + 1;
        let bank =
            new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), next_slot);

        // Successfully invoke the new BPF builtin program.
        bank.process_transaction(&Transaction::new(
            &vec![&mint_keypair],
            Message::new(
                &[Instruction::new_with_bytes(*builtin_id, &[], Vec::new())],
                Some(&mint_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        ))
        .unwrap();

        // Successfully invoke the new BPF builtin program via CPI.
        bank.process_transaction(&Transaction::new(
            &vec![&mint_keypair],
            Message::new(
                &[Instruction::new_with_bytes(
                    cpi_program_id,
                    &[],
                    vec![AccountMeta::new_readonly(*builtin_id, false)],
                )],
                Some(&mint_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        ))
        .unwrap();

        // Simulate crossing another epoch boundary for a new bank.
        goto_end_of_slot(bank.clone());
        first_slot_in_next_epoch += slots_per_epoch;
        let bank = new_bank_from_parent_with_bank_forks(
            &bank_forks,
            bank,
            &Pubkey::default(),
            first_slot_in_next_epoch,
        );

        // Run the post-migration program checks again.
        assert!(bank.feature_set.is_active(feature_id));
        test_context.run_program_checks(&bank, migration_slot);

        // Again, successfully invoke the new BPF builtin program.
        bank.process_transaction(&Transaction::new(
            &vec![&mint_keypair],
            Message::new(
                &[Instruction::new_with_bytes(*builtin_id, &[], Vec::new())],
                Some(&mint_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        ))
        .unwrap();

        // Again, successfully invoke the new BPF builtin program via CPI.
        bank.process_transaction(&Transaction::new(
            &vec![&mint_keypair],
            Message::new(
                &[Instruction::new_with_bytes(
                    cpi_program_id,
                    &[],
                    vec![AccountMeta::new_readonly(*builtin_id, false)],
                )],
                Some(&mint_keypair.pubkey()),
            ),
            bank.last_blockhash(),
        ))
        .unwrap();
    }

    // Simulate a failure to migrate the program.
    // Here we want to see that the bank handles the failure gracefully and
    // advances to the next epoch without issue.
    #[test]
    fn test_core_bpf_migration_failure() {
        let (genesis_config, _mint_keypair) = create_genesis_config(0);
        let mut root_bank = Bank::new_for_tests(&genesis_config);

        let test_prototype = TestPrototype::Builtin(&BUILTINS[0]); // System program
        let (builtin_id, config) = test_prototype.deconstruct();
        let feature_id = &config.feature_id;
        let source_buffer_address = &config.source_buffer_address;
        let upgrade_authority_address = Some(Pubkey::new_unique());

        // Add the feature to the bank's inactive feature set.
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.inactive.insert(*feature_id);
        root_bank.feature_set = Arc::new(feature_set);

        // Initialize the source buffer account.
        let _test_context = TestContext::new(
            &root_bank,
            builtin_id,
            source_buffer_address,
            upgrade_authority_address,
        );

        let (bank, bank_forks) = root_bank.wrap_with_bank_forks_for_tests();

        // Intentionally nuke the source buffer account to force the migration
        // to fail.
        bank.store_account_and_update_capitalization(
            source_buffer_address,
            &AccountSharedData::default(),
        );

        // Activate the feature.
        bank.store_account_and_update_capitalization(
            feature_id,
            &feature::create_account(&Feature::default(), 42),
        );

        // Advance the bank to cross the epoch boundary and activate the
        // feature.
        goto_end_of_slot(bank.clone());
        let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 33);

        // Assert the feature _was_ activated but the program was not migrated.
        assert!(bank.feature_set.is_active(feature_id));
        assert!(bank
            .transaction_processor
            .builtin_program_ids
            .read()
            .unwrap()
            .contains(builtin_id));
        assert_eq!(
            bank.get_account(builtin_id).unwrap().owner(),
            &native_loader::id()
        );

        // Simulate crossing an epoch boundary again.
        goto_end_of_slot(bank.clone());
        let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 96);

        // Again, assert the feature is still active and the program still was
        // not migrated.
        assert!(bank.feature_set.is_active(feature_id));
        assert!(bank
            .transaction_processor
            .builtin_program_ids
            .read()
            .unwrap()
            .contains(builtin_id));
        assert_eq!(
            bank.get_account(builtin_id).unwrap().owner(),
            &native_loader::id()
        );
    }

    // Simulate creating a bank from a snapshot after a migration feature was
    // activated, but the migration failed.
    // Here we want to see that the bank recognizes the failed migration and
    // adds the original builtin to the new bank.
    #[test]
    fn test_core_bpf_migration_init_after_failed_migration() {
        let (genesis_config, _mint_keypair) = create_genesis_config(0);

        let test_prototype = TestPrototype::Builtin(&BUILTINS[0]); // System program
        let (builtin_id, config) = test_prototype.deconstruct();
        let feature_id = &config.feature_id;

        // Since the test feature IDs aren't included in the SDK, the only way
        // to simulate loading from snapshot with this feature active is to
        // create a bank, overwrite the feature set with the feature active,
        // then re-run the `finish_init` method.
        let mut bank = Bank::new_for_tests(&genesis_config);

        // Set up the feature set with the migration feature marked as active.
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.active.insert(*feature_id, 0);
        bank.feature_set = Arc::new(feature_set);
        bank.store_account_and_update_capitalization(
            feature_id,
            &feature::create_account(
                &Feature {
                    activated_at: Some(0),
                },
                42,
            ),
        );

        // Run `finish_init` to simulate starting up from a snapshot.
        // Clear all builtins to simulate a fresh bank init.
        bank.transaction_processor
            .program_cache
            .write()
            .unwrap()
            .remove_programs(
                bank.transaction_processor
                    .builtin_program_ids
                    .read()
                    .unwrap()
                    .clone()
                    .into_iter(),
            );
        bank.transaction_processor
            .builtin_program_ids
            .write()
            .unwrap()
            .clear();
        bank.finish_init(&genesis_config, None, false);

        // Assert the feature is active and the bank still added the builtin.
        assert!(bank.feature_set.is_active(feature_id));
        assert!(bank
            .transaction_processor
            .builtin_program_ids
            .read()
            .unwrap()
            .contains(builtin_id));
        assert_eq!(
            bank.get_account(builtin_id).unwrap().owner(),
            &native_loader::id()
        );

        // Simulate crossing an epoch boundary for a new bank.
        let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
        goto_end_of_slot(bank.clone());
        let bank = new_bank_from_parent_with_bank_forks(&bank_forks, bank, &Pubkey::default(), 33);

        // Assert the feature is active but the builtin was not migrated.
        assert!(bank.feature_set.is_active(feature_id));
        assert!(bank
            .transaction_processor
            .builtin_program_ids
            .read()
            .unwrap()
            .contains(builtin_id));
        assert_eq!(
            bank.get_account(builtin_id).unwrap().owner(),
            &native_loader::id()
        );
    }

    // Simulate creating a bank from a snapshot after a migration feature was
    // activated and the migration was successful.
    // Here we want to see that the bank recognizes the migration and
    // _does not_ add the original builtin to the new bank.
    #[test]
    fn test_core_bpf_migration_init_after_successful_migration() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(0);

        let test_prototype = TestPrototype::Builtin(&BUILTINS[0]); // System program
        let (builtin_id, config) = test_prototype.deconstruct();
        let feature_id = &config.feature_id;

        let upgrade_authority_address = Some(Pubkey::new_unique());
        let elf = test_elf();
        let program_data_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
        let program_data_size = program_data_metadata_size + elf.len();

        // Set up a post-migration builtin.
        let builtin_program_data_address = get_program_data_address(builtin_id);
        let builtin_program_account = AccountSharedData::new_data(
            100_000,
            &UpgradeableLoaderState::Program {
                programdata_address: builtin_program_data_address,
            },
            &bpf_loader_upgradeable::id(),
        )
        .unwrap();
        let mut builtin_program_data_account = AccountSharedData::new_data_with_space(
            100_000,
            &UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address,
            },
            program_data_size,
            &bpf_loader_upgradeable::id(),
        )
        .unwrap();
        builtin_program_data_account.data_as_mut_slice()[program_data_metadata_size..]
            .copy_from_slice(&elf);
        genesis_config
            .accounts
            .insert(*builtin_id, builtin_program_account.into());
        genesis_config.accounts.insert(
            builtin_program_data_address,
            builtin_program_data_account.into(),
        );

        // Use this closure to run checks on the builtin.
        let check_builtin_is_bpf = |bank: &Bank| {
            // The bank's transaction processor should not contain the builtin
            // in its list of builtin program IDs.
            assert!(!bank
                .transaction_processor
                .builtin_program_ids
                .read()
                .unwrap()
                .contains(builtin_id));
            // The builtin should be owned by the upgradeable loader and have
            // the correct state.
            let fetched_builtin_program_account = bank.get_account(builtin_id).unwrap();
            assert_eq!(
                fetched_builtin_program_account.owner(),
                &bpf_loader_upgradeable::id()
            );
            assert_eq!(
                bincode::deserialize::<UpgradeableLoaderState>(
                    fetched_builtin_program_account.data()
                )
                .unwrap(),
                UpgradeableLoaderState::Program {
                    programdata_address: builtin_program_data_address
                }
            );
            // The builtin's program data should be owned by the upgradeable
            // loader and have the correct state.
            let fetched_builtin_program_data_account =
                bank.get_account(&builtin_program_data_address).unwrap();
            assert_eq!(
                fetched_builtin_program_data_account.owner(),
                &bpf_loader_upgradeable::id()
            );
            assert_eq!(
                bincode::deserialize::<UpgradeableLoaderState>(
                    &fetched_builtin_program_data_account.data()[..program_data_metadata_size]
                )
                .unwrap(),
                UpgradeableLoaderState::ProgramData {
                    slot: 0,
                    upgrade_authority_address
                }
            );
            assert_eq!(
                &fetched_builtin_program_data_account.data()[program_data_metadata_size..],
                elf,
            );
        };

        // Create a new bank.
        let mut bank = Bank::new_for_tests(&genesis_config);
        check_builtin_is_bpf(&bank);

        // Now, add the feature ID as active, and run `finish_init` again to
        // make sure the feature is idempotent.
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.active.insert(*feature_id, 0);
        bank.feature_set = Arc::new(feature_set);
        bank.store_account_and_update_capitalization(
            feature_id,
            &feature::create_account(
                &Feature {
                    activated_at: Some(0),
                },
                42,
            ),
        );

        // Run `finish_init` to simulate starting up from a snapshot.
        // Clear all builtins to simulate a fresh bank init.
        bank.transaction_processor
            .program_cache
            .write()
            .unwrap()
            .remove_programs(
                bank.transaction_processor
                    .builtin_program_ids
                    .read()
                    .unwrap()
                    .clone()
                    .into_iter(),
            );
        bank.transaction_processor
            .builtin_program_ids
            .write()
            .unwrap()
            .clear();
        bank.finish_init(&genesis_config, None, false);

        check_builtin_is_bpf(&bank);
    }
}
