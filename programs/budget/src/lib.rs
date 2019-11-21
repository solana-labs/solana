pub mod budget_expr;
pub mod budget_instruction;
pub mod budget_processor;
pub mod budget_state;

use crate::budget_processor::process_instruction;

const BUDGET_PROGRAM_ID: [u8; 32] = [
    2, 203, 81, 223, 225, 24, 34, 35, 203, 214, 138, 130, 144, 208, 35, 77, 63, 16, 87, 51, 47,
    198, 115, 123, 98, 188, 19, 160, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    BUDGET_PROGRAM_ID,
    "Budget1111111111111111111111111111111111111",
    solana_budget_program,
    process_instruction
);
