#![feature(test)]
extern crate test;

use solana_ledger::entry::{next_entry_mut, Entry, EntrySlice};
use solana_sdk::hash::{hash, Hash};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_transaction;
use test::Bencher;

const NUM_HASHES: u64 = 400;
const NUM_ENTRIES: usize = 800;

#[bench]
fn bench_poh_verify_ticks(bencher: &mut Bencher) {
    let zero = Hash::default();
    let mut cur_hash = hash(&zero.as_ref());

    let mut ticks: Vec<Entry> = Vec::with_capacity(NUM_ENTRIES);
    for _ in 0..NUM_ENTRIES {
        ticks.push(next_entry_mut(&mut cur_hash, NUM_HASHES, vec![]));
    }

    bencher.iter(|| {
        ticks.verify(&cur_hash);
    })
}

#[bench]
fn bench_poh_verify_transaction_entries(bencher: &mut Bencher) {
    let zero = Hash::default();
    let mut cur_hash = hash(&zero.as_ref());

    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();

    let mut ticks: Vec<Entry> = Vec::with_capacity(NUM_ENTRIES);
    for _ in 0..NUM_ENTRIES {
        let tx = system_transaction::transfer(&keypair1, &pubkey1, 42, cur_hash);
        ticks.push(next_entry_mut(&mut cur_hash, NUM_HASHES, vec![tx]));
    }

    bencher.iter(|| {
        ticks.verify(&cur_hash);
    })
}
