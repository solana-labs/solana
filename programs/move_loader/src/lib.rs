pub mod account_state;
pub mod data_store;
pub mod error_mappers;
pub mod processor;

use crate::processor::process_instruction;
use solana_sdk::move_loader;

solana_sdk::declare_program!(
    move_loader::BS58_STRING,
    solana_move_loader_program,
    process_instruction
);
