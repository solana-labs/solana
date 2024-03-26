use {
    super::error::CoreBpfMigrationError,
    crate::bank::Bank,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        bpf_loader_upgradeable::{
            get_program_data_address, UpgradeableLoaderState, ID as BPF_LOADER_UPGRADEABLE_ID,
        },
        pubkey::Pubkey,
    },
};

/// The account details of an Upgradeable BPF program slated to replace a
/// built-in program.
#[derive(Debug)]
pub(crate) struct SourceUpgradeableBpf {
    pub program_address: Pubkey,
    pub program_account: AccountSharedData,
    pub program_data_address: Pubkey,
    pub program_data_account: AccountSharedData,
}

impl SourceUpgradeableBpf {
    fn check_program_account(&self) -> Result<(), CoreBpfMigrationError> {
        // The program account should be owned by the upgradeable loader.
        if self.program_account.owner() != &BPF_LOADER_UPGRADEABLE_ID {
            return Err(CoreBpfMigrationError::IncorrectOwner(self.program_address));
        }

        // The program account should have a pointer to its data account.
        if let UpgradeableLoaderState::Program {
            programdata_address,
        } = &self.program_account.deserialize_data()?
        {
            if programdata_address != &self.program_data_address {
                return Err(CoreBpfMigrationError::InvalidProgramAccount(
                    self.program_address,
                ));
            }
        }

        Ok(())
    }

    fn check_program_data_account(&self) -> Result<(), CoreBpfMigrationError> {
        // The program data account should be owned by the upgradeable loader.
        if self.program_data_account.owner() != &BPF_LOADER_UPGRADEABLE_ID {
            return Err(CoreBpfMigrationError::IncorrectOwner(
                self.program_data_address,
            ));
        }

        // The program data account should have the correct state.
        let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
        if self.program_data_account.data().len() < programdata_data_offset {
            return Err(CoreBpfMigrationError::InvalidProgramDataAccount(
                self.program_data_address,
            ));
        }
        // Length checked in previous block.
        match bincode::deserialize::<UpgradeableLoaderState>(
            &self.program_data_account.data()[..programdata_data_offset],
        )? {
            UpgradeableLoaderState::ProgramData { .. } => Ok(()),
            _ => Err(CoreBpfMigrationError::InvalidProgramDataAccount(
                self.program_data_address,
            )),
        }
    }

    /// Collects the details of an upgradeable BPF program and verifies it is
    /// properly configured.
    /// The program account should exist with a pointer to its data account.
    /// The program data account should exist with the correct state
    /// (a ProgramData header and the program ELF).
    pub(crate) fn new_checked(
        bank: &Bank,
        program_address: &Pubkey,
    ) -> Result<Self, CoreBpfMigrationError> {
        // The program account should exist.
        let program_account = bank
            .get_account_with_fixed_root(program_address)
            .ok_or(CoreBpfMigrationError::AccountNotFound(*program_address))?;

        // The program data account should exist.
        let program_data_address = get_program_data_address(program_address);
        let program_data_account = bank
            .get_account_with_fixed_root(&program_data_address)
            .ok_or(CoreBpfMigrationError::ProgramHasNoDataAccount(
                *program_address,
            ))?;

        let source_upgradeable_bpf = Self {
            program_address: *program_address,
            program_account,
            program_data_address,
            program_data_account,
        };

        source_upgradeable_bpf.check_program_account()?;
        source_upgradeable_bpf.check_program_data_account()?;

        Ok(source_upgradeable_bpf)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_simple_test_bank,
        assert_matches::assert_matches,
        solana_sdk::{account::Account, bpf_loader_upgradeable::ID as BPF_LOADER_UPGRADEABLE_ID},
    };

    fn store_account<T: serde::Serialize>(
        bank: &Bank,
        address: &Pubkey,
        data: &T,
        additional_data: Option<&[u8]>,
        executable: bool,
        owner: &Pubkey,
    ) {
        let mut data = bincode::serialize(data).unwrap();
        if let Some(additional_data) = additional_data {
            data.extend_from_slice(additional_data);
        }
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

    #[test]
    fn test_source_upgradeable_bpf() {
        let bank = create_simple_test_bank(0);

        let program_id = Pubkey::new_unique();
        let program_data_address = get_program_data_address(&program_id);

        // Fail if the program account does not exist
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::AccountNotFound(..)
        );

        // Store the proper program account
        let proper_program_account_state = UpgradeableLoaderState::Program {
            programdata_address: program_data_address,
        };
        store_account(
            &bank,
            &program_id,
            &proper_program_account_state,
            None,
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        // Fail if the program data account does not exist
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::ProgramHasNoDataAccount(..)
        );

        // Store the proper program data account
        let proper_program_data_account_state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        };
        store_account(
            &bank,
            &program_data_address,
            &proper_program_data_account_state,
            Some(&[4u8; 200]),
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        // Success
        let source_upgradeable_bpf = SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap();

        let check_program_account_data = bincode::serialize(&proper_program_account_state).unwrap();
        let check_program_account_data_len = check_program_account_data.len();
        let check_program_lamports =
            bank.get_minimum_balance_for_rent_exemption(check_program_account_data_len);
        let check_program_account = AccountSharedData::from(Account {
            data: check_program_account_data,
            executable: true,
            lamports: check_program_lamports,
            owner: BPF_LOADER_UPGRADEABLE_ID,
            ..Account::default()
        });

        let mut check_program_data_account_data =
            bincode::serialize(&proper_program_data_account_state).unwrap();
        check_program_data_account_data.extend_from_slice(&[4u8; 200]);
        let check_program_data_account_data_len = check_program_data_account_data.len();
        let check_program_data_lamports =
            bank.get_minimum_balance_for_rent_exemption(check_program_data_account_data_len);
        let check_program_data_account = AccountSharedData::from(Account {
            data: check_program_data_account_data,
            executable: false,
            lamports: check_program_data_lamports,
            owner: BPF_LOADER_UPGRADEABLE_ID,
            ..Account::default()
        });

        assert_eq!(source_upgradeable_bpf.program_address, program_id);
        assert_eq!(
            source_upgradeable_bpf.program_account,
            check_program_account
        );
        assert_eq!(
            source_upgradeable_bpf.program_data_address,
            program_data_address
        );
        assert_eq!(
            source_upgradeable_bpf.program_data_account,
            check_program_data_account
        );
    }

    #[test]
    fn test_source_upgradeable_bpf_bad_program_account() {
        let bank = create_simple_test_bank(0);

        let program_id = Pubkey::new_unique();
        let program_data_address = get_program_data_address(&program_id);

        // Store the proper program data account
        store_account(
            &bank,
            &program_data_address,
            &UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            },
            Some(&[4u8; 200]),
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        // Fail if the program account is not owned by the upgradeable loader
        store_account(
            &bank,
            &program_id,
            &UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            },
            None,
            true,
            &Pubkey::new_unique(), // Not the upgradeable loader
        );
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::IncorrectOwner(..)
        );

        // Fail if the program account's state is not `UpgradeableLoaderState::Program`
        store_account(
            &bank,
            &program_id,
            &vec![0u8; 200],
            None,
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::BincodeError(..)
        );

        // Fail if the program account's state is `UpgradeableLoaderState::Program`,
        // but it points to the wrong data account
        store_account(
            &bank,
            &program_id,
            &UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(), // Not the correct data account
            },
            None,
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::InvalidProgramAccount(..)
        );
    }

    #[test]
    fn test_source_upgradeable_bpf_bad_program_data_account() {
        let bank = create_simple_test_bank(0);

        let program_id = Pubkey::new_unique();
        let program_data_address = get_program_data_address(&program_id);

        // Store the proper program account
        store_account(
            &bank,
            &program_id,
            &UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            },
            None,
            true,
            &BPF_LOADER_UPGRADEABLE_ID,
        );

        // Fail if the program data account is not owned by the upgradeable loader
        store_account(
            &bank,
            &program_data_address,
            &UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            },
            Some(&[4u8; 200]),
            false,
            &Pubkey::new_unique(), // Not the upgradeable loader
        );
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::IncorrectOwner(..)
        );

        // Fail if the program data account does not have the correct state
        store_account(
            &bank,
            &program_data_address,
            &vec![4u8; 200], // Not the correct state
            None,
            false,
            &BPF_LOADER_UPGRADEABLE_ID,
        );
        assert_matches!(
            SourceUpgradeableBpf::new_checked(&bank, &program_id).unwrap_err(),
            CoreBpfMigrationError::BincodeError(..)
        );
    }
}
