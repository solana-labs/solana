#![feature(test)]
extern crate test;
use {
    rand::Rng,
    solana_builtins_default_costs::BUILTIN_INSTRUCTION_COSTS,
    solana_sdk::{
        address_lookup_table, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        compute_budget, ed25519_program, loader_v4, pubkey::Pubkey, secp256k1_program,
    },
    test::Bencher,
};

struct BenchSetup {
    pubkeys: [Pubkey; 12],
}

const NUM_TRANSACTIONS_PER_ITER: usize = 1024;
const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;

fn setup() -> BenchSetup {
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

    BenchSetup { pubkeys }
}

#[bench]
fn bench_hash_find(bencher: &mut Bencher) {
    let BenchSetup { pubkeys } = setup();

    bencher.iter(|| {
        for _t in 0..NUM_TRANSACTIONS_PER_ITER {
            let idx = rand::thread_rng().gen_range(0..pubkeys.len());
            let ix_execution_cost =
                if let Some(builtin_cost) = BUILTIN_INSTRUCTION_COSTS.get(&pubkeys[idx]) {
                    *builtin_cost
                } else {
                    u64::from(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
                };
            assert!(ix_execution_cost != DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64);
        }
    });
}
