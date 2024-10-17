#![allow(clippy::arithmetic_side_effects)]
#![feature(test)]
extern crate solana_ledger;
extern crate test;

use {
    rand::Rng,
    solana_entry::entry::{create_ticks, Entry},
    solana_ledger::{
        blockstore::{entries_to_test_shreds, Blockstore},
        get_tmp_ledger_path_auto_delete,
    },
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey, signature::Signature},
    solana_transaction_status::TransactionStatusMeta,
    std::path::Path,
    test::Bencher,
};

// Given some shreds and a ledger at ledger_path, benchmark writing the shreds to the ledger
fn bench_write_shreds(bench: &mut Bencher, entries: Vec<Entry>, ledger_path: &Path) {
    let blockstore =
        Blockstore::open(ledger_path).expect("Expected to be able to open database ledger");
    bench.iter(move || {
        let shreds = entries_to_test_shreds(&entries, 0, 0, true, 0, /*merkle_variant:*/ true);
        blockstore.insert_shreds(shreds, None, false).unwrap();
    });
}

// Insert some shreds into the ledger in preparation for read benchmarks
fn setup_read_bench(
    blockstore: &Blockstore,
    num_small_shreds: u64,
    num_large_shreds: u64,
    slot: Slot,
) {
    // Make some big and small entries
    let entries = create_ticks(
        num_large_shreds * 4 + num_small_shreds * 2,
        0,
        Hash::default(),
    );

    // Convert the entries to shreds, write the shreds to the ledger
    let shreds = entries_to_test_shreds(
        &entries,
        slot,
        slot.saturating_sub(1), // parent_slot
        true,                   // is_full_slot
        0,                      // version
        true,                   // merkle_variant
    );
    blockstore
        .insert_shreds(shreds, None, false)
        .expect("Expectd successful insertion of shreds into ledger");
}

// Write small shreds to the ledger
#[bench]
#[ignore]
fn bench_write_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let num_entries = 32 * 1024;
    let entries = create_ticks(num_entries, 0, Hash::default());
    bench_write_shreds(bench, entries, ledger_path.path());
}

// Write big shreds to the ledger
#[bench]
#[ignore]
fn bench_write_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let num_entries = 32 * 1024;
    let entries = create_ticks(num_entries, 0, Hash::default());
    bench_write_shreds(bench, entries, ledger_path.path());
}

#[bench]
#[ignore]
fn bench_read_sequential(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");

    // Insert some big and small shreds into the ledger
    let num_small_shreds = 32 * 1024;
    let num_large_shreds = 32 * 1024;
    let total_shreds = num_small_shreds + num_large_shreds;
    let slot = 0;
    setup_read_bench(&blockstore, num_small_shreds, num_large_shreds, slot);

    let num_reads = total_shreds / 15;
    let mut rng = rand::thread_rng();
    bench.iter(move || {
        // Generate random starting point in the range [0, total_shreds - 1], read num_reads shreds sequentially
        let start_index = rng.gen_range(0..num_small_shreds + num_large_shreds);
        for i in start_index..start_index + num_reads {
            let _ = blockstore.get_data_shred(slot, i % total_shreds);
        }
    });
}

#[bench]
#[ignore]
fn bench_read_random(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");

    // Insert some big and small shreds into the ledger
    let num_small_shreds = 32 * 1024;
    let num_large_shreds = 32 * 1024;
    let total_shreds = num_small_shreds + num_large_shreds;
    let slot = 0;
    setup_read_bench(&blockstore, num_small_shreds, num_large_shreds, slot);

    let num_reads = total_shreds / 15;

    // Generate a num_reads sized random sample of indexes in range [0, total_shreds - 1],
    // simulating random reads
    let mut rng = rand::thread_rng();
    let indexes: Vec<usize> = (0..num_reads)
        .map(|_| rng.gen_range(0..total_shreds) as usize)
        .collect();
    bench.iter(move || {
        for i in indexes.iter() {
            let _ = blockstore.get_data_shred(slot, *i as u64);
        }
    });
}

#[bench]
#[ignore]
fn bench_insert_data_shred_small(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let num_entries = 32 * 1024;
    let entries = create_ticks(num_entries, 0, Hash::default());
    bench.iter(move || {
        let shreds = entries_to_test_shreds(&entries, 0, 0, true, 0, /*merkle_variant:*/ true);
        blockstore.insert_shreds(shreds, None, false).unwrap();
    });
}

#[bench]
#[ignore]
fn bench_insert_data_shred_big(bench: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let num_entries = 32 * 1024;
    let entries = create_ticks(num_entries, 0, Hash::default());
    bench.iter(move || {
        let shreds = entries_to_test_shreds(&entries, 0, 0, true, 0, /*merkle_variant:*/ true);
        blockstore.insert_shreds(shreds, None, false).unwrap();
    });
}

#[bench]
#[ignore]
fn bench_write_transaction_memos(b: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let signatures: Vec<Signature> = (0..64).map(|_| Signature::new_unique()).collect();
    b.iter(|| {
        for (slot, signature) in signatures.iter().enumerate() {
            blockstore
                .write_transaction_memos(
                    signature,
                    slot as u64,
                    "bench_write_transaction_memos".to_string(),
                )
                .unwrap();
        }
    });
}

#[bench]
#[ignore]
fn bench_add_transaction_memos_to_batch(b: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let signatures: Vec<Signature> = (0..64).map(|_| Signature::new_unique()).collect();
    b.iter(|| {
        let mut memos_batch = blockstore.get_write_batch().unwrap();

        for (slot, signature) in signatures.iter().enumerate() {
            blockstore
                .add_transaction_memos_to_batch(
                    signature,
                    slot as u64,
                    "bench_write_transaction_memos".to_string(),
                    &mut memos_batch,
                )
                .unwrap();
        }

        blockstore.write_batch(memos_batch).unwrap();
    });
}

#[bench]
#[ignore]
fn bench_write_transaction_status(b: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let signatures: Vec<Signature> = (0..64).map(|_| Signature::new_unique()).collect();
    let keys_with_writable: Vec<Vec<(Pubkey, bool)>> = (0..64)
        .map(|_| {
            vec![
                (Pubkey::new_unique(), true),
                (Pubkey::new_unique(), false),
                (Pubkey::new_unique(), true),
                (Pubkey::new_unique(), false),
            ]
        })
        .collect();
    let slot = 5;

    b.iter(|| {
        for (tx_idx, signature) in signatures.iter().enumerate() {
            blockstore
                .write_transaction_status(
                    slot,
                    *signature,
                    keys_with_writable[tx_idx].iter().map(|(k, v)| (k, *v)),
                    TransactionStatusMeta::default(),
                    tx_idx,
                )
                .unwrap();
        }
    });
}

#[bench]
#[ignore]
fn bench_add_transaction_status_to_batch(b: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore =
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger");
    let signatures: Vec<Signature> = (0..64).map(|_| Signature::new_unique()).collect();
    let keys_with_writable: Vec<Vec<(Pubkey, bool)>> = (0..64)
        .map(|_| {
            vec![
                (Pubkey::new_unique(), true),
                (Pubkey::new_unique(), false),
                (Pubkey::new_unique(), true),
                (Pubkey::new_unique(), false),
            ]
        })
        .collect();
    let slot = 5;

    b.iter(|| {
        let mut status_batch = blockstore.get_write_batch().unwrap();

        for (tx_idx, signature) in signatures.iter().enumerate() {
            blockstore
                .add_transaction_status_to_batch(
                    slot,
                    *signature,
                    keys_with_writable[tx_idx].iter().map(|(k, v)| (k, *v)),
                    TransactionStatusMeta::default(),
                    tx_idx,
                    &mut status_batch,
                )
                .unwrap();
        }

        blockstore.write_batch(status_batch).unwrap();
    });
}
