#[macro_export]
macro_rules! solana_token_program {
    () => {
        ("solana_token_program".to_string(), solana_token_api::id())
    };
}

use solana_token_api::token_processor::process_instruction;

solana_sdk::solana_entrypoint!(process_instruction);
