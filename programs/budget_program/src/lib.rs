use solana_budget_api::budget_processor::process_instruction;

solana_sdk::process_instruction_entrypoint!(process_instruction);
