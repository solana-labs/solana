safecoin_sdk::declare_builtin!(
    safecoin_sdk::bpf_loader_deprecated::ID,
    safecoin_bpf_loader_deprecated_program,
    safecoin_bpf_loader_program::process_instruction,
    deprecated::id
);
