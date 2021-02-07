pub mod budget_expr;
pub mod budget_instruction;
pub mod budget_processor;
pub mod budget_state;

use crate::budget_processor::process_instruction;

safecoin_sdk::declare_program!(
    "Budget1111111111111111111111111111111111111",
    safecoin_budget_program,
    process_instruction
);
