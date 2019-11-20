#[macro_export]
macro_rules! solana_config_program {
    () => {
        ("solana_config_program".to_string(), solana_config_api::id())
    };
}
use solana_config_api::config_processor::process_instruction;

solana_sdk::solana_entrypoint!(process_instruction);
