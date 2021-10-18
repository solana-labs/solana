//! This modules provides an iterator API to access an entire slot of shreds,
//! regardless of whether the shreds are in the cache, on the fs or a mix of both.

use {
    super::*,
    crate::blockstore::blockstore_shreds::{ShredFileData, ShredSlotCache},
    crate::shred::SHRED_PAYLOAD_SIZE,
    std::{collections::btree_map, iter::Peekable, sync::RwLockReadGuard},
};

// Iterator mode - set at iterator creation and updated as iterator progresses
pub(crate) enum SlotShredIteratorMode {
    CacheOnly,
    FsOnly,
    CacheAndFs,
    Exhausted,
}

pub(crate) struct SlotIteratorFsData<'a> {
    index_iter: Peekable<btree_map::Iter<'a, u32, u32>>,
    data_buffer: &'a [u8],
}

pub(crate) struct SlotIterator<'a> {
    _slot: Slot,
    cache_iter: Option<Peekable<btree_map::Iter<'a, u64, Vec<u8>>>>,
    file_data: Option<SlotIteratorFsData<'a>>,
    mode: SlotShredIteratorMode,
}

impl<'a> SlotIterator<'a> {
    pub fn setup(
        cache: &'a Option<Arc<RwLock<ShredSlotCache>>>,
        path: &Path,
    ) -> (
        Option<RwLockReadGuard<'a, ShredSlotCache>>,
        Option<ShredFileData>,
    ) {
        let cache_guard = cache.as_ref().map(|cache| cache.read().unwrap());

        let file_data = match fs::File::open(path) {
            Ok(mut file) => Some(Blockstore::read_shred_file(&mut file).unwrap_or_else(|_| {
                panic!("failed to read {}, possibly corrupted", path.display())
            })),
            Err(_err) => None,
        };

        (cache_guard, file_data)
    }

    pub fn new(
        slot: Slot,
        start_index: u64,
        cache_guard: &'a Option<RwLockReadGuard<ShredSlotCache>>,
        file_data: &'a Option<ShredFileData>,
    ) -> Self {
        let cache_iter = if cache_guard.is_some() {
            let mut iter = cache_guard.as_ref().unwrap().iter().peekable();
            // Advance the iterator so first .next() will be >= start_index
            while iter.next_if(|&(idx, _)| idx < &start_index).is_some() {}
            Some(iter)
        } else {
            None
        };

        let file_data = file_data.as_ref().map(|file_data| {
            let mut index_iter = file_data.index.iter().peekable();
            // Advance the iterator so first .next() will be >= start_index
            while index_iter
                .next_if(|&(idx, _)| (*idx as u64) < start_index)
                .is_some()
            {}

            SlotIteratorFsData {
                index_iter,
                data_buffer: &file_data.data,
            }
        });

        let mode = if cache_iter.is_some() {
            if file_data.is_some() {
                SlotShredIteratorMode::CacheAndFs
            } else {
                SlotShredIteratorMode::CacheOnly
            }
        } else if file_data.is_some() {
            SlotShredIteratorMode::FsOnly
        } else {
            SlotShredIteratorMode::Exhausted
        };

        SlotIterator {
            _slot: slot,
            cache_iter,
            file_data,
            mode,
        }
    }

    // Consume the next element from the cache, or update the iterator's mode tostate_if_exhausted
    fn consume_next_cache(
        &mut self,
        state_if_exhausted: SlotShredIteratorMode,
    ) -> Option<(u64, Vec<u8>)> {
        self.cache_iter
            .as_mut()
            .unwrap()
            .next()
            .map(|(idx, shred)| (*idx, shred.clone()))
            .or_else(|| {
                self.mode = state_if_exhausted;
                None
            })
    }

    // Consume the next element from the fs, or update the iterator's mode tostate_if_exhausted
    fn consume_next_fs(
        &mut self,
        state_if_exhausted: SlotShredIteratorMode,
    ) -> Option<(u64, Vec<u8>)> {
        let file_data_ref = self.file_data.as_mut().unwrap();

        file_data_ref
            .index_iter
            .next()
            .map(|(idx, offset)| {
                let mut buffer = vec![0; SHRED_PAYLOAD_SIZE];
                let offset = *offset as usize;
                buffer.copy_from_slice(
                    &file_data_ref.data_buffer[offset..offset + SHRED_PAYLOAD_SIZE],
                );
                (*idx as u64, buffer)
            })
            .or_else(|| {
                self.mode = state_if_exhausted;
                None
            })
    }
}

impl Iterator for SlotIterator<'_> {
    type Item = (u64, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self.mode {
            SlotShredIteratorMode::CacheOnly => {
                self.consume_next_cache(SlotShredIteratorMode::Exhausted)
            }
            SlotShredIteratorMode::FsOnly => self.consume_next_fs(SlotShredIteratorMode::Exhausted),
            SlotShredIteratorMode::CacheAndFs => {
                let next_cache = self.cache_iter.as_mut().unwrap().peek();
                let next_fs = self.file_data.as_mut().unwrap().index_iter.peek();

                match (next_cache, next_fs) {
                    (Some(next_cache), Some(next_fs)) => {
                        if *next_cache.0 < *next_fs.0 as u64 {
                            self.consume_next_cache(SlotShredIteratorMode::FsOnly)
                        } else {
                            self.consume_next_fs(SlotShredIteratorMode::CacheOnly)
                        }
                    }
                    (Some(_next_cache), None) => {
                        // We now know fs is exhausted so update to cache only
                        self.mode = SlotShredIteratorMode::CacheOnly;
                        self.consume_next_cache(SlotShredIteratorMode::Exhausted)
                    }
                    (None, Some(_next_fs)) => {
                        // We now know cache is exhausted so update to fs only
                        self.mode = SlotShredIteratorMode::FsOnly;
                        self.consume_next_fs(SlotShredIteratorMode::Exhausted)
                    }
                    (None, None) => {
                        // The only case when this can happen is when the start_index supplied
                        // at creation is larger than all indexes in both cache and on fs
                        self.mode = SlotShredIteratorMode::Exhausted;
                        None
                    }
                }
            }
            SlotShredIteratorMode::Exhausted => None,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use {
        crate::{get_tmp_ledger_path_auto_delete, shred::max_ticks_per_n_shreds},
        itertools::Itertools,
    };

    fn verify_iterator(blockstore: &Blockstore, slot: Slot, shreds: &[Shred]) {
        // Ensure iterator with start_index = 0 yields all inserted elements
        {
            let cache = blockstore.data_shred_slot_cache(slot);
            let (cache_guard, file_data) =
                SlotIterator::setup(&cache, &blockstore.data_shred_slot_path(slot));
            let shred_iter = SlotIterator::new(slot, 0, &cache_guard, &file_data);
            for (original_shred, (_, iter_shred)) in shreds.iter().zip_eq(shred_iter) {
                assert_eq!(original_shred.payload, iter_shred);
            }
        }

        // Ensure iterator with non-zero start index yields expected elements
        {
            let start_index = 5;
            let cache = blockstore.data_shred_slot_cache(slot);
            let (cache_guard, file_data) =
                SlotIterator::setup(&cache, &blockstore.data_shred_slot_path(slot));
            let shred_iter = SlotIterator::new(
                slot,
                start_index.try_into().unwrap(),
                &cache_guard,
                &file_data,
            );
            for (original_shred, (_, iter_shred)) in
                shreds.iter().skip(start_index).zip_eq(shred_iter)
            {
                assert_eq!(original_shred.payload, iter_shred);
            }
        }

        // Ensure iterator with start_index > all elements yields no elements
        {
            let start_index = 10;
            let cache = blockstore.data_shred_slot_cache(slot);
            let (cache_guard, file_data) =
                SlotIterator::setup(&cache, &blockstore.data_shred_slot_path(slot));
            let shred_iter = SlotIterator::new(
                slot,
                start_index.try_into().unwrap(),
                &cache_guard,
                &file_data,
            );
            for (original_shred, (_, iter_shred)) in
                shreds.iter().skip(start_index).zip_eq(shred_iter)
            {
                assert_eq!(original_shred.payload, iter_shred);
            }
        }
    }

    #[test]
    fn test_slot_shred_iterator() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let num_slots = 3;
        let num_shreds_per_slot = 10;
        let num_entries_per_slot = max_ticks_per_n_shreds(num_shreds_per_slot, None);
        let (mut shreds, _) = make_many_slot_entries(0, num_slots, num_entries_per_slot);
        let shreds_per_slot = shreds.len() / num_slots as usize;

        // Insert all shreds for slot 0 and leave them in the cache
        let shreds0 = shreds.drain(0..shreds_per_slot).collect_vec();
        blockstore
            .insert_shreds(shreds0.clone(), None, false)
            .unwrap();
        verify_iterator(&blockstore, 0, &shreds0);

        // Insert all shreds for slot 1 and flush them to fs
        let shreds1 = shreds.drain(0..shreds_per_slot).collect_vec();
        blockstore
            .insert_shreds(shreds1.clone(), None, false)
            .unwrap();
        blockstore.flush_data_shreds_for_slot_to_fs(1).unwrap();
        verify_iterator(&blockstore, 1, &shreds1);

        // Insert half of shreds for slot 2 and flush them to fs;
        // then insert the other half and leave them in cache
        let shreds2 = shreds.clone();
        let mut shreds2_even = Vec::new();
        let mut shreds2_odd = Vec::new();
        for (i, shred) in shreds.into_iter().enumerate() {
            if i % 2 == 0 {
                shreds2_even.push(shred);
            } else {
                shreds2_odd.push(shred);
            }
        }
        blockstore
            .insert_shreds(shreds2_even.clone(), None, false)
            .unwrap();
        blockstore.flush_data_shreds_for_slot_to_fs(2).unwrap();
        blockstore
            .insert_shreds(shreds2_odd.clone(), None, false)
            .unwrap();
        verify_iterator(&blockstore, 2, &shreds2);

        // Construct an iterator for a non-existent slot
        {
            let slot = num_slots + 1;
            let cache = blockstore.data_shred_slot_cache(slot);
            let (cache_guard, file_data) =
                SlotIterator::setup(&cache, &blockstore.data_shred_slot_path(slot));
            let shred_iter = SlotIterator::new(slot, 0, &cache_guard, &file_data);
            assert_eq!(shred_iter.count(), 0);
        }
    }
}
