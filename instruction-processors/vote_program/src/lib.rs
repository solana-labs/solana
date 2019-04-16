use solana_vote_api::vote_instruction::process_instruction;

solana_sdk::solana_entrypoint!(process_instruction);
