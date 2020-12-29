use solana_runtime::{
    bank::{Builtin, Builtins},
    builtins::ActivationType,
};
use solana_sdk::{feature_set, pubkey::Pubkey};

macro_rules! to_builtin {
    ($b:expr) => {
        Builtin::new(&$b.0, $b.1, $b.2)
    };
}

/// Builtin programs that are always available
fn genesis_builtins() -> Vec<Builtin> {
    vec![
        to_builtin!(solana_bpf_loader_deprecated_program!()),
        to_builtin!(solana_bpf_loader_program!()),
    ]
}

/// Builtin programs activated dynamically by feature
fn feature_builtins() -> Vec<(Builtin, Pubkey, ActivationType)> {
    vec![(
        to_builtin!(solana_bpf_loader_upgradeable_program!()),
        feature_set::bpf_loader_upgradeable_program::id(),
        ActivationType::NewProgram,
    )]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_builtins: feature_builtins(),
    }
}
