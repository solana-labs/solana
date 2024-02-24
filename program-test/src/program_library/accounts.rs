//! Constructs accounts data for a program for different known loaders.

use {
    bincode::serialize,
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        pubkey::Pubkey,
        rent::Rent,
    },
};

/// Describes one or more accounts that belong to the same program.
///
/// Different loaders use different account structure, so it could be useful to know which loader a
/// particular program uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramAccounts {
    /// [`bpf_loader`] aka loader v2.  Non-upgradable programs use a single account.  This account
    /// will just directly hold ELF of the program.
    V2 {
        address: Pubkey,
        account: AccountSharedData,
    },

    /// [`bpf_loader_upgradeable`] aka loader v3.  Upgradeable programs, use two accounts.
    ///
    /// ELF data is stored in the `program_data_account`, but it is prefixed by an additional
    /// header.  See [`UpgradeableLoaderState::ProgramData`].
    V3 {
        program_address: Pubkey,
        program_account: AccountSharedData,
        program_data_address: Pubkey,
        program_data_account: AccountSharedData,
    },
}

impl From<ProgramAccounts> for Vec<(Pubkey, AccountSharedData)> {
    fn from(accounts: ProgramAccounts) -> Self {
        match accounts {
            ProgramAccounts::V2 { address, account } => vec![(address, account)],
            ProgramAccounts::V3 {
                program_address,
                program_account,
                program_data_address,
                program_data_account,
            } => vec![
                (program_address, program_account),
                (program_data_address, program_data_account),
            ],
        }
    }
}

impl ProgramAccounts {
    pub fn loader_v2(rent: &Rent, address: Pubkey, elf: &[u8]) -> ProgramAccounts {
        let data = elf.to_vec();

        let account = AccountSharedData::from(Account {
            lamports: rent.minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::ID,
            executable: true,
            rent_epoch: 0,
        });

        ProgramAccounts::V2 { address, account }
    }

    pub fn loader_v3(rent: &Rent, program_address: Pubkey, elf: &[u8]) -> ProgramAccounts {
        const LOADER_ID: &Pubkey = &bpf_loader_upgradeable::ID;

        let (program_data_address, _) =
            Pubkey::find_program_address(&[program_address.as_ref()], LOADER_ID);

        let mut program_data = serialize(&UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::default()),
        })
        .expect("`UpgradeableLoaderState::ProgramData` always serializes correctly");
        program_data.extend_from_slice(elf);

        let program_account_data = serialize(&UpgradeableLoaderState::Program {
            programdata_address: program_data_address,
        })
        .expect("`UpgradeableLoaderState::Program` always serializes correctly");

        let program_data_account = AccountSharedData::from(Account {
            lamports: rent.minimum_balance(program_data.len()).max(1),
            data: program_data,
            owner: *LOADER_ID,
            executable: false,
            rent_epoch: 0,
        });

        let program_account = AccountSharedData::from(Account {
            lamports: rent.minimum_balance(program_account_data.len()).max(1),
            data: program_account_data,
            owner: *LOADER_ID,
            executable: true,
            rent_epoch: 0,
        });

        ProgramAccounts::V3 {
            program_address,
            program_account,
            program_data_address,
            program_data_account,
        }
    }
}
