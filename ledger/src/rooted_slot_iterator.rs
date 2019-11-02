use crate::{blocktree::*, blocktree_meta::SlotMeta};
use log::*;
use solana_sdk::clock::Slot;

pub struct RootedSlotIterator<'a> {
    next_slots: Vec<Slot>,
    blocktree: &'a Blocktree,
}

impl<'a> RootedSlotIterator<'a> {
    pub fn new(start_slot: Slot, blocktree: &'a Blocktree) -> Result<Self> {
        if blocktree.is_root(start_slot) {
            Ok(Self {
                next_slots: vec![start_slot],
                blocktree,
            })
        } else {
            Err(BlocktreeError::SlotNotRooted)
        }
    }
}
impl<'a> Iterator for RootedSlotIterator<'a> {
    type Item = (Slot, SlotMeta);

    fn next(&mut self) -> Option<Self::Item> {
        // Clone b/c passing the closure to the map below requires exclusive access to
        // `self`, which is borrowed here if we don't clone.
        let rooted_slot = self
            .next_slots
            .iter()
            .find(|x| self.blocktree.is_root(**x))
            .cloned();

        rooted_slot
            .map(|rooted_slot| {
                let slot_meta = self
                    .blocktree
                    .meta(rooted_slot)
                    .expect("Database failure, couldnt fetch SlotMeta");

                if slot_meta.is_none() {
                    warn!("Rooted SlotMeta was deleted in between checking is_root and fetch");
                }

                slot_meta.map(|slot_meta| {
                    self.next_slots = slot_meta.next_slots.clone();
                    (rooted_slot, slot_meta)
                })
            })
            .unwrap_or(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree_processor::fill_blocktree_slot_with_ticks;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_rooted_slot_iterator() {
        let blocktree_path = get_tmp_ledger_path("test_rooted_slot_iterator");
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        blocktree.set_roots(&[0]).unwrap();
        let ticks_per_slot = 5;
        /*
            Build a blocktree in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1  <-- set_root(true)
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
            let last_entry_hash = fill_blocktree_slot_with_ticks(
                &blocktree,
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
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 4, fork_point, fork_hash);

        // Set a root
        blocktree.set_roots(&[1, 2, 3]).unwrap();

        // Trying to get an iterator on a different fork will error
        assert!(RootedSlotIterator::new(4, &blocktree).is_err());

        // Trying to get an iterator on any slot on the root fork should succeed
        let result: Vec<_> = RootedSlotIterator::new(3, &blocktree)
            .unwrap()
            .into_iter()
            .map(|(slot, _)| slot)
            .collect();
        let expected = vec![3];
        assert_eq!(result, expected);

        let result: Vec<_> = RootedSlotIterator::new(0, &blocktree)
            .unwrap()
            .into_iter()
            .map(|(slot, _)| slot)
            .collect();
        let expected = vec![0, 1, 2, 3];
        assert_eq!(result, expected);

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
}
