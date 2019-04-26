pub mod exchange_instruction;
pub mod exchange_processor;
pub mod exchange_state;

#[macro_use]
extern crate solana_metrics;

use solana_sdk::pubkey::Pubkey;

pub const EXCHANGE_PROGRAM_ID: [u8; 32] = [
    3, 147, 111, 103, 210, 47, 14, 213, 108, 116, 49, 115, 232, 171, 14, 111, 167, 140, 221, 234,
    33, 70, 185, 192, 42, 31, 141, 152, 0, 0, 0, 0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == EXCHANGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&EXCHANGE_PROGRAM_ID)
}
