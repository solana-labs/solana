use solana_token_api::token_processor::process_instruction;

solana_sdk::process_instruction_entrypoint!(process_instruction);
