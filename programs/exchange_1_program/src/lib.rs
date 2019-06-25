#[macro_export]
macro_rules! solana_exchange_program {
    () => {
        (
            "solana_exchange_program".to_string(),
            solana_exchange_api::id(),
        )
    };
}
use solana_exchange_api::exchange_processor::process_instruction;

solana_sdk::solana_entrypoint!(process_instruction);
