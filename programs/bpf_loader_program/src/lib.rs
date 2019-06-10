#[macro_export]
macro_rules! solana_bpf_loader_program {
    () => {
        (
            "solana_bpf_loader_program".to_string(),
            solana_sdk::bpf_loader::id(),
        )
    };
}

use solana_bpf_loader_api::process_instruction;
solana_sdk::solana_entrypoint!(process_instruction);
