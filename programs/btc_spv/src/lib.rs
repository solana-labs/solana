#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]
// This version is a work in progress and contains placeholders and incomplete components
pub mod header_store;
pub mod spv_instruction;
pub mod spv_processor;
pub mod spv_state;
pub mod utils;

use crate::spv_processor::process_instruction;

solana_sdk::declare_program!(
    "BtcSpv1111111111111111111111111111111111111",
    solana_btc_spv_program,
    process_instruction
);
