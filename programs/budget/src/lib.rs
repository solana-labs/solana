mod budget_processor;

use crate::budget_processor::process_instruction;

solana_sdk::process_instruction_entrypoint!(process_instruction);
