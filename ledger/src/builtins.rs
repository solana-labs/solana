use {
    solana_program_runtime::builtin_program::BuiltinProgram,
    solana_runtime::builtins::{BuiltinFeatureTransition, Builtins},
};

macro_rules! to_builtin {
    ($b:expr) => {
        BuiltinProgram {
            name: $b.0.to_string(),
            program_id: $b.1,
            process_instruction: $b.2,
        }
    };
}

/// Builtin programs that are always available
fn genesis_builtins() -> Vec<BuiltinProgram> {
    vec![
        to_builtin!(solana_bpf_loader_deprecated_program!()),
        to_builtin!(solana_bpf_loader_program!()),
        to_builtin!(solana_bpf_loader_upgradeable_program!()),
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
