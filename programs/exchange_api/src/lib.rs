pub mod exchange_instruction;
pub mod exchange_processor;
pub mod exchange_state;

#[macro_use]
extern crate solana_metrics;

pub const EXCHANGE_PROGRAM_ID: [u8; 32] = [
    3, 147, 111, 103, 210, 47, 14, 213, 108, 116, 49, 115, 232, 171, 14, 111, 167, 140, 221, 234,
    33, 70, 185, 192, 42, 31, 141, 152, 0, 0, 0, 0,
];

solana_sdk::solana_program_id!(EXCHANGE_PROGRAM_ID);
