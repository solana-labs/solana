#![allow(dead_code)] // Removed in later commit
use {
    super::error::CoreBpfMigrationError,
    crate::bank::Bank,
    solana_sdk::{
        account::Account,
        bpf_loader_upgradeable::{
            get_program_data_address, UpgradeableLoaderState, ID as BPF_LOADER_UPGRADEABLE_ID,
        },
        pubkey::Pubkey,
    },
};

/// Used to validate a source BPF Upgradeable program's account and data
/// account before migrating a built-in program to Core BPF.
#[derive(Debug)]
pub(crate) struct BpfUpgradeableConfig {
    pub program_address: Pubkey,
    pub program_account: Account,
    pub program_data_address: Pubkey,
    pub program_data_account: Account,
    pub total_data_size: usize,
}

impl BpfUpgradeableConfig {
    fn check_program_account(&self) -> Result<(), CoreBpfMigrationError> {
        // The program account should be owned by the upgradeable loader.
        if self.program_account.owner != BPF_LOADER_UPGRADEABLE_ID {
            return Err(CoreBpfMigrationError::IncorrectOwner(self.program_address));
        }

        // The program account should have a pointer to its data account.
        if let UpgradeableLoaderState::Program {
            programdata_address,
        } = bincode::deserialize(&self.program_account.data)
            .map_err::<CoreBpfMigrationError, _>(|_| {
                CoreBpfMigrationError::InvalidProgramAccount(self.program_address)
            })?
        {
            if programdata_address != self.program_data_address {
                return Err(CoreBpfMigrationError::InvalidProgramAccount(
                    self.program_address,
                ));
            }
        }

        Ok(())
    }

    fn check_program_data_account(&self) -> Result<(), CoreBpfMigrationError> {
        // The program data account should be owned by the upgradeable loader.
        if self.program_data_account.owner != BPF_LOADER_UPGRADEABLE_ID {
            return Err(CoreBpfMigrationError::IncorrectOwner(
                self.program_data_address,
            ));
        }

        // The program data account should have the correct state.
        let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
        if self.program_data_account.data.len() < programdata_data_offset {
            return Err(CoreBpfMigrationError::InvalidProgramDataAccount(
                self.program_data_address,
            ));
        }
        // Length checked in previous block.
        match bincode::deserialize::<UpgradeableLoaderState>(
            &self.program_data_account.data[..programdata_data_offset],
        ) {
            Ok(UpgradeableLoaderState::ProgramData { .. }) => Ok(()),
            _ => Err(CoreBpfMigrationError::InvalidProgramDataAccount(
                self.program_data_address,
            )),
        }
    }

    /// Create a new migration configuration for a BPF Upgradeable source
    /// program.
    pub(crate) fn new_checked(
        bank: &Bank,
        program_id: &Pubkey,
    ) -> Result<Self, CoreBpfMigrationError> {
        let program_address = *program_id;
        // The program account should exist.
        let program_account: Account = bank
            .get_account_with_fixed_root(&program_address)
            .ok_or(CoreBpfMigrationError::AccountNotFound(program_address))?
            .into();

        // The program data account should exist.
        let program_data_address = get_program_data_address(&program_address);
        let program_data_account: Account = bank
            .get_account_with_fixed_root(&program_data_address)
            .ok_or(CoreBpfMigrationError::ProgramHasNoDataAccount(
                program_address,
            ))?
            .into();

        // The total data size is the size of the program account's data plus
        // the size of the program data account's data.
        let total_data_size = program_account
            .data
            .len()
            .checked_add(program_data_account.data.len())
            .ok_or(CoreBpfMigrationError::ArithmeticOverflow)?;

        let config = Self {
            program_address,
            program_account,
            program_data_address,
            program_data_account,
            total_data_size,
        };

        config.check_program_account()?;
        config.check_program_data_account()?;

        Ok(config)
    }
}
