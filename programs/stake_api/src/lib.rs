pub mod stake_instruction;
pub mod stake_state;

use solana_sdk::pubkey::Pubkey;

const STAKE_PROGRAM_ID: [u8; 32] = [
    6, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178, 85, 127, 83, 92, 138,
    120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STAKE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STAKE_PROGRAM_ID)
}
