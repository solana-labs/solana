#![feature(test)]

extern crate test;

use solana::entry::{next_entries, reconstruct_entries_from_blobs, EntrySlice};
use solana_sdk::hash::{hash, Hash};
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction;
use test::Bencher;

#[bench]
fn bench_block_to_blobs_to_block(bencher: &mut Bencher) {
    let zero = Hash::default();
    let one = hash(&zero.as_ref());
    let keypair = Keypair::new();
    let tx0 = system_transaction::transfer(&keypair, &keypair.pubkey(), 1, one);
    let transactions = vec![tx0; 10];
    let entries = next_entries(&zero, 1, transactions);

    bencher.iter(|| {
        let blobs = entries.to_blobs();
        assert_eq!(reconstruct_entries_from_blobs(blobs).unwrap().0, entries);
    });
}
