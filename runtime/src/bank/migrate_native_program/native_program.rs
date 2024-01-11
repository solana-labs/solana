use {
    super::{error::MigrateNativeProgramError, NativeProgram},
    crate::bank::Bank,
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader_upgradeable::get_program_data_address,
        native_loader::ID as NATIVE_LOADER_ID,
        pubkey::Pubkey,
    },
};

/// Struct for holding the configuration of a native program that is being
/// migrated to a BPF program.
///
/// This struct is used to validate the native program's account and data
/// account before the migration is performed.
#[allow(dead_code)] // TODO: Removed in future commit
#[derive(Debug)]
struct NativeProgramConfig {
    program_address: Pubkey,
    program_account: Account,
    program_data_address: Pubkey,
    total_data_size: usize,
}
#[allow(dead_code)] // TODO: Removed in future commit
impl NativeProgramConfig {
    /// Creates a new migration config for the given native program,
    /// validating the native program's account and data account
    fn new_checked(
        bank: &Bank,
        native_program: NativeProgram,
    ) -> Result<Self, MigrateNativeProgramError> {
        let program_address = native_program.id();
        let program_account: Account = if native_program.is_synthetic() {
            // The program account should _not_ exist
            if bank.get_account_with_fixed_root(&program_address).is_some() {
                return Err(MigrateNativeProgramError::AccountExists(program_address));
            }

            AccountSharedData::default().into()
        } else {
            let program_account: Account = bank
                .get_account_with_fixed_root(&program_address)
                .ok_or(MigrateNativeProgramError::AccountNotFound(program_address))?
                .into();

            // The program account should be owned by the native loader and be
            // executable
            if program_account.owner != NATIVE_LOADER_ID {
                return Err(MigrateNativeProgramError::IncorrectOwner(program_address));
            }
            if !program_account.executable {
                return Err(MigrateNativeProgramError::AccountNotExecutable(
                    program_address,
                ));
            }

            program_account
        };

        // The program data account should _not_ exist
        let (program_data_address, _) = get_program_data_address(&program_address);
        if bank
            .get_account_with_fixed_root(&program_data_address)
            .is_some()
        {
            return Err(MigrateNativeProgramError::ProgramHasDataAccount(
                program_address,
            ));
        }

        // The total data size is the size of the program account's data
        let total_data_size = program_account.data.len();

        Ok(Self {
            program_address,
            program_account,
            program_data_address,
            total_data_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::bank::tests::create_simple_test_bank, assert_matches::assert_matches,
        solana_sdk::bpf_loader_upgradeable::ID as BPF_LOADER_UPGRADEABLE_ID, test_case::test_case,
    };

    fn store_default_account(bank: &Bank, address: &Pubkey, executable: bool, owner: &Pubkey) {
        let lamports = bank.get_minimum_balance_for_rent_exemption(0);
        let account = AccountSharedData::from(Account {
            executable,
            lamports,
            owner: *owner,
            ..Account::default()
        });
        bank.store_account_and_update_capitalization(&address, &account);
    }

    fn store_empty_account(bank: &Bank, address: &Pubkey) {
        bank.store_account_and_update_capitalization(&address, &AccountSharedData::default());
    }

    #[test_case(NativeProgram::AddressLookupTable)]
    #[test_case(NativeProgram::BpfLoader)]
    #[test_case(NativeProgram::BpfLoaderUpgradeable)]
    #[test_case(NativeProgram::ComputeBudget)]
    #[test_case(NativeProgram::Config)]
    #[test_case(NativeProgram::Ed25519)]
    #[test_case(NativeProgram::FeatureGate)]
    #[test_case(NativeProgram::LoaderV4)]
    #[test_case(NativeProgram::NativeLoader)]
    #[test_case(NativeProgram::Secp256k1)]
    #[test_case(NativeProgram::Stake)]
    #[test_case(NativeProgram::System)]
    #[test_case(NativeProgram::Vote)]
    #[test_case(NativeProgram::ZkTokenProof)]
    fn test_native_program_config(native_program: NativeProgram) {
        let bank = create_simple_test_bank(0);

        let check_program_id = native_program.id();
        let check_program_account: Account = bank
            .get_account_with_fixed_root(&check_program_id)
            .unwrap_or_default() // AccountSharedData::default() for synthetic programs
            .into();
        let check_program_data_address = get_program_data_address(&check_program_id).0;

        let native_program_config =
            NativeProgramConfig::new_checked(&bank, native_program).unwrap();

        assert_eq!(native_program_config.program_address, check_program_id);
        assert_eq!(native_program_config.program_account, check_program_account);
        assert_eq!(
            native_program_config.program_data_address,
            check_program_data_address
        );
        assert_eq!(
            native_program_config.total_data_size,
            check_program_account.data.len()
        );
    }

    #[test_case(NativeProgram::FeatureGate)]
    #[test_case(NativeProgram::LoaderV4)]
    #[test_case(NativeProgram::NativeLoader)]
    #[test_case(NativeProgram::ZkTokenProof)]
    fn test_native_program_config_synthetic(native_program: NativeProgram) {
        let bank = create_simple_test_bank(0);

        let program_id = native_program.id();
        let (program_data_address, _) = get_program_data_address(&program_id);

        // Fail if the program data account exists
        store_default_account(
            &bank,
            &program_data_address,
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::ProgramHasDataAccount(_)
        );

        // Fail if the program account exists
        store_default_account(&bank, &program_id, true, &NATIVE_LOADER_ID);
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::AccountExists(_)
        );
    }

    #[test_case(NativeProgram::AddressLookupTable)]
    #[test_case(NativeProgram::BpfLoader)]
    #[test_case(NativeProgram::BpfLoaderUpgradeable)]
    #[test_case(NativeProgram::ComputeBudget)]
    #[test_case(NativeProgram::Config)]
    #[test_case(NativeProgram::Ed25519)]
    #[test_case(NativeProgram::Secp256k1)]
    #[test_case(NativeProgram::Stake)]
    #[test_case(NativeProgram::System)]
    #[test_case(NativeProgram::Vote)]
    fn test_native_program_config_non_synthetic(native_program: NativeProgram) {
        let bank = create_simple_test_bank(0);

        let program_id = native_program.id();
        let (program_data_address, _) = get_program_data_address(&program_id);

        // Fail if the program data account exists
        store_default_account(
            &bank,
            &program_data_address,
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::ProgramHasDataAccount(_)
        );

        // Fail if the program account does not exist
        store_empty_account(&bank, &program_id);
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::AccountNotFound(_)
        );

        // Fail if the owner is not the native loader
        store_default_account(&bank, &program_id, true, &Pubkey::new_unique());
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::IncorrectOwner(_)
        );

        // Fail if the program account is not executable
        store_default_account(&bank, &program_id, false, &NATIVE_LOADER_ID);
        assert_matches!(
            NativeProgramConfig::new_checked(&bank, native_program).unwrap_err(),
            MigrateNativeProgramError::AccountNotExecutable(_)
        );
    }
}
