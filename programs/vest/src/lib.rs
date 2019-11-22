pub mod vest_instruction;
pub mod vest_processor;
pub mod vest_schedule;
pub mod vest_state;

use crate::vest_processor::process_instruction;

solana_sdk::declare_program!(
    "Vest111111111111111111111111111111111111111",
    solana_vest_program,
    process_instruction
);
