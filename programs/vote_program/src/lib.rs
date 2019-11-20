#[macro_export]
macro_rules! solana_vote_program {
    () => {
        ("solana_vote_program".to_string(), solana_vote_api::id())
    };
}

use solana_vote_api::vote_instruction::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
