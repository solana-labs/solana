#[macro_export]
macro_rules! solana_budget_program {
    () => {
        ("solana_budget_program".to_string(), solana_budget_api::id())
    };
}

use solana_budget_api::budget_processor::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
