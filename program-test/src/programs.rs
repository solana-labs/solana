use {
    bincode::serialize,
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        pubkey::Pubkey,
        rent::Rent,
    },
    thiserror::Error,
};

mod spl_token {
    solana_sdk::declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
}
mod spl_token_2022 {
    solana_sdk::declare_id!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
}
mod spl_memo_1_0 {
    solana_sdk::declare_id!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
}
mod spl_memo_3_0 {
    solana_sdk::declare_id!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
}
mod spl_associated_token_account {
    solana_sdk::declare_id!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
}

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramLoadError {
    #[error("Unknown program address: {0}")]
    UnknownAddress(Pubkey),
}

struct ProgramInfo {
    program_id: Pubkey,
    loader_id: Pubkey,
    elf: &'static [u8],
}

static SPL_PROGRAMS: &[ProgramInfo] = &[
    ProgramInfo {
        program_id: spl_token::ID,
        loader_id: bpf_loader::ID,
        elf: include_bytes!("programs/spl_token-3.5.0.so"),
    },
    ProgramInfo {
        program_id: spl_token_2022::ID,
        loader_id: bpf_loader_upgradeable::ID,
        elf: include_bytes!("programs/spl_token_2022-1.0.0.so"),
    },
    ProgramInfo {
        program_id: spl_memo_1_0::ID,
        loader_id: bpf_loader::ID,
        elf: include_bytes!("programs/spl_memo-1.0.0.so"),
    },
    ProgramInfo {
        program_id: spl_memo_3_0::ID,
        loader_id: bpf_loader::ID,
        elf: include_bytes!("programs/spl_memo-3.0.0.so"),
    },
    ProgramInfo {
        program_id: spl_associated_token_account::ID,
        loader_id: bpf_loader::ID,
        elf: include_bytes!("programs/spl_associated_token_account-1.1.1.so"),
    },
];

/// Describes one or more accounts that belong to the same program.
///
/// Different loaders use different account structure, so it could be useful to know which loader a
/// particular program uses.
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

pub type ProgramElf = Vec<u8>;

/// Returns the program ELF and the specified program account(s).
///
/// For loader v2, non-upgradable programs, `elf` data will be the same as the one stored in the
/// program account.  But for the loader v3 and loader 4, upgradable programs, the relationship is a
/// bit more complex and it could be convenient to retrieve the ELF bytes directly.
pub fn spl_program(
    rent: &Rent,
    target_address: Pubkey,
) -> Result<(ProgramElf, ProgramAccounts), ProgramLoadError> {
    let target = SPL_PROGRAMS
        .iter()
        .find(|ProgramInfo { program_id, .. }| *program_id == target_address);

    let target = target.ok_or(ProgramLoadError::UnknownAddress(target_address))?;

    Ok((target.elf.to_vec(), program_accounts(rent, target)))
}

/// Returns accounts for all known programs, paired with their addresses.
///
/// Depending on the loader, a program may use one or two accounts.
///
/// In particular, loader v3, upgradable programs, have two accounts: one for the program and one
/// for the program data.  In this case the program ID is the main account address.
pub fn spl_programs(rent: &Rent) -> Vec<(Pubkey, AccountSharedData)> {
    SPL_PROGRAMS
        .iter()
        .flat_map(|info| {
            let accounts = program_accounts(rent, info);
            Vec::<_>::from(accounts).into_iter()
        })
        .collect()
}

fn program_accounts(rent: &Rent, info: &ProgramInfo) -> ProgramAccounts {
    let ProgramInfo {
        program_id,
        loader_id,
        elf,
    } = info;

    match *loader_id {
        bpf_loader::ID => program_accounts_loader_v2(rent, *program_id, elf),
        bpf_loader_upgradeable::ID => program_accounts_loader_v3(rent, *program_id, elf),
        unexpected => panic!("Unexpected program loader: {unexpected}"),
    }
}

fn program_accounts_loader_v2(rent: &Rent, address: Pubkey, elf: &[u8]) -> ProgramAccounts {
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

fn program_accounts_loader_v3(rent: &Rent, program_address: Pubkey, elf: &[u8]) -> ProgramAccounts {
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
