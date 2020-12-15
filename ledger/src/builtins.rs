use solana_runtime::{
    bank::{Builtin, Builtins},
    builtins::ActivationType,
};
use solana_sdk::{feature_set, genesis_config::ClusterType, pubkey::Pubkey};

macro_rules! to_builtin {
    ($b:expr) => {
        Builtin::new(&$b.0, $b.1, $b.2)
    };
}

/// Builtin programs that are always available
fn genesis_builtins(cluster_type: ClusterType, bpf_jit: bool) -> Vec<Builtin> {
    let mut builtins = vec![
        to_builtin!(solana_bpf_loader_deprecated_program!()),
        if bpf_jit {
            to_builtin!(solana_bpf_loader_program_with_jit!())
        } else {
            to_builtin!(solana_bpf_loader_program!())
        },
    ];
    if cluster_type != ClusterType::MainnetBeta {
        builtins.push(if bpf_jit {
            to_builtin!(solana_bpf_loader_upgradeable_program_with_jit!())
        } else {
            to_builtin!(solana_bpf_loader_upgradeable_program!())
        });
    }
    builtins
}

/// Builtin programs activated dynamically by feature
fn feature_builtins() -> Vec<(Builtin, Pubkey, ActivationType)> {
    vec![(
        to_builtin!(solana_bpf_loader_upgradeable_program!()),
        feature_set::bpf_loader_upgradeable_program::id(),
        ActivationType::NewProgram,
    )]
}

pub(crate) fn get(cluster_type: ClusterType, bpf_jit: bool) -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(cluster_type, bpf_jit),
        feature_builtins: feature_builtins(),
    }
}
