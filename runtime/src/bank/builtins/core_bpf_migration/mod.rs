#![allow(dead_code)] // Removed in later commit
pub(crate) mod error;
mod source_upgradeable_bpf;
mod target_builtin;

use {
    crate::bank::Bank,
    error::CoreBpfMigrationError,
    solana_program_runtime::{
        invoke_context::InvokeContext, loaded_programs::LoadedProgramsForTxBatch,
        sysvar_cache::SysvarCache,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::Slot,
        hash::Hash,
        instruction::InstructionError,
        pubkey::Pubkey,
        transaction_context::TransactionContext,
    },
    source_upgradeable_bpf::SourceUpgradeableBpf,
    std::sync::atomic::Ordering::Relaxed,
    target_builtin::TargetBuiltin,
};

/// Identifies the type of built-in program targeted for Core BPF migration.
/// The type of target determines whether the program should have a program
/// account or not, which is checked before migration.
#[derive(Debug, PartialEq)]
pub(crate) enum CoreBpfMigrationTargetType {
    /// A standard (stateful) builtin program must have a program account.
    Builtin,
    /// A stateless builtin must not have a program account.
    Stateless,
}

/// Configuration for migrating a built-in program to Core BPF.
#[derive(Debug, PartialEq)]
pub(crate) struct CoreBpfMigrationConfig {
    /// The program ID of the source program to be used to replace the builtin.
    pub source_program_id: Pubkey,
    /// The feature gate to trigger the migration to Core BPF.
    /// Note: This feature gate should never be the same as any builtin's
    /// `enable_feature_id`. It should always be a feature gate that will be
    /// activated after the builtin is already enabled.
    pub feature_id: Pubkey,
    /// The type of target to replace.
    pub migration_target: CoreBpfMigrationTargetType,
    /// Static message used to emit datapoint logging.
    /// This is used to identify the migration in the logs.
    /// Should be unique to the migration, ie:
    /// "migrate_{builtin/stateless}_to_core_bpf_{program_name}".
    pub datapoint_name: &'static str,
}

fn checked_add(a: usize, b: usize) -> Result<usize, CoreBpfMigrationError> {
    a.checked_add(b)
        .ok_or(CoreBpfMigrationError::ArithmeticOverflow)
}

/// Create an `AccountSharedData` with data initialized to
/// `UpgradeableLoaderState::Program` populated with the target's new data
/// account address.
///
/// Note that the account's data is initialized manually, but the rest of the
/// account's fields are inherited from the source program account, including
/// the lamports.
fn new_target_program_account(
    target: &TargetBuiltin,
    source: &SourceUpgradeableBpf,
) -> Result<AccountSharedData, CoreBpfMigrationError> {
    let state = UpgradeableLoaderState::Program {
        programdata_address: target.program_data_address,
    };
    let data = bincode::serialize(&state)?;
    // The source program account has the same state, so it should already have
    // a sufficient lamports balance to cover rent for this state.
    // Out of an abundance of caution, first ensure the source program
    // account's data is the same length as the serialized state.
    if source.program_account.data().len() != data.len() {
        return Err(CoreBpfMigrationError::InvalidProgramAccount(
            source.program_address,
        ));
    }
    // Then copy the source account's contents and overwrite the data with the
    // newly created target program account data.
    let mut account = source.program_account.clone();
    account.set_data_from_slice(&data);
    Ok(account)
}

/// Create an `AccountSharedData` with data initialized to
/// `UpgradeableLoaderState::ProgramData` populated with the current slot, as
/// well as the source program data account's upgrade authority and ELF.
///
/// Note that the account's data is initialized manually, but the rest of the
/// account's fields are inherited from the source program account, including
/// the lamports.
fn new_target_program_data_account(
    source: &SourceUpgradeableBpf,
    slot: Slot,
) -> Result<AccountSharedData, CoreBpfMigrationError> {
    let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
    // Deserialize the program data metadata to get the upgrade authority.
    if let UpgradeableLoaderState::ProgramData {
        upgrade_authority_address,
        ..
    } = bincode::deserialize(&source.program_data_account.data()[..programdata_data_offset])?
    {
        let mut account = source.program_data_account.clone();
        // This account's data was just partially deserialized into
        // `UpgradeableLoaderState`, so it's guaranteed to have at least enough
        // space for the same type to be serialized in.
        // The ELF should remain untouched, since it follows the
        // `UpgradeableLoaderState`.
        //
        // Serialize the new `UpgradeableLoaderState` with the bank's current
        // slot and the deserialized upgrade authority.
        bincode::serialize_into(
            account.data_as_mut_slice(),
            &UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address,
            },
        )?;
        return Ok(account);
    }
    Err(CoreBpfMigrationError::InvalidProgramDataAccount(
        source.program_data_address,
    ))
}

impl Bank {
    /// In order to properly update the newly migrated Core BPF program in
    /// the program cache, the migration must directly invoke the BPF
    /// Upgradeable Loader's deployment functionality for validating the ELF
    /// bytes against the current environment, as well as updating the program
    /// cache.
    ///
    /// Invoking the loader's `direct_deploy_program` function will update the
    /// program cache in the currently executing context (ie. `programs_loaded`
    /// and `programs_modified`), but the runtime must also propagate those
    /// updates to the currently active cache.
    fn directly_invoke_loader_v3_deploy(
        &self,
        builtin_program_id: &Pubkey,
        program_data_account: &AccountSharedData,
    ) -> Result<(), InstructionError> {
        let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
        let data_len = program_data_account.data().len();
        let elf = program_data_account
            .data()
            .get(programdata_data_offset..)
            .ok_or(InstructionError::InvalidAccountData)?;

        // Set up the two `LoadedProgramsForTxBatch` instances, as if
        // processing a new transaction batch.
        let programs_loaded = LoadedProgramsForTxBatch::new_from_cache(
            self.slot,
            self.epoch,
            &self.transaction_processor.program_cache.read().unwrap(),
        );
        let mut programs_modified = LoadedProgramsForTxBatch::new(
            self.slot,
            programs_loaded.environments.clone(),
            programs_loaded.upcoming_environments.clone(),
            programs_loaded.latest_root_epoch,
        );

        // Configure a dummy `InvokeContext` from the runtime's current
        // environment, as well as the two `LoadedProgramsForTxBatch`
        // instances configured above, then invoke the loader.
        {
            let compute_budget = self.runtime_config.compute_budget.unwrap_or_default();
            let mut sysvar_cache = SysvarCache::default();
            sysvar_cache.fill_missing_entries(|pubkey, set_sysvar| {
                if let Some(account) = self.get_account(pubkey) {
                    set_sysvar(account.data());
                }
            });

            let mut dummy_transaction_context = TransactionContext::new(
                vec![],
                self.rent_collector.rent.clone(),
                compute_budget.max_invoke_stack_height,
                compute_budget.max_instruction_trace_length,
            );

            let mut dummy_invoke_context = InvokeContext::new(
                &mut dummy_transaction_context,
                &sysvar_cache,
                None,
                compute_budget,
                &programs_loaded,
                &mut programs_modified,
                self.feature_set.clone(),
                Hash::default(),
                0,
            );

            solana_bpf_loader_program::direct_deploy_program(
                &mut dummy_invoke_context,
                builtin_program_id,
                &bpf_loader_upgradeable::id(),
                data_len,
                elf,
                self.slot,
            )?
        }

        // Update the program cache by merging with `programs_modified`, which
        // should have been updated by the deploy function.
        self.transaction_processor
            .program_cache
            .write()
            .unwrap()
            .merge(&programs_modified);

        Ok(())
    }

    pub(crate) fn migrate_builtin_to_core_bpf(
        &mut self,
        builtin_program_id: &Pubkey,
        config: &CoreBpfMigrationConfig,
    ) -> Result<(), CoreBpfMigrationError> {
        datapoint_info!(config.datapoint_name, ("slot", self.slot, i64));

        let target =
            TargetBuiltin::new_checked(self, builtin_program_id, &config.migration_target)?;
        let source = SourceUpgradeableBpf::new_checked(self, &config.source_program_id)?;

        // Attempt serialization first before modifying the bank.
        let new_target_program_account = new_target_program_account(&target, &source)?;
        let new_target_program_data_account = new_target_program_data_account(&source, self.slot)?;

        // Gather old and new account data sizes, for updating the bank's
        // accounts data size delta off-chain.
        // The old data size is the total size of all accounts involved.
        // The new data size is the total size of the source program accounts,
        // since the target program account is replaced with a new program
        // account of the same size as the source program account, and the
        // source program data account is copied to the target program data
        // account before both source program accounts are cleared.
        let target_program_len = target.program_account.data().len();
        let source_program_len = source.program_account.data().len();
        let source_program_data_len = source.program_data_account.data().len();
        let old_data_size = checked_add(
            target_program_len,
            checked_add(source_program_len, source_program_data_len)?,
        )?;
        let new_data_size = checked_add(source_program_len, source_program_data_len)?;

        // Deploy the new target Core BPF program.
        // This step will validate the program ELF against the current runtime
        // environment, as well as update the program cache.
        self.directly_invoke_loader_v3_deploy(
            &target.program_address,
            &source.program_data_account,
        )?;

        // Burn lamports from the target program account, since it will be
        // replaced.
        self.capitalization
            .fetch_sub(target.program_account.lamports(), Relaxed);

        // Replace the target builtin account with the
        // `new_target_program_account` and clear the source program account.
        self.store_account(&target.program_address, &new_target_program_account);
        self.store_account(&source.program_address, &AccountSharedData::default());

        // Copy the source program data account into the account at the target
        // builtin program's data address, which was verified to be empty by
        // `TargetBuiltin::new_checked`, then clear the source program data
        // account.
        self.store_account(
            &target.program_data_address,
            &new_target_program_data_account,
        );
        self.store_account(&source.program_data_address, &AccountSharedData::default());

        // Remove the built-in program from the bank's list of built-ins.
        self.builtin_program_ids.remove(&target.program_address);

        // Update the account data size delta.
        self.calculate_and_update_accounts_data_size_delta_off_chain(old_data_size, new_data_size);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_simple_test_bank,
        assert_matches::assert_matches,
        solana_program_runtime::loaded_programs::{LoadedProgram, LoadedProgramType},
        solana_sdk::{
            account_utils::StateMut,
            bpf_loader_upgradeable::{self, get_program_data_address},
            native_loader,
        },
    };

    const TEST_ELF: &[u8] =
        include_bytes!("../../../../../programs/bpf_loader/test_elfs/out/noop_aligned.so");

    const PROGRAM_DATA_OFFSET: usize = UpgradeableLoaderState::size_of_programdata_metadata();

    struct TestContext {
        builtin_id: Pubkey,
        source_program_id: Pubkey,
        upgrade_authority_address: Option<Pubkey>,
        elf: Vec<u8>,
    }
    impl TestContext {
        // Initialize some test values and set up the source BPF upgradeable
        // program in the bank.
        fn new(bank: &Bank) -> Self {
            let builtin_id = Pubkey::new_unique();
            let source_program_id = Pubkey::new_unique();
            let upgrade_authority_address = Some(Pubkey::new_unique());
            let elf = TEST_ELF.to_vec();

            let source_program_data_address = get_program_data_address(&source_program_id);

            let source_program_account = {
                let data = bincode::serialize(&UpgradeableLoaderState::Program {
                    programdata_address: source_program_data_address,
                })
                .unwrap();

                let data_len = data.len();
                let lamports = bank.get_minimum_balance_for_rent_exemption(data_len);

                let mut account =
                    AccountSharedData::new(lamports, data_len, &bpf_loader_upgradeable::id());
                account.set_data(data);
                account
            };

            let source_program_data_account = {
                let mut data = bincode::serialize(&UpgradeableLoaderState::ProgramData {
                    slot: 99, // Arbitrary slot for testing.
                    upgrade_authority_address,
                })
                .unwrap();
                data.extend_from_slice(&elf);

                let data_len = data.len();
                let lamports = bank.get_minimum_balance_for_rent_exemption(data_len);

                let mut account =
                    AccountSharedData::new(lamports, data_len, &bpf_loader_upgradeable::id());
                account.set_data(data);
                account
            };

            bank.store_account_and_update_capitalization(
                &source_program_id,
                &source_program_account,
            );
            bank.store_account_and_update_capitalization(
                &source_program_data_address,
                &source_program_data_account,
            );

            Self {
                builtin_id,
                source_program_id,
                upgrade_authority_address,
                elf,
            }
        }

        // Evaluate the account state of the builtin and source post-migration.
        // Ensure the builtin program account is now a BPF upgradeable program,
        // the source program account and data account have been cleared, and
        // the bank's builtin IDs and cache have been updated.
        fn run_program_checks_post_migration(&self, bank: &Bank) {
            // Verify both the source program account and source program data
            // account have been cleared.
            assert!(bank.get_account(&self.source_program_id).is_none());
            assert!(bank
                .get_account(&get_program_data_address(&self.source_program_id))
                .is_none());

            let program_account = bank.get_account(&self.builtin_id).unwrap();
            let program_data_address = get_program_data_address(&self.builtin_id);

            // Program account is owned by the upgradeable loader.
            assert_eq!(program_account.owner(), &bpf_loader_upgradeable::id());

            // Program account has the correct state, with a pointer to its program
            // data address.
            let program_account_state: UpgradeableLoaderState = program_account.state().unwrap();
            assert_eq!(
                program_account_state,
                UpgradeableLoaderState::Program {
                    programdata_address: program_data_address
                }
            );

            let program_data_account = bank.get_account(&program_data_address).unwrap();

            // Program data account is owned by the upgradeable loader.
            assert_eq!(program_data_account.owner(), &bpf_loader_upgradeable::id());

            // Program data account has the correct state.
            // It should exactly match the original, including upgrade authority
            // and slot.
            let program_data_account_state_metadata: UpgradeableLoaderState =
                bincode::deserialize(&program_data_account.data()[..PROGRAM_DATA_OFFSET]).unwrap();
            assert_eq!(
                program_data_account_state_metadata,
                UpgradeableLoaderState::ProgramData {
                    slot: bank.slot, // _Not_ the original deployment slot
                    upgrade_authority_address: self.upgrade_authority_address  // Preserved
                },
            );
            assert_eq!(
                &program_data_account.data()[PROGRAM_DATA_OFFSET..],
                &self.elf,
            );

            // The bank's builtins should no longer contain the builtin
            // program ID.
            assert!(!bank.builtin_program_ids.contains(&self.builtin_id));

            // The cache should contain the target program.
            let program_cache = bank.transaction_processor.program_cache.read().unwrap();
            let entries = program_cache.get_flattened_entries(true, true);
            let target_entry = entries
                .iter()
                .find(|(program_id, _)| program_id == &self.builtin_id)
                .map(|(_, entry)| entry)
                .unwrap();

            // The target program entry should be updated.
            assert_eq!(target_entry.account_size, program_data_account.data().len());
            assert_eq!(target_entry.deployment_slot, bank.slot());
            assert_eq!(target_entry.effective_slot, bank.slot() + 1);
            assert_eq!(target_entry.latest_access_slot.load(Relaxed), bank.slot());

            // The target program entry should now be a BPF program.
            assert_matches!(target_entry.program, LoadedProgramType::LegacyV1(..));
        }
    }

    #[test]
    fn test_migrate_builtin() {
        let mut bank = create_simple_test_bank(0);

        let test_context = TestContext::new(&bank);

        let TestContext {
            builtin_id,
            source_program_id,
            ..
        } = test_context;

        // This will be checked by `TargetBuiltinProgram::new_checked`, but set
        // up the mock builtin and ensure it exists as configured.
        let builtin_account = {
            let builtin_name = String::from("test_builtin");
            let account =
                AccountSharedData::new_data(1, &builtin_name, &native_loader::id()).unwrap();
            bank.store_account_and_update_capitalization(&builtin_id, &account);
            bank.add_builtin(builtin_id, builtin_name.as_str(), LoadedProgram::default());
            account
        };
        assert_eq!(&bank.get_account(&builtin_id).unwrap(), &builtin_account);

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_program_id,
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "test_migrate_builtin",
        };

        // Gather bank information to check later.
        let bank_pre_migration_capitalization = bank.capitalization();
        let bank_pre_migration_accounts_data_size_delta_off_chain =
            bank.accounts_data_size_delta_off_chain.load(Relaxed);

        // Perform the migration.
        bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
            .unwrap();

        // Run the post-migration program checks.
        test_context.run_program_checks_post_migration(&bank);

        // The bank's capitalization should reflect the burned lamports
        // from the replaced builtin program account.
        assert_eq!(
            bank.capitalization(),
            bank_pre_migration_capitalization - builtin_account.lamports()
        );

        // The bank's accounts data size delta off-chain should reflect the
        // change in data size from the replaced builtin program account.
        assert_eq!(
            bank.accounts_data_size_delta_off_chain.load(Relaxed),
            bank_pre_migration_accounts_data_size_delta_off_chain
                - builtin_account.data().len() as i64,
        );
    }

    #[test]
    fn test_migrate_stateless_builtin() {
        let mut bank = create_simple_test_bank(0);

        let test_context = TestContext::new(&bank);

        let TestContext {
            builtin_id,
            source_program_id,
            ..
        } = test_context;

        // This will be checked by `TargetBuiltinProgram::new_checked`, but
        // assert the stateless builtin account doesn't exist.
        assert!(bank.get_account(&builtin_id).is_none());

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_program_id,
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Stateless,
            datapoint_name: "test_migrate_stateless_builtin",
        };

        // Gather bank information to check later.
        let bank_pre_migration_capitalization = bank.capitalization();
        let bank_pre_migration_accounts_data_size_delta_off_chain =
            bank.accounts_data_size_delta_off_chain.load(Relaxed);

        // Perform the migration.
        bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
            .unwrap();

        // Run the post-migration program checks.
        test_context.run_program_checks_post_migration(&bank);

        // The bank's capitalization should be exactly the same.
        assert_eq!(bank.capitalization(), bank_pre_migration_capitalization);

        // The bank's accounts data size delta off-chain should be exactly the
        // same.
        assert_eq!(
            bank.accounts_data_size_delta_off_chain.load(Relaxed),
            bank_pre_migration_accounts_data_size_delta_off_chain,
        );
    }
}
