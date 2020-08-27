use crate::process_instruction;

solana_sdk::declare_loader!(
    solana_sdk::bpf_loader_deprecated::ID,
    solana_bpf_loader_deprecated_program,
    process_instruction,
    solana_bpf_loader_program,
    deprecated::id
);
