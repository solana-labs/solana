#[macro_export]
macro_rules! solana_storage_program {
    () => {
        (
            "solana_storage_program".to_string(),
            solana_storage_api::id(),
        )
    };
}

use solana_storage_api::storage_processor::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
