use crate::blockstore::*;
use solana_sdk::clock::Slot;

pub struct AncestorIterator<'a> {
    current: Option<Slot>,
    blockstore: &'a Blockstore,
}

impl<'a> AncestorIterator<'a> {
    pub fn new(start_slot: Slot, blockstore: &'a Blockstore) -> Self {
        let current = blockstore.meta(start_slot).unwrap().and_then(|slot_meta| {
            if slot_meta.is_parent_set() && start_slot != 0 {
                Some(slot_meta.parent_slot)
            } else {
                None
            }
        });
        Self {
            current,
            blockstore,
        }
    }

    pub fn new_inclusive(start_slot: Slot, blockstore: &'a Blockstore) -> Self {
        Self {
            current: blockstore.meta(start_slot).unwrap().map(|_| start_slot),
            blockstore,
        }
    }
}
impl<'a> Iterator for AncestorIterator<'a> {
    type Item = Slot;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        current.map(|slot| {
            if slot != 0 {
                self.current = self.blockstore.meta(slot).unwrap().and_then(|slot_meta| {
                    if slot_meta.is_parent_set() {
                        Some(slot_meta.parent_slot)
                    } else {
                        None
                    }
                });
            } else {
                self.current = None;
            }
            slot
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockstore_processor::fill_blockstore_slot_with_ticks;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_ancestor_iterator() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        blockstore.set_roots(&[0]).unwrap();
        let ticks_per_slot = 5;

        /*
            Build a blockstore in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1
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

        // Test correctness
        assert!(AncestorIterator::new(0, &blockstore).next().is_none());
        assert_eq!(
            AncestorIterator::new(4, &blockstore).collect::<Vec<Slot>>(),
            vec![1, 0]
        );
        assert_eq!(
            AncestorIterator::new(3, &blockstore).collect::<Vec<Slot>>(),
            vec![2, 1, 0]
        );
    }

    #[test]
    fn test_ancestor_iterator_inclusive() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();

        let (shreds, _) = make_slot_entries(0, 0, 42);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        let (shreds, _) = make_slot_entries(1, 0, 42);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        let (shreds, _) = make_slot_entries(2, 1, 42);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        assert_eq!(
            AncestorIterator::new(2, &blockstore).collect::<Vec<Slot>>(),
            vec![1, 0]
        );
        // existing start_slot
        assert_eq!(
            AncestorIterator::new_inclusive(2, &blockstore).collect::<Vec<Slot>>(),
            vec![2, 1, 0]
        );

        // non-existing start_slot
        assert_eq!(
            AncestorIterator::new_inclusive(3, &blockstore).collect::<Vec<Slot>>(),
            vec![] as Vec<Slot>
        );
    }
}
