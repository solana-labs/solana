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
