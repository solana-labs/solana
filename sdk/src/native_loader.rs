use crate::account::Account;
use crate::pubkey::Pubkey;

const NATIVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 135, 132, 191, 20, 139, 164, 40, 47, 176, 18, 87, 72, 136, 169, 241, 83, 160, 125, 173, 247,
    101, 192, 69, 92, 154, 151, 3, 128, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&NATIVE_LOADER_PROGRAM_ID)
}

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == NATIVE_LOADER_PROGRAM_ID
}

/// Create an executable account with the given shared object name.
pub fn create_loadable_account(name: &str) -> Account {
    Account {
        lamports: 1,
        owner: id(),
        data: name.as_bytes().to_vec(),
        executable: true,
    }
}
