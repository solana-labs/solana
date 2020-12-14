use solana_runtime::{
    bank::{Builtin, Builtins},
    builtins::ActivationType,
};
use solana_sdk::{feature_set, genesis_config::ClusterType, pubkey::Pubkey};

/// Builtin programs that are always available
fn genesis_builtins(cluster_type: ClusterType) -> Vec<Builtin> {
    let builtins = if cluster_type != ClusterType::MainnetBeta {
        vec![
<<<<<<< HEAD
            solana_bpf_loader_deprecated_program!(),
            solana_bpf_loader_program!(),
=======
            to_builtin!(solana_bpf_loader_deprecated_program!()),
            if bpf_jit {
                to_builtin!(solana_bpf_loader_program_with_jit!())
            } else {
                to_builtin!(solana_bpf_loader_program!())
            },
            if bpf_jit {
                to_builtin!(solana_bpf_loader_upgradeable_program_with_jit!())
            } else {
                to_builtin!(solana_bpf_loader_upgradeable_program!())
            },
>>>>>>> 9e9039458... Upgradeable loader (#13689)
        ]
    } else {
        // Remove this `else` block and the `cluster_type` argument to this function once
        // `feature_set::bpf_loader2_program::id()` is active on Mainnet Beta
        vec![solana_bpf_loader_deprecated_program!()]
    };

    builtins
        .into_iter()
        .map(|b| Builtin::new(&b.0, b.1, b.2))
        .collect()
}

/// Builtin programs activated dynamically by feature
fn feature_builtins() -> Vec<(Builtin, Pubkey, ActivationType)> {
<<<<<<< HEAD
    let builtins = vec![(
        solana_bpf_loader_program!(),
        feature_set::bpf_loader2_program::id(),
        ActivationType::NewProgram,
    )];

    builtins
        .into_iter()
        .map(|(b, p, t)| (Builtin::new(&b.0, b.1, b.2), p, t))
        .collect()
=======
    vec![
        (
            to_builtin!(solana_bpf_loader_program!()),
            feature_set::bpf_loader2_program::id(),
            ActivationType::NewProgram,
        ),
        (
            to_builtin!(solana_bpf_loader_upgradeable_program!()),
            feature_set::bpf_loader_upgradeable_program::id(),
            ActivationType::NewProgram,
        ),
    ]
>>>>>>> 9e9039458... Upgradeable loader (#13689)
}

pub(crate) fn get(cluster_type: ClusterType) -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(cluster_type),
        feature_builtins: feature_builtins(),
    }
}
