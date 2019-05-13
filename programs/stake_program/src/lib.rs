#[macro_export]
macro_rules! solana_stake_program {
    () => {
        ("solana_stake_program".to_string(), solana_stake_api::id())
    };
}

use solana_stake_api::stake_instruction::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
