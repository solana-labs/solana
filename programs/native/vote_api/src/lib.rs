pub mod vote_instruction;
pub mod vote_state;
pub mod vote_transaction;

use solana_sdk::pubkey::Pubkey;

pub const VOTE_PROGRAM_ID: [u8; 32] = [
    132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == VOTE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&VOTE_PROGRAM_ID)
}
