pub mod account_state;
pub mod data_store;
pub mod error_mappers;
pub mod processor;

use crate::processor::process_instruction;
use solana_sdk::move_loader::PROGRAM_ID;

solana_sdk::declare_program!(
    PROGRAM_ID,
    "MoveLdr111111111111111111111111111111111111",
    solana_move_loader_program,
    process_instruction
);
