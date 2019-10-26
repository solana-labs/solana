#[macro_use]
extern crate solana_ledger;

use solana_ledger::blocktree::{self, get_tmp_ledger_path, Blocktree};
use solana_ledger::entry;
use solana_sdk::hash::Hash;
use std::sync::Arc;
use std::thread::Builder;

#[test]
fn test_multiple_threads_insert_shred() {
    let blocktree_path = get_tmp_ledger_path!();
    let blocktree = Arc::new(Blocktree::open(&blocktree_path).unwrap());

    for _ in 0..100 {
        let num_threads = 10;

        // Create `num_threads` different ticks in slots 1..num_therads + 1, all
        // with parent = slot 0
        let threads: Vec<_> = (0..num_threads)
            .map(|i| {
                let entries = entry::create_ticks(1, Hash::default());
                let shreds = blocktree::entries_to_test_shreds(entries, i + 1, 0, false);
                let blocktree_ = blocktree.clone();
                Builder::new()
                    .name("blocktree-writer".to_string())
                    .spawn(move || {
                        blocktree_.insert_shreds(shreds, None).unwrap();
                    })
                    .unwrap()
            })
            .collect();

        for t in threads {
            t.join().unwrap()
        }

        // Check slot 0 has the correct children
        let mut meta0 = blocktree.meta(0).unwrap().unwrap();
        meta0.next_slots.sort();
        let expected_next_slots: Vec<_> = (1..num_threads + 1).collect();
        assert_eq!(meta0.next_slots, expected_next_slots);

        // Delete slots for next iteration
        blocktree.purge_slots(0, None);
    }

    // Cleanup
    drop(blocktree);
    Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
}
