pub mod token_processor;
mod token_state;

use solana_sdk::pubkey::Pubkey;

const TOKEN_PROGRAM_ID: [u8; 32] = [
    131, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&TOKEN_PROGRAM_ID)
}
