use {
    crate::{blockstore::*, blockstore_meta::SlotMeta},
    solana_sdk::clock::Slot,
};

pub struct NextSlotsIterator<'a> {
    pending_slots: Vec<Slot>,
    blockstore: &'a Blockstore,
}

impl<'a> NextSlotsIterator<'a> {
    pub fn new(start_slot: Slot, blockstore: &'a Blockstore) -> Self {
        Self {
            pending_slots: vec![start_slot],
            blockstore,
        }
    }
}

impl<'a> Iterator for NextSlotsIterator<'a> {
    type Item = (Slot, SlotMeta);
    fn next(&mut self) -> Option<Self::Item> {
        if self.pending_slots.is_empty() {
            None
        } else {
            let slot = self.pending_slots.pop().unwrap();
            if let Some(slot_meta) = self.blockstore.meta(slot).unwrap() {
                self.pending_slots.extend(slot_meta.next_slots.iter());
                Some((slot, slot_meta))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::blockstore_processor::fill_blockstore_slot_with_ticks,
        solana_sdk::hash::Hash, std::collections::HashSet,
    };

    #[test]
    fn test_next_slots_iterator() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        blockstore.set_roots(std::iter::once(&0)).unwrap();
        let ticks_per_slot = 5;
        /*
            Build a blockstore in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1  <-- set_root
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4

        */

        // Fork 1, ending at slot 3
        let last_entry_hash = Hash::default();
        let fork_point = 1;
        let mut fork_hash = Hash::default();
        for slot in 0..=3 {
            let parent = {
                if slot == 0 {
                    0
                } else {
                    slot - 1
                }
            };
            let last_entry_hash = fill_blockstore_slot_with_ticks(
                &blockstore,
                ticks_per_slot,
                slot,
                parent,
                last_entry_hash,
            );

            if slot == fork_point {
                fork_hash = last_entry_hash;
            }
        }

        // Fork 2, ending at slot 4
        let _ =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 4, fork_point, fork_hash);

        // Trying to get an iterator on any slot on the root fork should succeed
        let result: HashSet<_> = NextSlotsIterator::new(0, &blockstore)
            .map(|(slot, _)| slot)
            .collect();
        let expected = vec![0, 1, 2, 3, 4].into_iter().collect();
        assert_eq!(result, expected);

        let result: HashSet<_> = NextSlotsIterator::new(2, &blockstore)
            .map(|(slot, _)| slot)
            .collect();
        let expected = vec![2, 3].into_iter().collect();
        assert_eq!(result, expected);

        let result: HashSet<_> = NextSlotsIterator::new(4, &blockstore)
            .map(|(slot, _)| slot)
            .collect();
        let expected = vec![4].into_iter().collect();
        assert_eq!(result, expected);
    }
}
