pub mod stake_instruction;
pub mod stake_state;

use solana_sdk::pubkey::Pubkey;

const STAKE_PROGRAM_ID: [u8; 32] = [
    135, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STAKE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STAKE_PROGRAM_ID)
}
