use solana_sdk::{
    account::{Account, AccountSharedData},
    bpf_loader,
    bpf_loader_upgradeable::{self, get_program_data_address, UpgradeableLoaderState},
    pubkey::Pubkey,
    rent::Rent,
};

mod spl_memo_1_0 {
    solana_sdk::declare_id!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
}
mod spl_memo_3_0 {
    solana_sdk::declare_id!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
}
static SPL_PROGRAMS: &[(Pubkey, Pubkey, &[u8])] = &[
    (
        solana_inline_spl::token::ID,
        solana_sdk::bpf_loader::ID,
        include_bytes!("programs/spl_token-3.5.0.so"),
    ),
    (
        solana_inline_spl::token_2022::ID,
        solana_sdk::bpf_loader_upgradeable::ID,
        include_bytes!("programs/spl_token_2022-5.0.2.so"),
    ),
    (
        spl_memo_1_0::ID,
        solana_sdk::bpf_loader::ID,
        include_bytes!("programs/spl_memo-1.0.0.so"),
    ),
    (
        spl_memo_3_0::ID,
        solana_sdk::bpf_loader::ID,
        include_bytes!("programs/spl_memo-3.0.0.so"),
    ),
    (
        solana_inline_spl::associated_token_account::ID,
        solana_sdk::bpf_loader::ID,
        include_bytes!("programs/spl_associated_token_account-1.1.1.so"),
    ),
];

/// Returns a tuple `(Pubkey, Account)` for a BPF program, where the key is the
/// provided program ID and the account is a valid BPF Loader program account
/// containing the ELF.
fn bpf_loader_program_account(program_id: &Pubkey, elf: &[u8], rent: &Rent) -> (Pubkey, Account) {
    (
        *program_id,
        Account {
            lamports: rent.minimum_balance(elf.len()).max(1),
            data: elf.to_vec(),
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    )
}

/// Returns two tuples `(Pubkey, Account)` for a BPF upgradeable program.
/// The first tuple is the program account. It contains the provided program ID
/// and an account with a pointer to its program data account.
/// The second tuple is the program data account. It contains the program data
/// address and an account with the program data - a valid BPF Loader Upgradeable
/// program data account containing the ELF.
pub(crate) fn bpf_loader_upgradeable_program_accounts(
    program_id: &Pubkey,
    elf: &[u8],
    rent: &Rent,
) -> [(Pubkey, Account); 2] {
    let programdata_address = get_program_data_address(program_id);
    let program_account = {
        let space = UpgradeableLoaderState::size_of_program();
        let lamports = rent.minimum_balance(space);
        let data = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address,
        })
        .unwrap();
        Account {
            lamports,
            data,
            owner: bpf_loader_upgradeable::id(),
            executable: true,
            rent_epoch: 0,
        }
    };
    let programdata_account = {
        let space = UpgradeableLoaderState::size_of_programdata_metadata() + elf.len();
        let lamports = rent.minimum_balance(space);
        let mut data = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::default()),
        })
        .unwrap();
        data.extend_from_slice(elf);
        Account {
            lamports,
            data,
            owner: bpf_loader_upgradeable::id(),
            executable: false,
            rent_epoch: 0,
        }
    };
    [
        (*program_id, program_account),
        (programdata_address, programdata_account),
    ]
}

pub fn spl_programs(rent: &Rent) -> Vec<(Pubkey, AccountSharedData)> {
    SPL_PROGRAMS
        .iter()
        .flat_map(|(program_id, loader_id, elf)| {
            let mut accounts = vec![];
            if loader_id.eq(&solana_sdk::bpf_loader_upgradeable::ID) {
                for (key, account) in bpf_loader_upgradeable_program_accounts(program_id, elf, rent)
                {
                    accounts.push((key, AccountSharedData::from(account)));
                }
            } else {
                let (key, account) = bpf_loader_program_account(program_id, elf, rent);
                accounts.push((key, AccountSharedData::from(account)));
            }
            accounts
        })
        .collect()
}
