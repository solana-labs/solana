#![feature(test)]

extern crate test;

use {
    rand::{thread_rng, Rng},
    solana_runtime::{
        accounts_db::AccountInfo,
        accounts_index::{
            AccountSecondaryIndexes, AccountsIndex, ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS,
        },
    },
    solana_sdk::pubkey::{self, Pubkey},
    test::Bencher,
};

#[bench]
fn bench_accounts_index(bencher: &mut Bencher) {
    const NUM_PUBKEYS: usize = 10_000;
    let pubkeys: Vec<_> = (0..NUM_PUBKEYS).map(|_| pubkey::new_rand()).collect();

    const NUM_FORKS: u64 = 16;

    let mut reclaims = vec![];
    let index = AccountsIndex::<AccountInfo>::new(Some(ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS));
    for f in 0..NUM_FORKS {
        for pubkey in pubkeys.iter().take(NUM_PUBKEYS) {
            index.upsert(
                f,
                pubkey,
                &Pubkey::default(),
                &[],
                &AccountSecondaryIndexes::default(),
                AccountInfo::default(),
                &mut reclaims,
                false,
            );
        }
    }

    let mut fork = NUM_FORKS;
    let mut root = 0;
    bencher.iter(|| {
        for _p in 0..NUM_PUBKEYS {
            let pubkey = thread_rng().gen_range(0, NUM_PUBKEYS);
            index.upsert(
                fork,
                &pubkeys[pubkey],
                &Pubkey::default(),
                &[],
                &AccountSecondaryIndexes::default(),
                AccountInfo::default(),
                &mut reclaims,
                false,
            );
            reclaims.clear();
        }
        index.add_root(root, false);
        root += 1;
        fork += 1;
    });
}
