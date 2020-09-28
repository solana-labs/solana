use solana_runtime::{
    bank::{Builtin, Builtins, Entrypoint},
    feature_set,
};
use solana_sdk::{genesis_config::ClusterType, pubkey::Pubkey};

/// Builtin programs that are always available
fn genesis_builtins(cluster_type: ClusterType) -> Vec<Builtin> {
    let builtins = if cluster_type != ClusterType::MainnetBeta {
        vec![
            solana_bpf_loader_deprecated_program!(),
            solana_bpf_loader_program!(),
        ]
    } else {
        // Remove this `else` block and the `cluster_type` argument to this function once
        // `feature_set::bpf_loader2_program::id()` is active on Mainnet Beta
        vec![solana_bpf_loader_deprecated_program!()]
    };

    builtins
        .into_iter()
        .map(|b| Builtin::new(&b.0, b.1, Entrypoint::Loader(b.2)))
        .collect()
}

/// Builtin programs activated dynamically by feature
fn feature_builtins() -> Vec<(Builtin, Pubkey)> {
    let builtins = vec![(
        solana_bpf_loader_program!(),
        feature_set::bpf_loader2_program::id(),
    )];

    builtins
        .into_iter()
        .map(|(b, p)| (Builtin::new(&b.0, b.1, Entrypoint::Loader(b.2)), p))
        .collect()
}

pub(crate) fn get(cluster_type: ClusterType) -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(cluster_type),
        feature_builtins: feature_builtins(),
    }
}
