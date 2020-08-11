<<<<<<< HEAD
solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader_deprecated::ID,
    solana_bpf_loader_deprecated_program,
    solana_bpf_loader_program::process_instruction,
=======
use crate::process_instruction;

solana_sdk::declare_loader!(
    solana_sdk::bpf_loader_deprecated::ID,
    solana_bpf_loader_deprecated_program,
    process_instruction,
    solana_bpf_loader_program,
>>>>>>> 9290e561e... Align host addresses (#11384)
    deprecated::id
);
