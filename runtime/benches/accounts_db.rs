#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    solana_runtime::{
        account_info::AccountInfo,
        accounts_db::AccountsDb,
        accounts_index::{AccountsIndexScanResult, UpsertReclaim},
    },
    solana_sdk::{account::AccountSharedData, pubkey},
    test::Bencher,
};

// 1k (old/new)
// test bench_scan ... bench:     130,526 ns/iter (+/- 3,402)
// test bench_scan ... bench:     128,999 ns/iter (+/- 3,811)
//
// 10k (old/new)
// test bench_scan ... bench:   1,417,961 ns/iter (+/- 32,285)
// test bench_scan ... bench:   1,393,143 ns/iter (+/- 36,120)
//
// 100k (old/new)
// test bench_scan ... bench:  16,982,083 ns/iter (+/- 168,999)
// test bench_scan ... bench:  16,877,242 ns/iter (+/- 231,343)
#[bench]
fn bench_scan(bencher: &mut Bencher) {
    let db = AccountsDb::new_single_for_tests();
    let empty_account = AccountSharedData::default();
    let count = 1_000;
    let pubkeys = (0..count).map(|_| pubkey::new_rand()).collect::<Vec<_>>();

    pubkeys.iter().for_each(|k| {
        for slot in 0..2 {
            // each upsert here (to a different slot) adds a refcount of 1 since entry is NOT cached
            db.accounts_index.upsert(
                slot,
                slot,
                k,
                &empty_account,
                &solana_runtime::accounts_index::AccountSecondaryIndexes::default(),
                AccountInfo::default(),
                &mut Vec::default(),
                UpsertReclaim::IgnoreReclaims,
            );
        }
    });

    const AVOID_CALLBACK_RESULT: u8 = AccountsIndexScanResult::Unknown as u8;
    bencher.iter(move || {
        db.accounts_index
            .scan::<_, _, false, AVOID_CALLBACK_RESULT>(
                pubkeys.iter(),
                |_k, _slot_refs, _entry| AccountsIndexScanResult::OnlyKeepInMemoryIfDirty,
            );
    });
}
