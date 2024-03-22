use {
    super::{error::CoreBpfMigrationError, CoreBpfMigrationTargetType},
    crate::bank::Bank,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        bpf_loader_upgradeable::get_program_data_address,
        native_loader::ID as NATIVE_LOADER_ID,
        pubkey::Pubkey,
    },
};

/// Used to validate a built-in program's account before migrating to Core BPF.
#[derive(Debug)]
pub(crate) struct TargetProgramBuiltin {
    pub program_address: Pubkey,
    pub program_account: AccountSharedData,
    pub program_data_address: Pubkey,
    pub total_data_size: usize,
}

impl TargetProgramBuiltin {
    /// Create a new migration configuration for a built-in program.
    pub(crate) fn new_checked(
        bank: &Bank,
        program_address: &Pubkey,
        migration_target: &CoreBpfMigrationTargetType,
    ) -> Result<Self, CoreBpfMigrationError> {
        let program_account = match migration_target {
            CoreBpfMigrationTargetType::Builtin => {
                // The program account should exist.
                let program_account = bank
                    .get_account_with_fixed_root(program_address)
                    .ok_or(CoreBpfMigrationError::AccountNotFound(*program_address))?;

                // The program account should be owned by the native loader.
                if program_account.owner() != &NATIVE_LOADER_ID {
                    return Err(CoreBpfMigrationError::IncorrectOwner(*program_address));
                }

                program_account
            }
            CoreBpfMigrationTargetType::Stateless => {
                // The program account should _not_ exist.
                if bank.get_account_with_fixed_root(program_address).is_some() {
                    return Err(CoreBpfMigrationError::AccountExists(*program_address));
                }

                AccountSharedData::default()
            }
        };

        let program_data_address = get_program_data_address(program_address);

        // The program data account should not exist.
        if bank
            .get_account_with_fixed_root(&program_data_address)
            .is_some()
        {
            return Err(CoreBpfMigrationError::ProgramHasDataAccount(
                *program_address,
            ));
        }

        // The total data size is the size of the program account's data.
        let total_data_size = program_account.data().len();

        Ok(Self {
            program_address: *program_address,
            program_account,
            program_data_address,
            total_data_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::{tests::create_simple_test_bank, ApplyFeatureActivationsCaller},
        solana_sdk::{
            account::Account,
            bpf_loader_upgradeable::{UpgradeableLoaderState, ID as BPF_LOADER_UPGRADEABLE_ID},
            feature, feature_set,
        },
        test_case::test_case,
    };

    fn store_account<T: serde::Serialize>(
        bank: &Bank,
        address: &Pubkey,
        data: &T,
        executable: bool,
        owner: &Pubkey,
    ) {
        let data = bincode::serialize(data).unwrap();
        let data_len = data.len();
        let lamports = bank.get_minimum_balance_for_rent_exemption(data_len);
        let account = AccountSharedData::from(Account {
            data,
            executable,
            lamports,
            owner: *owner,
            ..Account::default()
        });
        bank.store_account_and_update_capitalization(address, &account);
    }

    #[test_case(solana_sdk::address_lookup_table::program::id(), None)]
    #[test_case(solana_sdk::bpf_loader::id(), None)]
    #[test_case(solana_sdk::bpf_loader_deprecated::id(), None)]
    #[test_case(solana_sdk::bpf_loader_upgradeable::id(), None)]
    #[test_case(solana_sdk::compute_budget::id(), None)]
    #[test_case(solana_config_program::id(), None)]
    #[test_case(solana_stake_program::id(), None)]
    #[test_case(solana_system_program::id(), None)]
    #[test_case(solana_vote_program::id(), None)]
    #[test_case(
        solana_sdk::loader_v4::id(),
        Some(feature_set::enable_program_runtime_v2_and_loader_v4::id())
    )]
    #[test_case(
        solana_zk_token_sdk::zk_token_proof_program::id(),
        Some(feature_set::zk_token_sdk_enabled::id())
    )]
    fn test_target_program_builtin(program_address: Pubkey, activation_feature: Option<Pubkey>) {
        let migration_target = CoreBpfMigrationTargetType::Builtin;
        let mut bank = create_simple_test_bank(0);

        if let Some(feature_id) = activation_feature {
            // Activate the feature to enable the built-in program
            bank.store_account(
                &feature_id,
                &feature::create_account(
                    &feature::Feature { activated_at: None },
                    bank.get_minimum_balance_for_rent_exemption(feature::Feature::size_of()),
                ),
            );
            bank.apply_feature_activations(ApplyFeatureActivationsCaller::NewFromParent, false);
        }

        let program_account = bank.get_account_with_fixed_root(&program_address).unwrap();
        let program_data_address = get_program_data_address(&program_address);

        // Success
        let builtin_config =
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target).unwrap();
        assert_eq!(builtin_config.program_address, program_address);
        assert_eq!(builtin_config.program_account, program_account);
        assert_eq!(builtin_config.program_data_address, program_data_address);
        assert_eq!(builtin_config.total_data_size, program_account.data().len());

        // Fail if the program account is not owned by the native loader
        store_account(
            &bank,
            &program_address,
            &String::from("some built-in program"),
            true,
            &Pubkey::new_unique(), // Not the native loader
        );
        assert_eq!(
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target)
                .unwrap_err(),
            CoreBpfMigrationError::IncorrectOwner(program_address)
        );

        // Fail if the program data account exists
        store_account(
            &bank,
            &program_address,
            &program_account.data(),
            program_account.executable(),
            program_account.owner(),
        );
        store_account(
            &bank,
            &program_data_address,
            &UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            },
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_eq!(
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target)
                .unwrap_err(),
            CoreBpfMigrationError::ProgramHasDataAccount(program_address)
        );

        // Fail if the program account does not exist
        bank.store_account_and_update_capitalization(
            &program_address,
            &AccountSharedData::default(),
        );
        assert_eq!(
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target)
                .unwrap_err(),
            CoreBpfMigrationError::AccountNotFound(program_address)
        );
    }

    #[test_case(solana_sdk::feature::id())]
    #[test_case(solana_sdk::native_loader::id())]
    fn test_target_program_stateless_builtin(program_address: Pubkey) {
        let migration_target = CoreBpfMigrationTargetType::Stateless;
        let bank = create_simple_test_bank(0);

        let program_account = AccountSharedData::default();
        let program_data_address = get_program_data_address(&program_address);

        // Success
        let builtin_config =
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target).unwrap();
        assert_eq!(builtin_config.program_address, program_address);
        assert_eq!(builtin_config.program_account, program_account);
        assert_eq!(builtin_config.program_data_address, program_data_address);
        assert_eq!(builtin_config.total_data_size, program_account.data().len());

        // Fail if the program data account exists
        store_account(
            &bank,
            &program_data_address,
            &UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            },
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_eq!(
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target)
                .unwrap_err(),
            CoreBpfMigrationError::ProgramHasDataAccount(program_address)
        );

        // Fail if the program account exists
        store_account(
            &bank,
            &program_address,
            &String::from("some built-in program"),
            true,
            &NATIVE_LOADER_ID,
        );
        assert_eq!(
            TargetProgramBuiltin::new_checked(&bank, &program_address, &migration_target)
                .unwrap_err(),
            CoreBpfMigrationError::AccountExists(program_address)
        );
    }
}
