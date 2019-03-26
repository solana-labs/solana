use solana_vote_api::vote_processor::process_instruction;

solana_sdk::solana_entrypoint!(process_instruction);
