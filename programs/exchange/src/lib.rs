pub mod exchange_instruction;
pub mod exchange_processor;
pub mod exchange_state;

#[macro_use]
extern crate solana_metrics;

use crate::exchange_processor::process_instruction;

pub const EXCHANGE_PROGRAM_ID: [u8; 32] = [
    3, 147, 111, 103, 210, 47, 14, 213, 108, 116, 49, 115, 232, 171, 14, 111, 167, 140, 221, 234,
    33, 70, 185, 192, 42, 31, 141, 152, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    EXCHANGE_PROGRAM_ID,
    "Exchange11111111111111111111111111111111111",
    solana_exchange_program,
    process_instruction
);

pub mod faucet {
    pub const EXCHANGE_FAUCET_ID: [u8; 32] = [
        3, 147, 111, 103, 210, 47, 23, 11, 176, 29, 147, 89, 237, 155, 21, 62, 107, 105, 157, 1,
        98, 204, 206, 211, 54, 212, 79, 15, 160, 0, 0, 0,
    ];

    solana_sdk::solana_name_id!(
        EXCHANGE_FAUCET_ID,
        "ExchangeFaucet11111111111111111111111111111"
    );
}
