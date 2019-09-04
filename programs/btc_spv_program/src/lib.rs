use solana_btc_spv_api::spv_processor::process_instruction;

#[macro_export]
macro_rules! solana_btc_spv_program {
    () => {
        (
            "solana_btc_spv_program".to_string(),
            solana_btc_spv_api::id(),
        )
    };
}

solana_sdk::solana_entrypoint!(process_instruction);
