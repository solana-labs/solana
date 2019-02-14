use crate::account::Account;
use crate::pubkey::Pubkey;

pub const NATIVE_LOADER_PROGRAM_ID: [u8; 32] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&NATIVE_LOADER_PROGRAM_ID)
}

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == NATIVE_LOADER_PROGRAM_ID
}

/// Create an executable account owned by the given program_id and shared object name.
pub fn create_program_account(program_id: Pubkey, name: &str) -> Account {
    Account {
        tokens: 1,
        owner: program_id,
        userdata: name.as_bytes().to_vec(),
        executable: true,
        loader: id(),
    }
}
