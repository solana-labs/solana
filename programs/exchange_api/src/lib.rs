pub mod exchange_instruction;
pub mod exchange_processor;
pub mod exchange_state;

use solana_sdk::pubkey::Pubkey;

pub const EXCHANGE_PROGRAM_ID: [u8; 32] = [
    134, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == EXCHANGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&EXCHANGE_PROGRAM_ID)
}
