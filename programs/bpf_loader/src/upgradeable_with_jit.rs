safecoin_sdk::declare_builtin!(
    safecoin_sdk::bpf_loader_upgradeable::ID,
    safecoin_bpf_loader_upgradeable_program_with_jit,
    safecoin_bpf_loader_program::process_instruction_jit,
    upgradeable_with_jit::id
);
