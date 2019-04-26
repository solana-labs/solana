pub mod token_processor;
mod token_state;

use solana_sdk::pubkey::Pubkey;

const TOKEN_PROGRAM_ID: [u8; 32] = [
    6, 221, 246, 225, 142, 57, 236, 63, 240, 189, 82, 112, 85, 219, 2, 165, 51, 122, 113, 201, 115,
    12, 217, 253, 72, 146, 220, 192, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&TOKEN_PROGRAM_ID)
}
