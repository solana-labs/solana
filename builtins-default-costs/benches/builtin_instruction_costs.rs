#![feature(test)]
extern crate test;
use {
    rand::Rng,
    solana_builtins_default_costs::get_builtin_instruction_cost,
    solana_sdk::{
        address_lookup_table, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        compute_budget, ed25519_program, feature_set::FeatureSet, loader_v4, pubkey::Pubkey,
        secp256k1_program,
    },
    test::Bencher,
};

struct BenchSetup {
    pubkeys: [Pubkey; 12],
    feature_set: FeatureSet,
}

const NUM_TRANSACTIONS_PER_ITER: usize = 1024;

fn setup(all_features_enabled: bool) -> BenchSetup {
    let pubkeys: [Pubkey; 12] = [
        solana_stake_program::id(),
        solana_config_program::id(),
        solana_vote_program::id(),
        solana_system_program::id(),
        compute_budget::id(),
        address_lookup_table::program::id(),
        bpf_loader_upgradeable::id(),
        bpf_loader_deprecated::id(),
        bpf_loader::id(),
        loader_v4::id(),
        secp256k1_program::id(),
        ed25519_program::id(),
    ];

    let feature_set = if all_features_enabled {
        FeatureSet::all_enabled()
    } else {
        FeatureSet::default()
    };

    BenchSetup {
        pubkeys,
        feature_set,
    }
}

fn do_hash_find(setup: &BenchSetup) {
    for _t in 0..NUM_TRANSACTIONS_PER_ITER {
        let idx = rand::thread_rng().gen_range(0..setup.pubkeys.len());
        get_builtin_instruction_cost(&setup.pubkeys[idx], &setup.feature_set);
    }
}

#[bench]
fn bench_hash_find_builtins_not_migrated(bencher: &mut Bencher) {
    let bench_setup = setup(false);

    bencher.iter(|| {
        do_hash_find(&bench_setup);
    });
}

#[bench]
fn bench_hash_find_builtins_migrated(bencher: &mut Bencher) {
    let bench_setup = setup(true);

    bencher.iter(|| {
        do_hash_find(&bench_setup);
    });
}
