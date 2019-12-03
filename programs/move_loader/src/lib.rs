pub mod account_state;
pub mod data_store;
pub mod error_mappers;
pub mod processor;

use crate::processor::process_instruction;

solana_sdk::declare_program!(
    solana_sdk::move_loader::ID,
    solana_move_loader_program,
    process_instruction
);
