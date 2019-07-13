#[macro_export]
macro_rules! solana_move_loader_program {
    () => {
        (
            "solana_move_loader_program".to_string(),
            solana_move_loader_api::id(),
        )
    };
}

use solana_move_loader_api::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
