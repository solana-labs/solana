use solana_exchange_api::exchange_processor::process_instruction;

solana_sdk::process_instruction_entrypoint!(process_instruction);
