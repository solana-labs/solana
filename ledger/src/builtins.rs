use {
    solana_program_runtime::builtin_program::BuiltinProgram,
    solana_runtime::builtins::{BuiltinFeatureTransition, Builtins},
    solana_sdk::{bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable},
};

/// Builtin programs that are always available
fn genesis_builtins() -> Vec<BuiltinProgram> {
    vec![
        BuiltinProgram {
            name: "solana_bpf_loader_deprecated_program".to_string(),
            program_id: bpf_loader_deprecated::id(),
            process_instruction: solana_bpf_loader_program::process_instruction,
        },
        BuiltinProgram {
            name: "solana_bpf_loader_program".to_string(),
            program_id: bpf_loader::id(),
            process_instruction: solana_bpf_loader_program::process_instruction,
        },
        BuiltinProgram {
            name: "solana_bpf_loader_upgradeable_program".to_string(),
            program_id: bpf_loader_upgradeable::id(),
            process_instruction: solana_bpf_loader_program::process_instruction,
        },
    ]
}

/// Dynamic feature transitions for builtin programs
fn builtin_feature_transitions() -> Vec<BuiltinFeatureTransition> {
    vec![]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_transitions: builtin_feature_transitions(),
    }
}
