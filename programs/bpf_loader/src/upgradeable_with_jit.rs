solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader_upgradeable::ID,
    solana_bpf_loader_upgradeable_program_with_jit,
    solana_bpf_loader_program::process_instruction_jit,
    upgradeable_with_jit::id
);
