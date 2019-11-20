#[macro_export]
macro_rules! solana_vest_program {
    () => {
        ("solana_vest_program".to_string(), solana_vest_api::id())
    };
}

use solana_vest_api::vest_processor::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
