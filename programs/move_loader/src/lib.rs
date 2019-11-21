pub mod account_state;
pub mod data_store;
pub mod error_mappers;
pub mod processor;

use solana_sdk::move_loader;
use crate::processor::process_instruction;

solana_sdk::declare_program!(
    move_loader::BS58_STRING,
    solana_move_loader_program,
    process_instruction
);
