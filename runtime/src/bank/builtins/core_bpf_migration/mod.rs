pub(crate) mod error;
mod source_buffer;
mod target_builtin;
mod target_core_bpf;

use {
    crate::bank::Bank,
    error::CoreBpfMigrationError,
    num_traits::{CheckedAdd, CheckedSub},
    solana_program_runtime::{
        invoke_context::{EnvironmentConfig, InvokeContext},
        loaded_programs::ProgramCacheForTxBatch,
        sysvar_cache::SysvarCache,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        hash::Hash,
        instruction::InstructionError,
        pubkey::Pubkey,
        transaction_context::TransactionContext,
    },
    source_buffer::SourceBuffer,
    std::{cmp::Ordering, sync::atomic::Ordering::Relaxed},
    target_builtin::TargetBuiltin,
    target_core_bpf::TargetCoreBpf,
};

/// Identifies the type of built-in program targeted for Core BPF migration.
/// The type of target determines whether the program should have a program
/// account or not, which is checked before migration.
#[allow(dead_code)] // Remove after first migration is configured.
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
    /// The address of the source buffer account to be used to replace the
    /// builtin.
    pub source_buffer_address: Pubkey,
    /// The authority to be used as the BPF program's upgrade authority.
    ///
    /// Note: If this value is set to `None`, then the migration will ignore
    /// the source buffer account's authority. If it's set to any `Some(..)`
    /// value, then the migration will perform a sanity check to ensure the
    /// source buffer account's authority matches the provided value.
    pub upgrade_authority_address: Option<Pubkey>,
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

fn checked_add<T: CheckedAdd>(a: T, b: T) -> Result<T, CoreBpfMigrationError> {
    a.checked_add(&b)
        .ok_or(CoreBpfMigrationError::ArithmeticOverflow)
}

fn checked_sub<T: CheckedSub>(a: T, b: T) -> Result<T, CoreBpfMigrationError> {
    a.checked_sub(&b)
        .ok_or(CoreBpfMigrationError::ArithmeticOverflow)
}

impl Bank {
    /// Create an `AccountSharedData` with data initialized to
    /// `UpgradeableLoaderState::Program` populated with the target's new data
    /// account address.
    fn new_target_program_account(
        &self,
        target: &TargetBuiltin,
    ) -> Result<AccountSharedData, CoreBpfMigrationError> {
        let state = UpgradeableLoaderState::Program {
            programdata_address: target.program_data_address,
        };
        let lamports =
            self.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program());
        let mut account =
            AccountSharedData::new_data(lamports, &state, &bpf_loader_upgradeable::id())?;
        account.set_executable(true);
        Ok(account)
    }

    /// Create an `AccountSharedData` with data initialized to
    /// `UpgradeableLoaderState::ProgramData` populated with the current slot, as
    /// well as the source program data account's upgrade authority and ELF.
    ///
    /// This function accepts a provided upgrade authority address, which comes
    /// from the migration configuration. If the provided upgrade authority
    /// address is different from the source buffer account's upgrade authority
    /// address, the migration will fail. If the provided upgrade authority
    /// address is `None`, the migration will ignore the source buffer account's
    /// upgrade authority and set the new program data account's upgrade
    /// authority to `None`.
    fn new_target_program_data_account(
        &self,
        source: &SourceBuffer,
        upgrade_authority_address: Option<Pubkey>,
    ) -> Result<AccountSharedData, CoreBpfMigrationError> {
        let buffer_metadata_size = UpgradeableLoaderState::size_of_buffer_metadata();
        if let UpgradeableLoaderState::Buffer {
            authority_address: buffer_authority,
        } = bincode::deserialize(&source.buffer_account.data()[..buffer_metadata_size])?
        {
            if let Some(provided_authority) = upgrade_authority_address {
                if upgrade_authority_address != buffer_authority {
                    return Err(CoreBpfMigrationError::UpgradeAuthorityMismatch(
                        provided_authority,
                        buffer_authority,
                    ));
                }
            }

            let elf = &source.buffer_account.data()[buffer_metadata_size..];

            let programdata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
            let space = programdata_metadata_size + elf.len();
            let lamports = self.get_minimum_balance_for_rent_exemption(space);
            let owner = &bpf_loader_upgradeable::id();

            let programdata_metadata = UpgradeableLoaderState::ProgramData {
                slot: self.slot,
                upgrade_authority_address,
            };

            let mut account = AccountSharedData::new_data_with_space(
                lamports,
                &programdata_metadata,
                space,
                owner,
            )?;
            account.data_as_mut_slice()[programdata_metadata_size..].copy_from_slice(elf);

            Ok(account)
        } else {
            Err(CoreBpfMigrationError::InvalidBufferAccount(
                source.buffer_address,
            ))
        }
    }

    /// In order to properly update a newly migrated or upgraded Core BPF
    /// program in the program cache, the runtime must directly invoke the BPF
    /// Upgradeable Loader's deployment functionality for validating the ELF
    /// bytes against the current environment, as well as updating the program
    /// cache.
    ///
    /// Invoking the loader's `direct_deploy_program` function will update the
    /// program cache in the currently executing context, but the runtime must
    /// also propagate those updates to the currently active cache.
    fn directly_invoke_loader_v3_deploy(
        &self,
        program_id: &Pubkey,
        programdata: &[u8],
    ) -> Result<(), InstructionError> {
        let data_len = programdata.len();
        let progradata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
        let elf = &programdata[progradata_metadata_size..];
        // Set up the two `LoadedProgramsForTxBatch` instances, as if
        // processing a new transaction batch.
        let mut program_cache_for_tx_batch = ProgramCacheForTxBatch::new_from_cache(
            self.slot,
            self.epoch,
            &self.transaction_processor.program_cache.read().unwrap(),
        );

        // Configure a dummy `InvokeContext` from the runtime's current
        // environment, as well as the two `ProgramCacheForTxBatch`
        // instances configured above, then invoke the loader.
        {
            let compute_budget = self.compute_budget().unwrap_or_default();
            let mut sysvar_cache = SysvarCache::default();
            sysvar_cache.fill_missing_entries(|pubkey, set_sysvar| {
                if let Some(account) = self.get_account(pubkey) {
                    set_sysvar(account.data());
                }
            });

            let mut dummy_transaction_context = TransactionContext::new(
                vec![],
                self.rent_collector.rent.clone(),
                compute_budget.max_instruction_stack_depth,
                compute_budget.max_instruction_trace_length,
            );

            let mut dummy_invoke_context = InvokeContext::new(
                &mut dummy_transaction_context,
                &mut program_cache_for_tx_batch,
                EnvironmentConfig::new(
                    Hash::default(),
                    None,
                    None,
                    self.feature_set.clone(),
                    0,
                    &sysvar_cache,
                ),
                None,
                compute_budget,
            );

            solana_bpf_loader_program::direct_deploy_program(
                &mut dummy_invoke_context,
                program_id,
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
            .merge(&program_cache_for_tx_batch.drain_modified_entries());

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
        let source = SourceBuffer::new_checked(self, &config.source_buffer_address)?;

        // Attempt serialization first before modifying the bank.
        let new_target_program_account = self.new_target_program_account(&target)?;
        let new_target_program_data_account =
            self.new_target_program_data_account(&source, config.upgrade_authority_address)?;

        // Gather old and new account data sizes, for updating the bank's
        // accounts data size delta off-chain.
        // The old data size is the total size of all original accounts
        // involved.
        // The new data size is the total size of all the new program accounts.
        let old_data_size = checked_add(
            target.program_account.data().len(),
            source.buffer_account.data().len(),
        )?;
        let new_data_size = checked_add(
            new_target_program_account.data().len(),
            new_target_program_data_account.data().len(),
        )?;

        // Deploy the new target Core BPF program.
        // This step will validate the program ELF against the current runtime
        // environment, as well as update the program cache.
        self.directly_invoke_loader_v3_deploy(
            &target.program_address,
            new_target_program_data_account.data(),
        )?;

        // Calculate the lamports to burn.
        // The target program account will be replaced, so burn its lamports.
        // The source buffer account will be cleared, so burn its lamports.
        // The two new program accounts will need to be funded.
        let lamports_to_burn = checked_add(
            target.program_account.lamports(),
            source.buffer_account.lamports(),
        )?;
        let lamports_to_fund = checked_add(
            new_target_program_account.lamports(),
            new_target_program_data_account.lamports(),
        )?;

        // Update the bank's capitalization.
        match lamports_to_burn.cmp(&lamports_to_fund) {
            Ordering::Greater => {
                self.capitalization
                    .fetch_sub(checked_sub(lamports_to_burn, lamports_to_fund)?, Relaxed);
            }
            Ordering::Less => {
                self.capitalization
                    .fetch_add(checked_sub(lamports_to_fund, lamports_to_burn)?, Relaxed);
            }
            Ordering::Equal => (),
        }

        // Store the new program accounts and clear the source buffer account.
        self.store_account(&target.program_address, &new_target_program_account);
        self.store_account(
            &target.program_data_address,
            &new_target_program_data_account,
        );
        self.store_account(&source.buffer_address, &AccountSharedData::default());

        // Remove the built-in program from the bank's list of built-ins.
        self.transaction_processor
            .builtin_program_ids
            .write()
            .unwrap()
            .remove(&target.program_address);

        // Update the account data size delta.
        self.calculate_and_update_accounts_data_size_delta_off_chain(old_data_size, new_data_size);

        Ok(())
    }

    /// Upgrade a Core BPF program.
    /// To use this function, add a feature-gated callsite to bank's
    /// `apply_feature_activations` function, similar to below.
    ///
    /// ```ignore
    /// if new_feature_activations.contains(&feature_set::test_upgrade_program::id()) {
    ///     self.upgrade_core_bpf_program(
    ///        &core_bpf_program_address,
    ///        &source_buffer_address,
    ///        "test_upgrade_core_bpf_program",
    ///     );
    /// }
    /// ```
    #[allow(dead_code)] // Only used when an upgrade is configured.
    pub(crate) fn upgrade_core_bpf_program(
        &mut self,
        core_bpf_program_address: &Pubkey,
        source_buffer_address: &Pubkey,
        datapoint_name: &'static str,
    ) -> Result<(), CoreBpfMigrationError> {
        datapoint_info!(datapoint_name, ("slot", self.slot, i64));

        let target = TargetCoreBpf::new_checked(self, core_bpf_program_address)?;
        let source = SourceBuffer::new_checked(self, source_buffer_address)?;

        // Attempt serialization first before modifying the bank.
        let new_target_program_data_account =
            self.new_target_program_data_account(&source, target.upgrade_authority_address)?;

        // Gather old and new account data sizes, for updating the bank's
        // accounts data size delta off-chain.
        // Since the program account is not replaced, only the program data
        // account and the source buffer account are involved.
        let old_data_size = checked_add(
            target.program_data_account.data().len(),
            source.buffer_account.data().len(),
        )?;
        let new_data_size = new_target_program_data_account.data().len();

        // Deploy the new target Core BPF program.
        // This step will validate the program ELF against the current runtime
        // environment, as well as update the program cache.
        self.directly_invoke_loader_v3_deploy(
            &target.program_address,
            new_target_program_data_account.data(),
        )?;

        // Calculate the lamports to burn.
        // Since the program account is not replaced, only the program data
        // account and the source buffer account are involved.
        // The target program data account will be replaced, so burn its
        // lamports.
        // The source buffer account will be cleared, so burn its lamports.
        // The new program data account will need to be funded.
        let lamports_to_burn = checked_add(
            target.program_data_account.lamports(),
            source.buffer_account.lamports(),
        )?;
        let lamports_to_fund = new_target_program_data_account.lamports();

        // Update the bank's capitalization.
        match lamports_to_burn.cmp(&lamports_to_fund) {
            Ordering::Greater => {
                self.capitalization
                    .fetch_sub(checked_sub(lamports_to_burn, lamports_to_fund)?, Relaxed);
            }
            Ordering::Less => {
                self.capitalization
                    .fetch_add(checked_sub(lamports_to_fund, lamports_to_burn)?, Relaxed);
            }
            Ordering::Equal => (),
        }

        // Store the new program data account and clear the source buffer account.
        self.store_account(
            &target.program_data_address,
            &new_target_program_data_account,
        );
        self.store_account(&source.buffer_address, &AccountSharedData::default());

        // Update the account data size delta.
        self.calculate_and_update_accounts_data_size_delta_off_chain(old_data_size, new_data_size);

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        crate::bank::tests::create_simple_test_bank,
        assert_matches::assert_matches,
        solana_program_runtime::loaded_programs::{ProgramCacheEntry, ProgramCacheEntryType},
        solana_sdk::{
            account_utils::StateMut,
            bpf_loader_upgradeable::{self, get_program_data_address},
            clock::Slot,
            native_loader,
        },
        std::{fs::File, io::Read},
        test_case::test_case,
    };

    fn test_elf() -> Vec<u8> {
        let mut elf = Vec::new();
        File::open("../programs/bpf_loader/test_elfs/out/noop_aligned.so")
            .unwrap()
            .read_to_end(&mut elf)
            .unwrap();
        elf
    }

    pub(crate) struct TestContext {
        target_program_address: Pubkey,
        source_buffer_address: Pubkey,
        upgrade_authority_address: Option<Pubkey>,
        elf: Vec<u8>,
    }
    impl TestContext {
        // Initialize some test values and set up the source buffer account in
        // the bank.
        pub(crate) fn new(
            bank: &Bank,
            target_program_address: &Pubkey,
            source_buffer_address: &Pubkey,
            upgrade_authority_address: Option<Pubkey>,
        ) -> Self {
            let elf = test_elf();

            let source_buffer_account = {
                // BPF Loader always writes ELF bytes after
                // `UpgradeableLoaderState::size_of_buffer_metadata()`.
                let buffer_metadata_size = UpgradeableLoaderState::size_of_buffer_metadata();
                let space = buffer_metadata_size + elf.len();
                let lamports = bank.get_minimum_balance_for_rent_exemption(space);
                let owner = &bpf_loader_upgradeable::id();

                let buffer_metadata = UpgradeableLoaderState::Buffer {
                    authority_address: upgrade_authority_address,
                };

                let mut account = AccountSharedData::new_data_with_space(
                    lamports,
                    &buffer_metadata,
                    space,
                    owner,
                )
                .unwrap();
                account.data_as_mut_slice()[buffer_metadata_size..].copy_from_slice(&elf);
                account
            };

            bank.store_account_and_update_capitalization(
                source_buffer_address,
                &source_buffer_account,
            );

            Self {
                target_program_address: *target_program_address,
                source_buffer_address: *source_buffer_address,
                upgrade_authority_address,
                elf,
            }
        }

        // Given a bank, calculate the expected capitalization and accounts data
        // size delta off-chain after a migration, using the values stored in
        // the test context.
        pub(crate) fn calculate_post_migration_capitalization_and_accounts_data_size_delta_off_chain(
            &self,
            bank: &Bank,
        ) -> (u64, i64) {
            let builtin_account = bank
                .get_account(&self.target_program_address)
                .unwrap_or_default();
            let source_buffer_account = bank.get_account(&self.source_buffer_address).unwrap();
            let resulting_program_data_len = UpgradeableLoaderState::size_of_program();
            let resulting_programdata_data_len =
                UpgradeableLoaderState::size_of_programdata_metadata() + self.elf.len();
            let expected_post_migration_capitalization = bank.capitalization()
                - builtin_account.lamports()
                - source_buffer_account.lamports()
                + bank.get_minimum_balance_for_rent_exemption(resulting_program_data_len)
                + bank.get_minimum_balance_for_rent_exemption(resulting_programdata_data_len);
            let expected_post_migration_accounts_data_size_delta_off_chain =
                bank.accounts_data_size_delta_off_chain.load(Relaxed)
                    + resulting_program_data_len as i64
                    + resulting_programdata_data_len as i64
                    - builtin_account.data().len() as i64
                    - source_buffer_account.data().len() as i64;
            (
                expected_post_migration_capitalization,
                expected_post_migration_accounts_data_size_delta_off_chain,
            )
        }

        // Given a bank, calculate the expected capitalization and accounts data
        // size delta off-chain after an upgrade, using the values stored in
        // the test context.
        fn calculate_post_upgrade_capitalization_and_accounts_data_size_delta_off_chain(
            &self,
            bank: &Bank,
        ) -> (u64, i64) {
            let program_data_account = bank
                .get_account(&get_program_data_address(&self.target_program_address))
                .unwrap_or_default();
            let source_buffer_account = bank.get_account(&self.source_buffer_address).unwrap();
            let resulting_programdata_data_len =
                UpgradeableLoaderState::size_of_programdata_metadata() + self.elf.len();
            let expected_post_migration_capitalization = bank.capitalization()
                - program_data_account.lamports()
                - source_buffer_account.lamports()
                + bank.get_minimum_balance_for_rent_exemption(resulting_programdata_data_len);
            let expected_post_migration_accounts_data_size_delta_off_chain =
                bank.accounts_data_size_delta_off_chain.load(Relaxed)
                    + resulting_programdata_data_len as i64
                    - program_data_account.data().len() as i64
                    - source_buffer_account.data().len() as i64;
            (
                expected_post_migration_capitalization,
                expected_post_migration_accounts_data_size_delta_off_chain,
            )
        }

        // Evaluate the account state of the target and source.
        // After either a migration or upgrade:
        // * The target program is a BPF upgradeable program with a pointer to
        //   its program data address.
        // * The source buffer account is cleared.
        // * The bank's builtin IDs do not contain the target program address.
        // * The cache contains the target program, and the entry is updated.
        pub(crate) fn run_program_checks(&self, bank: &Bank, migration_or_upgrade_slot: Slot) {
            // Verify the source buffer account has been cleared.
            assert!(bank.get_account(&self.source_buffer_address).is_none());

            let program_account = bank.get_account(&self.target_program_address).unwrap();
            let program_data_address = get_program_data_address(&self.target_program_address);

            // Program account is owned by the upgradeable loader.
            assert_eq!(program_account.owner(), &bpf_loader_upgradeable::id());

            // Program account is executable.
            assert!(program_account.executable());

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
            // It should have the same update authority and ELF as the source
            // buffer account.
            // The slot should be the slot it was migrated at.
            let programdata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
            let program_data_account_state_metadata: UpgradeableLoaderState =
                bincode::deserialize(&program_data_account.data()[..programdata_metadata_size])
                    .unwrap();
            assert_eq!(
                program_data_account_state_metadata,
                UpgradeableLoaderState::ProgramData {
                    slot: migration_or_upgrade_slot,
                    upgrade_authority_address: self.upgrade_authority_address // Preserved
                },
            );
            assert_eq!(
                &program_data_account.data()[programdata_metadata_size..],
                &self.elf,
            );

            // The bank's builtins should not contain the target program
            // address.
            assert!(!bank
                .transaction_processor
                .builtin_program_ids
                .read()
                .unwrap()
                .contains(&self.target_program_address));

            // The cache should contain the target program.
            let program_cache = bank.transaction_processor.program_cache.read().unwrap();
            let entries = program_cache.get_flattened_entries(true, true);
            let target_entry = entries
                .iter()
                .find(|(program_id, _)| program_id == &self.target_program_address)
                .map(|(_, entry)| entry)
                .unwrap();

            // The target program entry should be updated.
            assert_eq!(target_entry.account_size, program_data_account.data().len());
            assert_eq!(target_entry.deployment_slot, migration_or_upgrade_slot);
            assert_eq!(target_entry.effective_slot, migration_or_upgrade_slot + 1);

            // The target program entry should be a BPF program.
            assert_matches!(target_entry.program, ProgramCacheEntryType::Loaded(..));
        }
    }

    #[test_case(Some(Pubkey::new_unique()); "with_upgrade_authority")]
    #[test_case(None; "without_upgrade_authority")]
    fn test_migrate_builtin(upgrade_authority_address: Option<Pubkey>) {
        let mut bank = create_simple_test_bank(0);

        let builtin_id = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        // This will be checked by `TargetBuiltinProgram::new_checked`, but set
        // up the mock builtin and ensure it exists as configured.
        let builtin_account = {
            let builtin_name = String::from("test_builtin");
            let account =
                AccountSharedData::new_data(1, &builtin_name, &native_loader::id()).unwrap();
            bank.store_account_and_update_capitalization(&builtin_id, &account);
            bank.transaction_processor.add_builtin(
                &bank,
                builtin_id,
                builtin_name.as_str(),
                ProgramCacheEntry::default(),
            );
            account
        };
        assert_eq!(&bank.get_account(&builtin_id).unwrap(), &builtin_account);

        let test_context = TestContext::new(
            &bank,
            &builtin_id,
            &source_buffer_address,
            upgrade_authority_address,
        );
        let TestContext {
            target_program_address: builtin_id,
            source_buffer_address,
            ..
        } = test_context;

        let (
            expected_post_migration_capitalization,
            expected_post_migration_accounts_data_size_delta_off_chain,
        ) = test_context
            .calculate_post_migration_capitalization_and_accounts_data_size_delta_off_chain(&bank);

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_buffer_address,
            upgrade_authority_address,
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "test_migrate_builtin",
        };

        // Perform the migration.
        let migration_slot = bank.slot();
        bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
            .unwrap();

        // Run the post-migration program checks.
        test_context.run_program_checks(&bank, migration_slot);

        // Check the bank's capitalization.
        assert_eq!(
            bank.capitalization(),
            expected_post_migration_capitalization
        );

        // Check the bank's accounts data size delta off-chain.
        assert_eq!(
            bank.accounts_data_size_delta_off_chain.load(Relaxed),
            expected_post_migration_accounts_data_size_delta_off_chain
        );
    }

    #[test_case(Some(Pubkey::new_unique()); "with_upgrade_authority")]
    #[test_case(None; "without_upgrade_authority")]
    fn test_migrate_stateless_builtin(upgrade_authority_address: Option<Pubkey>) {
        let mut bank = create_simple_test_bank(0);

        let builtin_id = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        let test_context = TestContext::new(
            &bank,
            &builtin_id,
            &source_buffer_address,
            upgrade_authority_address,
        );
        let TestContext {
            target_program_address: builtin_id,
            source_buffer_address,
            ..
        } = test_context;

        // This will be checked by `TargetBuiltinProgram::new_checked`, but
        // assert the stateless builtin account doesn't exist.
        assert!(bank.get_account(&builtin_id).is_none());

        let (
            expected_post_migration_capitalization,
            expected_post_migration_accounts_data_size_delta_off_chain,
        ) = test_context
            .calculate_post_migration_capitalization_and_accounts_data_size_delta_off_chain(&bank);

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_buffer_address,
            upgrade_authority_address,
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Stateless,
            datapoint_name: "test_migrate_stateless_builtin",
        };

        // Perform the migration.
        let migration_slot = bank.slot();
        bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
            .unwrap();

        // Run the post-migration program checks.
        test_context.run_program_checks(&bank, migration_slot);

        // Check the bank's capitalization.
        assert_eq!(
            bank.capitalization(),
            expected_post_migration_capitalization
        );

        // Check the bank's accounts data size delta off-chain.
        assert_eq!(
            bank.accounts_data_size_delta_off_chain.load(Relaxed),
            expected_post_migration_accounts_data_size_delta_off_chain
        );
    }

    #[test]
    fn test_migrate_fail_authority_mismatch() {
        let mut bank = create_simple_test_bank(0);

        let builtin_id = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        let upgrade_authority_address = Some(Pubkey::new_unique());

        {
            let builtin_name = String::from("test_builtin");
            let account =
                AccountSharedData::new_data(1, &builtin_name, &native_loader::id()).unwrap();
            bank.store_account_and_update_capitalization(&builtin_id, &account);
            bank.transaction_processor.add_builtin(
                &bank,
                builtin_id,
                builtin_name.as_str(),
                ProgramCacheEntry::default(),
            );
            account
        };

        let test_context = TestContext::new(
            &bank,
            &builtin_id,
            &source_buffer_address,
            upgrade_authority_address,
        );
        let TestContext {
            target_program_address: builtin_id,
            source_buffer_address,
            ..
        } = test_context;

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_buffer_address,
            upgrade_authority_address: Some(Pubkey::new_unique()), // Mismatch.
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "test_migrate_builtin",
        };

        assert_matches!(
            bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
                .unwrap_err(),
            CoreBpfMigrationError::UpgradeAuthorityMismatch(_, _)
        )
    }

    #[test]
    fn test_migrate_none_authority_with_some_buffer_authority() {
        let mut bank = create_simple_test_bank(0);

        let builtin_id = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        let upgrade_authority_address = Some(Pubkey::new_unique());

        {
            let builtin_name = String::from("test_builtin");
            let account =
                AccountSharedData::new_data(1, &builtin_name, &native_loader::id()).unwrap();
            bank.store_account_and_update_capitalization(&builtin_id, &account);
            bank.transaction_processor.add_builtin(
                &bank,
                builtin_id,
                builtin_name.as_str(),
                ProgramCacheEntry::default(),
            );
            account
        };

        // Set up the source buffer with a valid authority, but the migration
        // config will define the upgrade authority to be `None`.
        {
            let elf = test_elf();
            let buffer_metadata_size = UpgradeableLoaderState::size_of_buffer_metadata();
            let space = buffer_metadata_size + elf.len();
            let lamports = bank.get_minimum_balance_for_rent_exemption(space);
            let owner = &bpf_loader_upgradeable::id();

            let buffer_metadata = UpgradeableLoaderState::Buffer {
                authority_address: upgrade_authority_address,
            };

            let mut account =
                AccountSharedData::new_data_with_space(lamports, &buffer_metadata, space, owner)
                    .unwrap();
            account.data_as_mut_slice()[buffer_metadata_size..].copy_from_slice(&elf);

            bank.store_account_and_update_capitalization(&source_buffer_address, &account);
        }

        let core_bpf_migration_config = CoreBpfMigrationConfig {
            source_buffer_address,
            upgrade_authority_address: None, // None.
            feature_id: Pubkey::new_unique(),
            migration_target: CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "test_migrate_builtin",
        };

        bank.migrate_builtin_to_core_bpf(&builtin_id, &core_bpf_migration_config)
            .unwrap();

        let program_data_address = get_program_data_address(&builtin_id);
        let program_data_account = bank.get_account(&program_data_address).unwrap();
        let program_data_account_state: UpgradeableLoaderState =
            program_data_account.state().unwrap();
        assert_eq!(
            program_data_account_state,
            UpgradeableLoaderState::ProgramData {
                upgrade_authority_address: None,
                slot: bank.slot(),
            },
        );
    }

    fn set_up_test_core_bpf_program(
        bank: &mut Bank,
        program_address: &Pubkey,
        upgrade_authority_address: Option<Pubkey>,
    ) {
        // This will be checked by `TargetCoreBpf::new_checked`, but set
        // up the mock Core BPF program and ensure it exists as configured.
        let programdata_address = get_program_data_address(program_address);
        let program_account = {
            let data = bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address,
            })
            .unwrap();
            let space = data.len();
            let lamports = bank.get_minimum_balance_for_rent_exemption(space);
            let owner = &bpf_loader_upgradeable::id();

            let mut account = AccountSharedData::new(lamports, space, owner);
            account.set_executable(true);
            account.data_as_mut_slice().copy_from_slice(&data);
            bank.store_account_and_update_capitalization(program_address, &account);
            account
        };
        let program_data_account = {
            let elf = [4u8; 20]; // Mock ELF to start.
            let programdata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
            let space = programdata_metadata_size + elf.len();
            let lamports = bank.get_minimum_balance_for_rent_exemption(space);
            let owner = &bpf_loader_upgradeable::id();

            let programdata_metadata = UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address,
            };

            let mut account = AccountSharedData::new_data_with_space(
                lamports,
                &programdata_metadata,
                space,
                owner,
            )
            .unwrap();
            account.data_as_mut_slice()[programdata_metadata_size..].copy_from_slice(&elf);
            bank.store_account_and_update_capitalization(&programdata_address, &account);
            account
        };
        assert_eq!(
            &bank.get_account(program_address).unwrap(),
            &program_account
        );
        assert_eq!(
            &bank.get_account(&programdata_address).unwrap(),
            &program_data_account
        );
    }

    #[test_case(Some(Pubkey::new_unique()); "with_upgrade_authority")]
    #[test_case(None; "without_upgrade_authority")]
    fn test_upgrade_core_bpf_program(upgrade_authority_address: Option<Pubkey>) {
        let mut bank = create_simple_test_bank(0);

        let core_bpf_program_address = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        set_up_test_core_bpf_program(
            &mut bank,
            &core_bpf_program_address,
            upgrade_authority_address,
        );

        let test_context = TestContext::new(
            &bank,
            &core_bpf_program_address,
            &source_buffer_address,
            upgrade_authority_address,
        );
        let TestContext {
            source_buffer_address,
            ..
        } = test_context;

        let (
            expected_post_upgrade_capitalization,
            expected_post_upgrade_accounts_data_size_delta_off_chain,
        ) = test_context
            .calculate_post_upgrade_capitalization_and_accounts_data_size_delta_off_chain(&bank);

        // Perform the upgrade.
        let upgrade_slot = bank.slot();
        bank.upgrade_core_bpf_program(
            &core_bpf_program_address,
            &source_buffer_address,
            "test_upgrade_core_bpf_program",
        )
        .unwrap();

        // Run the post-upgrade program checks.
        test_context.run_program_checks(&bank, upgrade_slot);

        // Check the bank's capitalization.
        assert_eq!(bank.capitalization(), expected_post_upgrade_capitalization);

        // Check the bank's accounts data size delta off-chain.
        assert_eq!(
            bank.accounts_data_size_delta_off_chain.load(Relaxed),
            expected_post_upgrade_accounts_data_size_delta_off_chain
        );
    }

    #[test]
    fn test_upgrade_fail_authority_mismatch() {
        let mut bank = create_simple_test_bank(0);

        let program_address = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        let upgrade_authority_address = Some(Pubkey::new_unique());

        set_up_test_core_bpf_program(&mut bank, &program_address, upgrade_authority_address);

        let _test_context = TestContext::new(
            &bank,
            &program_address,
            &source_buffer_address,
            Some(Pubkey::new_unique()), // Mismatch.
        );

        assert_matches!(
            bank.upgrade_core_bpf_program(
                &program_address,
                &source_buffer_address,
                "test_upgrade_core_bpf_program"
            )
            .unwrap_err(),
            CoreBpfMigrationError::UpgradeAuthorityMismatch(_, _)
        )
    }

    #[test]
    fn test_upgrade_none_authority_with_some_buffer_authority() {
        let mut bank = create_simple_test_bank(0);

        let program_address = Pubkey::new_unique();
        let source_buffer_address = Pubkey::new_unique();

        set_up_test_core_bpf_program(&mut bank, &program_address, None);

        let _test_context = TestContext::new(
            &bank,
            &program_address,
            &source_buffer_address,
            Some(Pubkey::new_unique()), // Not `None`.
        );

        bank.upgrade_core_bpf_program(
            &program_address,
            &source_buffer_address,
            "test_upgrade_core_bpf_program",
        )
        .unwrap();

        let program_data_address = get_program_data_address(&program_address);
        let program_data_account = bank.get_account(&program_data_address).unwrap();
        let program_data_account_state: UpgradeableLoaderState =
            program_data_account.state().unwrap();
        assert_eq!(
            program_data_account_state,
            UpgradeableLoaderState::ProgramData {
                upgrade_authority_address: None,
                slot: bank.slot(),
            },
        );
    }
}
