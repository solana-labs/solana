safecoin_sdk::declare_builtin!(
    safecoin_sdk::bpf_loader::ID,
    safecoin_bpf_loader_program_with_jit,
    safecoin_bpf_loader_program::process_instruction_jit
);
