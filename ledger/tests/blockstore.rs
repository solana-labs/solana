use {
    solana_entry::entry,
    solana_ledger::{
        blockstore::{self, make_many_slot_entries, test_all_empty_or_min, Blockstore},
        get_tmp_ledger_path_auto_delete,
    },
    solana_sdk::hash::Hash,
    std::{sync::Arc, thread::Builder},
};

#[test]
fn test_multiple_threads_insert_shred() {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());

    for _ in 0..100 {
        let num_threads = 10;

        // Create `num_threads` different ticks in slots 1..num_threads + 1, all
        // with parent = slot 0
        let threads: Vec<_> = (0..num_threads)
            .map(|i| {
                let entries = entry::create_ticks(1, 0, Hash::default());
                let shreds = blockstore::entries_to_test_shreds(
                    &entries,
                    i + 1,
                    0,
                    false,
                    0,
                    true, // merkle_variant
                );
                let blockstore_ = blockstore.clone();
                Builder::new()
                    .name("blockstore-writer".to_string())
                    .spawn(move || {
                        blockstore_.insert_shreds(shreds, None, false).unwrap();
                    })
                    .unwrap()
            })
            .collect();

        for t in threads {
            t.join().unwrap()
        }

        // Check slot 0 has the correct children
        let mut meta0 = blockstore.meta(0).unwrap().unwrap();
        meta0.next_slots.sort_unstable();
        let expected_next_slots: Vec<_> = (1..num_threads + 1).collect();
        assert_eq!(meta0.next_slots, expected_next_slots);

        // Delete slots for next iteration
        blockstore.purge_and_compact_slots(0, num_threads + 1);
    }
}

#[test]
fn test_purge_huge() {
    let ledger_path = get_tmp_ledger_path_auto_delete!();
    let blockstore = Blockstore::open(ledger_path.path()).unwrap();

    let (shreds, _) = make_many_slot_entries(0, 5000, 10);
    blockstore.insert_shreds(shreds, None, false).unwrap();

    blockstore.purge_and_compact_slots(0, 4999);
    test_all_empty_or_min(&blockstore, 5000);
}
