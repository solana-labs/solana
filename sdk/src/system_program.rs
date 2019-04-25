use crate::pubkey::Pubkey;

const SYSTEM_PROGRAM_ID: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&SYSTEM_PROGRAM_ID)
}

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == SYSTEM_PROGRAM_ID
}
