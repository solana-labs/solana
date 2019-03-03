pub mod rewards_instruction;
pub mod rewards_state;
pub mod rewards_transaction;

use solana_sdk::pubkey::Pubkey;

const REWARDS_PROGRAM_ID: [u8; 32] = [
    133, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == REWARDS_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&REWARDS_PROGRAM_ID)
}
