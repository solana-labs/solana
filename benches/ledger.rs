#![feature(test)]
extern crate solana;
extern crate test;

use solana::hash::{hash, Hash};
use solana::ledger::{next_entries, reconstruct_entries_from_blobs, Block};
use solana::packet::BlobRecycler;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
use solana::transaction::Transaction;
use test::Bencher;

#[bench]
fn bench_block_to_blobs_to_block(bencher: &mut Bencher) {
    let zero = Hash::default();
    let one = hash(&zero.as_ref());
    let keypair = Keypair::new();
    let tx0 = Transaction::system_move(&keypair, keypair.pubkey(), 1, one, 0);
    let transactions = vec![tx0; 10];
    let entries = next_entries(&zero, 1, transactions);

    let blob_recycler = BlobRecycler::default();
    bencher.iter(|| {
        let blobs = entries.to_blobs(&blob_recycler);
        assert_eq!(reconstruct_entries_from_blobs(blobs).unwrap(), entries);
    });
}
