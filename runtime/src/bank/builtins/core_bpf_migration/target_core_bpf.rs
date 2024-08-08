use {
    super::error::CoreBpfMigrationError,
    crate::bank::Bank,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        bpf_loader_upgradeable::{self, get_program_data_address, UpgradeableLoaderState},
        pubkey::Pubkey,
    },
};

/// The account details of a Core BPF program slated to be upgraded.
#[derive(Debug)]
pub(crate) struct TargetCoreBpf {
    pub program_address: Pubkey,
    pub program_data_address: Pubkey,
    pub program_data_account: AccountSharedData,
    pub upgrade_authority_address: Option<Pubkey>,
}

impl TargetCoreBpf {
    /// Collects the details of a Core BPF program and verifies it is properly
    /// configured.
    /// The program account should exist with a pointer to its data account
    /// and it should be marked as executable.
    /// The program data account should exist with the correct state
    /// (a ProgramData header and the program ELF).
    pub(crate) fn new_checked(
        bank: &Bank,
        program_address: &Pubkey,
    ) -> Result<Self, CoreBpfMigrationError> {
        let program_data_address = get_program_data_address(program_address);

        // The program account should exist.
        let program_account = bank
            .get_account_with_fixed_root(program_address)
            .ok_or(CoreBpfMigrationError::AccountNotFound(*program_address))?;

        // The program account should be owned by the upgradeable loader.
        if program_account.owner() != &bpf_loader_upgradeable::id() {
            return Err(CoreBpfMigrationError::IncorrectOwner(*program_address));
        }

        // The program account should be executable.
        if !program_account.executable() {
            return Err(CoreBpfMigrationError::ProgramAccountNotExecutable(
                *program_address,
            ));
        }

        // The program account should have a pointer to its data account.
        match program_account.deserialize_data::<UpgradeableLoaderState>()? {
            UpgradeableLoaderState::Program {
                programdata_address,
            } if programdata_address == program_data_address => (),
            _ => {
                return Err(CoreBpfMigrationError::InvalidProgramAccount(
                    *program_address,
                ))
            }
        }

        // The program data account should exist.
        let program_data_account = bank
            .get_account_with_fixed_root(&program_data_address)
            .ok_or(CoreBpfMigrationError::ProgramHasNoDataAccount(
                *program_address,
            ))?;

        // The program data account should be owned by the upgradeable loader.
        if program_data_account.owner() != &bpf_loader_upgradeable::id() {
            return Err(CoreBpfMigrationError::IncorrectOwner(program_data_address));
        }

        // The program data account should have the correct state.
        let programdata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
        if program_data_account.data().len() >= programdata_metadata_size {
            if let UpgradeableLoaderState::ProgramData {
                upgrade_authority_address,
                ..
            } = bincode::deserialize(&program_data_account.data()[..programdata_metadata_size])?
            {
                return Ok(Self {
                    program_address: *program_address,
                    program_data_address,
                    program_data_account,
                    upgrade_authority_address,
                });
            }
        }
        Err(CoreBpfMigrationError::InvalidProgramDataAccount(
            program_data_address,
        ))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::create_simple_test_bank,
        assert_matches::assert_matches,
        solana_sdk::{account::WritableAccount, bpf_loader_upgradeable},
    };

    fn store_account(bank: &Bank, address: &Pubkey, data: &[u8], owner: &Pubkey, executable: bool) {
        let space = data.len();
        let lamports = bank.get_minimum_balance_for_rent_exemption(space);
        let mut account = AccountSharedData::new(lamports, space, owner);
        account.set_executable(executable);
        account.data_as_mut_slice().copy_from_slice(data);
        bank.store_account_and_update_capitalization(address, &account);
    }

    #[test]
    fn test_target_core_bpf() {
        let bank = create_simple_test_bank(0);

        let program_address = Pubkey::new_unique();
        let program_data_address = get_program_data_address(&program_address);

        // Fail if the program account does not exist.
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::AccountNotFound(..)
        );

        // Fail if the program account is not owned by the upgradeable loader.
        store_account(
            &bank,
            &program_address,
            &bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            })
            .unwrap(),
            &Pubkey::new_unique(), // Not the upgradeable loader
            true,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::IncorrectOwner(..)
        );

        // Fail if the program account is not executable.
        store_account(
            &bank,
            &program_address,
            &bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            })
            .unwrap(),
            &bpf_loader_upgradeable::id(),
            false, // Not executable
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::ProgramAccountNotExecutable(..)
        );

        // Fail if the program account does not have the correct state.
        store_account(
            &bank,
            &program_address,
            &[4u8; 200], // Not the correct state
            &bpf_loader_upgradeable::id(),
            true,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::BincodeError(..)
        );

        // Fail if the program account does not have the correct state.
        // This time, valid `UpgradeableLoaderState` but not a program account.
        store_account(
            &bank,
            &program_address,
            &bincode::serialize(&UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            })
            .unwrap(),
            &bpf_loader_upgradeable::id(),
            true,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::InvalidProgramAccount(..)
        );

        // Fail if the program account does not have the correct state.
        // This time, valid `UpgradeableLoaderState::Program` but it points to
        // the wrong program data account.
        store_account(
            &bank,
            &program_address,
            &bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(), // Not the correct program data account
            })
            .unwrap(),
            &bpf_loader_upgradeable::id(),
            true,
        );

        // Store the proper program account.
        store_account(
            &bank,
            &program_address,
            &bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            })
            .unwrap(),
            &bpf_loader_upgradeable::id(),
            true,
        );

        // Fail if the program data account does not exist.
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::ProgramHasNoDataAccount(..)
        );

        // Fail if the program data account is not owned by the upgradeable loader.
        store_account(
            &bank,
            &program_data_address,
            &bincode::serialize(&UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(Pubkey::new_unique()),
            })
            .unwrap(),
            &Pubkey::new_unique(), // Not the upgradeable loader
            false,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::IncorrectOwner(..)
        );

        // Fail if the program data account does not have the correct state.
        store_account(
            &bank,
            &program_data_address,
            &[4u8; 200], // Not the correct state
            &bpf_loader_upgradeable::id(),
            false,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::BincodeError(..)
        );

        // Fail if the program data account does not have the correct state.
        // This time, valid `UpgradeableLoaderState` but not a program data account.
        store_account(
            &bank,
            &program_data_address,
            &bincode::serialize(&UpgradeableLoaderState::Program {
                programdata_address: program_data_address,
            })
            .unwrap(),
            &bpf_loader_upgradeable::id(),
            false,
        );
        assert_matches!(
            TargetCoreBpf::new_checked(&bank, &program_address).unwrap_err(),
            CoreBpfMigrationError::InvalidProgramDataAccount(..)
        );

        // Success
        let elf = vec![4u8; 200];
        let test_success = |upgrade_authority_address: Option<Pubkey>| {
            // BPF Loader always writes ELF bytes after
            // `UpgradeableLoaderState::size_of_programdata_metadata()`.
            let programdata_metadata_size = UpgradeableLoaderState::size_of_programdata_metadata();
            let data_len = programdata_metadata_size + elf.len();
            let mut data = vec![0u8; data_len];
            bincode::serialize_into(
                &mut data[..programdata_metadata_size],
                &UpgradeableLoaderState::ProgramData {
                    slot: 0,
                    upgrade_authority_address,
                },
            )
            .unwrap();
            data[programdata_metadata_size..].copy_from_slice(&elf);

            store_account(
                &bank,
                &program_data_address,
                &data,
                &bpf_loader_upgradeable::id(),
                false,
            );

            let target_core_bpf = TargetCoreBpf::new_checked(&bank, &program_address).unwrap();

            assert_eq!(target_core_bpf.program_address, program_address);
            assert_eq!(target_core_bpf.program_data_address, program_data_address);
            assert_eq!(
                bincode::deserialize::<UpgradeableLoaderState>(
                    &target_core_bpf.program_data_account.data()[..programdata_metadata_size]
                )
                .unwrap(),
                UpgradeableLoaderState::ProgramData {
                    slot: 0,
                    upgrade_authority_address
                },
            );
            assert_eq!(
                &target_core_bpf.program_data_account.data()[programdata_metadata_size..],
                elf.as_slice()
            );
        };

        // With authority
        test_success(Some(Pubkey::new_unique()));

        // Without authority
        test_success(None);
    }
}
