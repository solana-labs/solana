pub mod rewards_pools;
pub mod storage_contract;
pub mod storage_instruction;
pub mod storage_processor;

use crate::storage_processor::process_instruction;

solana_sdk::declare_program!(
    "Storage111111111111111111111111111111111111",
    solana_storage_program,
    process_instruction
);
