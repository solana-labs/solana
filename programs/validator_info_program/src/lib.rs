#[macro_export]
macro_rules! solana_validator_info_program {
    () => {
        (
            "solana_validator_info_program".to_string(),
            solana_validator_info_api::id(),
        )
    };
}

use solana_validator_info_api::validator_info_processor::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
