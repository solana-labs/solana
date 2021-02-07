safecoin_sdk::declare_builtin!(
    safecoin_sdk::bpf_loader_upgradeable::ID,
    safecoin_bpf_loader_upgradeable_program,
    safecoin_bpf_loader_program::process_instruction,
    upgradeable::id
);
