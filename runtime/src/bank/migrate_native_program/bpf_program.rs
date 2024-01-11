use {
    super::error::MigrateNativeProgramError,
    crate::bank::Bank,
    solana_sdk::{
        account::Account, bpf_loader::ID as BPF_LOADER_ID,
        bpf_loader_upgradeable::get_program_data_address, pubkey::Pubkey,
    },
};

/// Struct for holding the configuration of a BPF (non-upgradeable) program
/// intending to replace a native program.
///
/// This struct is used to validate the BPF (non-upgradeable) program's account
/// and data account before the migration is performed.
#[allow(dead_code)] // TODO: Removed in future commit
#[derive(Debug)]
struct BpfProgramConfig {
    program_address: Pubkey,
    program_account: Account,
    total_data_size: usize,
}
#[allow(dead_code)] // TODO: Removed in future commit
impl BpfProgramConfig {
    /// Creates a new migration config for the given BPF (non-upgradeable)
    /// program, validating the BPF program's account and data account
    fn new_checked(bank: &Bank, address: &Pubkey) -> Result<Self, MigrateNativeProgramError> {
        let program_address = *address;
        let program_account: Account = bank
            .get_account_with_fixed_root(&program_address)
            .ok_or(MigrateNativeProgramError::AccountNotFound(program_address))?
            .into();

        // The program account should be owned by the non-upgradeable loader and
        // be executable
        if program_account.owner != BPF_LOADER_ID {
            return Err(MigrateNativeProgramError::IncorrectOwner(program_address));
        }
        if !program_account.executable {
            return Err(MigrateNativeProgramError::AccountNotExecutable(
                program_address,
            ));
        }

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
            total_data_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_simple_test_bank,
        assert_matches::assert_matches,
        solana_sdk::{
            account::AccountSharedData, bpf_loader_upgradeable::ID as BPF_LOADER_UPGRADEABLE_ID,
        },
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
        bank.store_account_and_update_capitalization(&address, &account);
    }

    fn store_empty_account(bank: &Bank, address: &Pubkey) {
        bank.store_account_and_update_capitalization(&address, &AccountSharedData::default());
    }

    #[test]
    fn test_bpf_program_config() {
        let bank = create_simple_test_bank(0);

        let program_id = Pubkey::new_unique();
        let (program_data_address, _) = get_program_data_address(&program_id);

        let data = vec![6u8; 200];

        // Fail if the program account does not exist
        assert_matches!(
            BpfProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::AccountNotFound(_))
        );

        // Fail if the program account is not owned by the non-upgradeable loader
        store_account(&bank, &program_id, &data, true, &Pubkey::new_unique());
        assert_matches!(
            BpfProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::IncorrectOwner(_))
        );

        // Fail if the program account is not executable
        store_account(&bank, &program_id, &data, false, &BPF_LOADER_ID);
        assert_matches!(
            BpfProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::AccountNotExecutable(_))
        );

        // Store the proper program account
        store_account(&bank, &program_id, &data, true, &BPF_LOADER_ID);

        // Fail if the program data account exists
        store_account(
            &bank,
            &program_data_address,
            &data,
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            BpfProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::ProgramHasDataAccount(_))
        );

        let check_data = bincode::serialize(&data).unwrap();
        let check_data_len = check_data.len();
        let check_lamports = bank.get_minimum_balance_for_rent_exemption(check_data_len);
        let check_program_account = Account {
            data: check_data,
            executable: true,
            lamports: check_lamports,
            owner: BPF_LOADER_ID,
            ..Account::default()
        };

        // Success
        store_empty_account(&bank, &program_data_address);
        let bpf_program_config = BpfProgramConfig::new_checked(&bank, &program_id).unwrap();
        assert_eq!(bpf_program_config.program_address, program_id);
        assert_eq!(bpf_program_config.program_account, check_program_account);
        assert_eq!(bpf_program_config.total_data_size, check_data_len);
    }
}
