use {
    super::error::MigrateNativeProgramError,
    crate::bank::Bank,
    solana_sdk::{
        account::Account,
        bpf_loader_upgradeable::{
            get_program_data_address, UpgradeableLoaderState, ID as BPF_LOADER_UPGRADEABLE_ID,
        },
        pubkey::Pubkey,
    },
};

/// Deserializes the given data into an `UpgradeableLoaderState` and checks
/// that the program data address matches the given address
fn check_upgradeable_loader_state(
    data: &[u8],
    program_data_address: &Pubkey,
) -> Result<(), MigrateNativeProgramError> {
    let deserialized_program_data: UpgradeableLoaderState = bincode::deserialize(data)?;
    if let UpgradeableLoaderState::Program {
        programdata_address,
    } = deserialized_program_data
    {
        if programdata_address != *program_data_address {
            return Err(MigrateNativeProgramError::InvalidProgramDataAccount(
                *program_data_address,
            ));
        }
    } else {
        return Err(MigrateNativeProgramError::InvalidProgramDataAccount(
            *program_data_address,
        ));
    }
    Ok(())
}

/// Struct for holding the configuration of a BPF upgradeable program intending
/// to replace a native program.
///
/// This struct is used to validate the BPF upgradeable program's account and
/// data account before the migration is performed.
#[derive(Debug)]
pub struct BpfUpgradeableProgramConfig {
    pub program_address: Pubkey,
    pub program_account: Account,
    pub program_data_address: Pubkey,
    pub program_data_account: Account,
    pub total_data_size: usize,
}
impl BpfUpgradeableProgramConfig {
    /// Creates a new migration config for the given BPF upgradeable program,
    /// validating the BPF program's account and data account
    pub fn new_checked(bank: &Bank, address: &Pubkey) -> Result<Self, MigrateNativeProgramError> {
        let program_address = *address;
        let program_account: Account = bank
            .get_account_with_fixed_root(&program_address)
            .ok_or(MigrateNativeProgramError::AccountNotFound(program_address))?
            .into();

        // The program account should be owned by the upgradeable loader and
        // be executable
        if program_account.owner != BPF_LOADER_UPGRADEABLE_ID {
            return Err(MigrateNativeProgramError::IncorrectOwner(program_address));
        }
        if !program_account.executable {
            return Err(MigrateNativeProgramError::AccountNotExecutable(
                program_address,
            ));
        }

        // The program account should have a pointer to its data account
        let (program_data_address, _) = get_program_data_address(&program_address);
        check_upgradeable_loader_state(&program_account.data, &program_data_address)?;

        // The program data account should exist
        let program_data_account: Account = bank
            .get_account_with_fixed_root(&program_data_address)
            .ok_or(MigrateNativeProgramError::ProgramHasNoDataAccount(
                program_address,
            ))?
            .into();

        // The program data account should be owned by the upgradeable loader
        // and _not_ be executable
        if program_data_account.owner != BPF_LOADER_UPGRADEABLE_ID {
            return Err(MigrateNativeProgramError::IncorrectOwner(
                program_data_address,
            ));
        }
        if program_data_account.executable {
            return Err(MigrateNativeProgramError::AccountIsExecutable(
                program_data_address,
            ));
        }

        // The total data size is the size of the program account's data plus
        // the size of the program data account's data
        let total_data_size = program_account
            .data
            .len()
            .checked_add(program_data_account.data.len())
            .ok_or(MigrateNativeProgramError::ArithmeticOverflow)?;

        Ok(Self {
            program_address,
            program_account,
            program_data_address,
            program_data_account,
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

    #[test]
    fn test_bpf_upgradeable_program_config() {
        let bank = create_simple_test_bank(0);

        let program_id = Pubkey::new_unique();
        let (program_data_address, _) = get_program_data_address(&program_id);

        let data = vec![6u8; 200];

        // Fail if the program account does not exist
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::AccountNotFound(_))
        );

        // Fail if the program account is not owned by the upgradeable loader
        store_account(&bank, &program_id, &[0u8], true, &Pubkey::new_unique());
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::IncorrectOwner(_))
        );

        // Fail if the program account is not executable
        store_account(
            &bank,
            &program_id,
            &[0u8],
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::AccountNotExecutable(_))
        );

        // Fail if the program account's state is not `UpgradeableLoaderState::Program`
        store_account(&bank, &program_id, &data, true, &BPF_LOADER_UPGRADEABLE_ID);
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::FailedToDeserialize(_))
        );

        // Fail if the program account's state is `UpgradeableLoaderState::Program`,
        // but it points to the wrong data account
        store_account(
            &bank,
            &program_id,
            &UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(),
            },
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::InvalidProgramDataAccount(_))
        );

        // Store the proper program account
        store_account(
            &bank,
            &program_id,
            &UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            },
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        // Fail if the program data account does not exist
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::ProgramHasNoDataAccount(_))
        );

        // Fail if the program data account is not owned by the upgradeable loader
        store_account(
            &bank,
            &program_data_address,
            &data,
            false,
            &Pubkey::new_unique(),
        );
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::IncorrectOwner(_))
        );

        // Fail if the program data account is executable
        store_account(
            &bank,
            &program_data_address,
            &data,
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id),
            Err(MigrateNativeProgramError::AccountIsExecutable(_))
        );

        // Store the proper program data account
        store_account(
            &bank,
            &program_data_address,
            &data,
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        let check_program_account_data = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: program_data_address,
        })
        .unwrap();
        let check_program_account_data_len = check_program_account_data.len();
        let check_program_account = Account {
            data: check_program_account_data,
            executable: true,
            lamports: bank.get_minimum_balance_for_rent_exemption(check_program_account_data_len),
            owner: BPF_LOADER_UPGRADEABLE_ID,
            ..Account::default()
        };
        let check_program_data_account_data = bincode::serialize(&data).unwrap();
        let check_program_data_account_data_len = check_program_data_account_data.len();
        let check_program_data_account = Account {
            data: check_program_data_account_data,
            executable: false,
            lamports: bank
                .get_minimum_balance_for_rent_exemption(check_program_data_account_data_len),
            owner: BPF_LOADER_UPGRADEABLE_ID,
            ..Account::default()
        };

        // Success
        let bpf_upgradeable_program_config =
            BpfUpgradeableProgramConfig::new_checked(&bank, &program_id).unwrap();
        assert_eq!(bpf_upgradeable_program_config.program_address, program_id);
        assert_eq!(
            bpf_upgradeable_program_config.program_account,
            check_program_account
        );
        assert_eq!(
            bpf_upgradeable_program_config.program_data_address,
            program_data_address
        );
        assert_eq!(
            bpf_upgradeable_program_config.program_data_account,
            check_program_data_account
        );
        assert_eq!(
            bpf_upgradeable_program_config.total_data_size,
            check_program_account_data_len + check_program_data_account_data_len
        );
    }
}
