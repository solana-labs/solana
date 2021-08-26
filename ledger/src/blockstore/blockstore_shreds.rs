//! Blockstore functions specific to the storage of shreds
//!
//! TODO: More documentation
use {
    super::*,
    crate::shred::{ShredType, DATA_SHRED, SHRED_PAYLOAD_SIZE},
    serde::{Deserialize, Serialize},
    std::{
        collections::BTreeMap,
        fs,
        io::{Read, Seek, SeekFrom, Write},
        ops::Bound::{Included, Unbounded},
    },
};

pub(crate) const SHRED_DIRECTORY: &str = "shreds";
pub(crate) const DATA_SHRED_DIRECTORY: &str = "data";

pub(crate) type ShredCache = BTreeMap<u64, Vec<u8>>;
type ShredFileIndex = BTreeMap<u32, u32>;

/// Store shreds on the filesystem in a slot-per-file manner. The
/// file format consists of a header, an index, and a data section.
/// - The header contains basic metadata
/// - The index section contains a serialized BTreeMap mapping shred
///   index to offset in the data section
/// - The data section contained the serialized shreds end to ened
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ShredFileHeader {
    pub slot: Slot,
    pub shred_type: ShredType,
    pub num_shreds: u32,
    // Offset (in bytes) of the index
    pub index_offset: u32,
    // Size (in bytes) of the index
    pub index_size: u32,
    // Offset (in bytes) of the data
    pub data_offset: u32,
    // Size (in bytes) of all the serialized shreds
    pub data_size: u32,
}

// The following constant is computed by hand and hardcoded;
// 'test_asdf` ensures the value is correct.
const SIZE_OF_SHRED_FILE_HEADER: usize = 29;

impl ShredFileHeader {
    fn new(slot: Slot, cache: &ShredCache) -> Self {
        let num_shreds: u32 = cache.len().try_into().unwrap();
        Self {
            slot,
            shred_type: ShredType(DATA_SHRED),
            num_shreds,
            index_offset: SIZE_OF_SHRED_FILE_HEADER as u32,
            index_size: num_shreds * 8,
            data_offset: 0,
            data_size: num_shreds * (SHRED_PAYLOAD_SIZE as u32),
        }
    }
}

impl Blockstore {
    pub(crate) fn get_data_shred_from_cache(
        &self,
        slot: Slot,
        index: u64,
    ) -> Result<Option<Vec<u8>>> {
        let payload = self
            .data_slot_cache(slot)
            .and_then(|slot_cache| slot_cache.read().unwrap().get(&index).cloned());
        Ok(payload)
    }

    pub(crate) fn get_data_shred_from_fs(&self, slot: Slot, index: u64) -> Result<Option<Vec<u8>>> {
        let path = self.slot_data_shreds_path(slot);
        Self::get_shred_from_fs(&path, index)
    }

    pub(crate) fn get_data_shreds_for_slot_from_cache(
        &self,
        slot: Slot,
        start_index: u64,
    ) -> Option<ShredResult<Vec<Shred>>> {
        // First, check if we have a cache for this slot
        let slot_cache = match self.data_slot_cache(slot) {
            Some(slot_cache) => slot_cache,
            None => return None,
        };
        // Cache exists, grab and hold read lock while we iterate through
        let slot_cache = slot_cache.read().unwrap();
        Some(
            slot_cache
                .iter()
                .filter_map(|(shred_index, shred)| {
                    if shred_index >= &start_index {
                        Some(Shred::new_from_serialized_shred(shred.to_vec()))
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    pub(crate) fn get_data_shreds_for_slot_from_fs(
        &self,
        slot: Slot,
        start_index: u64,
    ) -> Option<ShredResult<Vec<Shred>>> {
        let path = self.slot_data_shreds_path(slot);
        Self::get_shreds_for_slot_from_fs(&path, start_index)
    }

    pub(crate) fn find_missing_data_indexes_cache(
        &self,
        first_timestamp: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
        slot_cache: Arc<RwLock<ShredCache>>,
    ) -> Vec<u64> {
        let ticks_since_first_insert =
            DEFAULT_TICKS_PER_SECOND * (timestamp() - first_timestamp) / 1000;

        let mut missing_indexes = vec![];
        let mut prev_index = start_index;
        'outer: for (index, shred) in slot_cache.read().unwrap().iter() {
            if *index < start_index {
                continue;
            }
            // Get the tick that will be used to figure out the timeout for this hole
            let reference_tick = u64::from(Shred::reference_tick_from_data(shred));
            // Break out early if the higher index holes have not timed out yet
            if ticks_since_first_insert < reference_tick + MAX_TURBINE_DELAY_IN_TICKS {
                return missing_indexes;
            }
            // Insert any newly discovered holes
            for i in prev_index..cmp::min(*index, end_index) {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break 'outer;
                }
            }
            // Update prev_index before the end-early check as we may use prev_index after
            prev_index = *index + 1;
            if *index >= end_index {
                break;
            }
        }
        // If prev_index < end_index, there could be holes within [start_index, end_index)
        // but that are greater than any shreds we have in the blockstore
        if missing_indexes.len() < max_missing && prev_index < end_index {
            for i in prev_index..end_index {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break;
                }
            }
        }
        missing_indexes
    }

    pub(crate) fn find_missing_data_indexes_fs(
        &self,
        first_timestamp: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
        shreds: Vec<Shred>,
    ) -> Vec<u64> {
        let ticks_since_first_insert =
            DEFAULT_TICKS_PER_SECOND * (timestamp() - first_timestamp) / 1000;

        let mut missing_indexes = vec![];
        let mut prev_index = start_index;
        'outer: for shred in shreds {
            let index = u64::from(shred.index());
            if index < start_index {
                continue;
            }
            // Get the tick that will be used to figure out the timeout for this hole
            let reference_tick = u64::from(shred.reference_tick());
            // Break out early if the higher index holes have not timed out yet
            if ticks_since_first_insert < reference_tick + MAX_TURBINE_DELAY_IN_TICKS {
                return missing_indexes;
            }
            // Insert any newly discovered holes
            for i in prev_index..cmp::min(index, end_index) {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break 'outer;
                }
            }
            // Update prev_index before the end-early check as we may use prev_index after
            prev_index = index + 1;
            if index >= end_index {
                break;
            }
        }
        // If prev_index < end_index, there could be holes within [start_index, end_index)
        // but that are greater than any shreds we have in the blockstore
        if missing_indexes.len() < max_missing && prev_index < end_index {
            for i in prev_index..end_index {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break;
                }
            }
        }
        missing_indexes
    }

    pub(crate) fn insert_data_shred_into_cache(&self, slot: Slot, index: u64, shred: &Shred) {
        let data_slot_cache = self.data_slot_cache(slot).unwrap_or_else(|| {
            // Inner map for slot does not exist, let's create it
            // DashMap .entry().or_insert() returns a RefMut, essentially a write lock,
            // which is dropped after this block ends, minimizing time held by the lock.
            // We still need a reference to the `ShredCache` behind the lock, hence, we
            // clone it out (`ShredCache` is an Arc so it is cheap to clone).
            self.data_shred_cache_slots.lock().unwrap().insert(slot);
            self.data_shred_cache
                .entry(slot)
                .or_insert(Arc::new(RwLock::new(BTreeMap::new())))
                .clone()
        });
        data_slot_cache
            .write()
            .unwrap()
            .insert(index, shred.payload.clone());
    }

    // Shreds are stored by slot, and assumed to be full when pushed to disk. With these
    // assumptions, we don't require the shred index for this function.
    pub(crate) fn is_data_shred_on_fs(&self, slot: Slot) -> bool {
        Path::new(&self.slot_data_shreds_path(slot)).exists()
    }

    // TODO: change this back to pub(crate); possibly need to make wrapper in blockstore_purge instead
    // of in ledger_cleanup_service
    pub fn flush_data_shreds_for_slot_to_fs(&self, slot: Slot) -> Result<()> {
        {
            // First, check if we have a cache for this slot
            let slot_cache = match self.data_slot_cache(slot) {
                Some(slot_cache) => slot_cache,
                None => {
                    // TODO: An error here indicates cache/keys out of sync, not
                    // sure what we should do here
                    error!(
                        "Slot {} was picked to be flushed, but not actually in cache",
                        slot
                    );
                    return Ok(());
                }
            };

            let path = self.slot_data_shreds_path(slot);
            let temp_path = format!("{}.tmp", &path);
            let temp_path = Path::new(&temp_path);

            // Write contents to a temporary file first bit-by-bit. We will later rename the
            // file, with this approach, the the file write will be "atomic".
            // TODO: Make this handle file already existing (merge)
            let mut file = fs::File::create(temp_path)?;
            let slot_cache = slot_cache.read().unwrap();
            let mut header = ShredFileHeader::new(slot, &slot_cache);
            let mut offset = 0;
            let index: ShredFileIndex = slot_cache
                .iter()
                .map(|(index, _shred)| {
                    let result = (*index as u32, offset as u32);
                    offset += SHRED_PAYLOAD_SIZE;
                    result
                })
                .collect();
            let index = bincode::serialize(&index)?;
            header.index_size = index.len() as u32;
            header.data_offset = SIZE_OF_SHRED_FILE_HEADER as u32 + header.index_size;
            header.data_size = offset as u32;

            let header = bincode::serialize(&header)?;
            file.write_all(&header)?;
            file.write_all(&index)?;

            let result: Result<Vec<_>> = slot_cache
                .iter()
                .map(|(_, shred)| {
                    file.write_all(shred).map_err(|err| {
                        BlockstoreError::Io(IoError::new(
                            ErrorKind::Other,
                            format!("Unable to write slot {}: {}", slot, err),
                        ))
                    })
                })
                .collect();
            drop(slot_cache);
            // Check that all of the individual writes succeeded
            let _result = result?;
            let path = Path::new(&path);
            fs::rename(temp_path, path)?;
        }
        // Slot has successfully been persisted, drop it from cache
        self.data_shred_cache.remove(&slot);
        self.data_shred_cache_slots.lock().unwrap().remove(&slot);
        Ok(())
    }

    /// Purge the data shreds within [from_slot, to_slot) slots
    pub(crate) fn purge_data_shreds(&self, from_slot: Slot, to_slot: Slot) {
        // Remove from the cache; no issues if the slot had previously been flushed
        let mut data_shred_cache_slots = self.data_shred_cache_slots.lock().unwrap();
        for slot in from_slot..to_slot {
            data_shred_cache_slots.remove(&slot);
            self.data_shred_cache.remove(&slot);
        }
        drop(data_shred_cache_slots);
        // TODO: Do this in parallel across several threads ?
        for slot in from_slot..to_slot {
            // Could get errors such as file doesn't exist; we don't care so just eat the error
            let _ = fs::remove_file(self.slot_data_shreds_path(slot));
        }
    }

    /// Recover shreds from WAL and re-establish consistent state in the blockstore
    pub(crate) fn recover(&self) -> Result<()> {
        let mut shred_wal = self.shred_wal.lock().unwrap();
        let recovered_shreds = shred_wal.recover()?;
        let mut full_insert_shreds = vec![];
        // There several possible scenarios for what we need to do with the shred
        // 1) If the shred is in index ...
        //    - If the shred is on disk, it is already accounted for and do nothing
        //    - If the shred is not on disk, it was in memory and lost when process died.
        //      So, re-insert it into the cache directly (to avoid having it in WAL twice)
        // 2) If the shred is not in the index (including the case where there is no index for
        //    the slot), perform a regular insert so the metadata is updated. Collect all of
        //    these until the end for single insert.
        for (slot, mut shreds) in recovered_shreds.into_iter() {
            let shred_index_opt = self.index_cf.get(slot)?;
            match shred_index_opt {
                Some(shred_index) => {
                    // Shreds are stored by slot and assumed to be complete when stored on fs,
                    // so we only need to read this once instead of inside below while loop.
                    let shred_on_disk = self.is_data_shred_on_fs(slot);
                    while !shreds.is_empty() {
                        let shred = shreds.pop().unwrap();
                        let index = shred.index() as u64;

                        if !shred_index.data().is_present(index) {
                            full_insert_shreds.push(shred);
                        } else if !shred_on_disk {
                            self.insert_data_shred_into_cache(slot, index, &shred)
                        }
                    }
                }
                None => full_insert_shreds.extend(shreds),
            }
        }
        // For now, drop shreds found in WAL but not in metadata; state will be consistent
        // like this, but we probably do want to do a full insert with these later
        if !full_insert_shreds.is_empty() {
            warn!(
                "{} shreds found in WAL but not in Rocks",
                full_insert_shreds.len()
            );
        }
        // TODO: is leader_schedule as None here ok ? Not sure what else could be gathered ?
        // TODO: insert_shreds() will deadlock as blockstore::recover() holds the WAL lock; need a
        // flag to avoid writing WAL in this special case, we want that anyways as any insertions
        // generated below are already in the WAL so we don't want them duplicated
        // self.insert_shreds(full_insert_shreds, None, false);
        Ok(())
    }

    pub(crate) fn destroy_shreds(shred_db_path: &Path) -> Result<()> {
        // fs::remove_dir_all() will fail if the path doesn't exist
        fs::create_dir_all(&shred_db_path)?;
        fs::remove_dir_all(&shred_db_path)?;
        Ok(())
    }

    pub(crate) fn shred_storage_size(&self) -> Result<u64> {
        Ok(fs_extra::dir::get_size(
            &self.ledger_path.join(SHRED_DIRECTORY),
        )?)
    }

    pub(crate) fn data_slot_cache(&self, slot: Slot) -> Option<Arc<RwLock<ShredCache>>> {
        self.data_shred_cache
            .get(&slot)
            .map(|res| res.value().clone())
    }

    fn slot_data_shreds_path(&self, slot: Slot) -> String {
        Path::new(&self.data_shred_path)
            .join(slot.to_string())
            .to_str()
            .unwrap()
            .to_string()
    }

    // Convenience wrapper to retrieve a single shred payload from fs
    fn get_shred_from_fs(slot_path: &str, index: u64) -> Result<Option<Vec<u8>>> {
        // Use the same value for start and end index to signify we only want one payload
        let mut payloads =
            Self::get_shred_payloads_for_slot_from_fs(slot_path, index, Some(index))?;
        Ok(payloads.pop())
    }

    // Convenience wrapper to retrieve and deserialize shreds from fs
    fn get_shreds_for_slot_from_fs(
        slot_path: &str,
        start_index: u64,
    ) -> Option<ShredResult<Vec<Shred>>> {
        let payloads =
            Self::get_shred_payloads_for_slot_from_fs(slot_path, start_index, None).ok()?;
        Some(
            payloads
                .into_iter()
                .map(Shred::new_from_serialized_shred)
                .collect(),
        )
    }

    // Retrieve shreds from fs
    // If end_index is Some(), range is inclusive on both ends
    // If end_index is None, range is inclusive on start, unbounded on end
    fn get_shred_payloads_for_slot_from_fs(
        slot_path: &str,
        start_index: u64,
        end_index: Option<u64>,
    ) -> Result<Vec<Vec<u8>>> {
        let path = Path::new(slot_path);
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_err) => return Ok(Vec::new()),
        };

        let mut header_buffer = vec![0; SIZE_OF_SHRED_FILE_HEADER];
        file.read_exact(&mut header_buffer)?;
        let header: ShredFileHeader = bincode::deserialize(&header_buffer)?;

        let mut index_buffer = vec![0; header.index_size.try_into().unwrap()];
        file.read_exact(&mut index_buffer)?;
        let index: ShredFileIndex = bincode::deserialize(&index_buffer)?;

        // Establish the bounds for the scan
        let start = Included(start_index as u32);
        let end = if let Some(end_index) = end_index {
            Included(end_index as u32)
        } else {
            Unbounded
        };
        let mut buffers = Vec::new();
        // Grab the lowest entry in the search range:
        // - If the result is Some(), do a file.seek() to get cursor to correct position.
        // - If the result is None, there is no overlap between the search range and the
        //   shreds actually present in the file.
        if let Some((&_low_bound, &seek_offset)) = index.range((start, end)).next() {
            // Prior to this call, the cursor is after the index / at begginning of data
            // section. The offsets in ShredFileIndex are zero-indexed from this point,
            // so we just seek forward whatever value was in the index.
            file.seek(SeekFrom::Current(seek_offset as i64))?;
        } else {
            return Ok(buffers);
        }

        for (_index, _offset) in index.range((start, end)) {
            let mut buffer = vec![0; SHRED_PAYLOAD_SIZE];
            file.read_exact(&mut buffer)?;
            buffers.push(buffer);
        }
        Ok(buffers)
    }

    // Used for tests only
    pub fn is_shred_in_cache(&self, slot: Slot, index: u64) -> bool {
        self.get_data_shred_from_cache(slot, index)
            .unwrap()
            .is_some()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{get_tmp_ledger_path, shred::max_ticks_per_n_shreds};

    #[test]
    fn test_get_data_shred_from_cache() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blockstore::open(&ledger_path).unwrap();

        // Create a bunch of shreds and insert them
        let num_entries = max_ticks_per_n_shreds(50, None);
        let (shreds, _) = make_slot_entries(0, 0, num_entries);
        ledger.insert_shreds(shreds.clone(), None, false).unwrap();

        // Ensure that all shreds inserted into cache can be retrieved
        for shred in shreds.iter() {
            assert_eq!(
                shred.payload,
                ledger
                    .get_data_shred_from_cache(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
        }
        // Try retrieving a shred that wasn't inserted
        assert!(ledger.get_data_shred_from_cache(1, 0).unwrap().is_none());

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_flush_data_shreds_for_slot_to_fs() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blockstore::open(&ledger_path).unwrap();

        // Create a bunch of shreds and insert them
        let num_entries = max_ticks_per_n_shreds(100, None);
        let (shreds, _) = make_slot_entries(0, 0, num_entries);
        ledger.insert_shreds(shreds.clone(), None, false).unwrap();

        // Just inserted shreds in cache only, not yet on disk
        for shred in shreds.iter() {
            assert!(ledger
                .get_data_shred_from_fs(shred.slot(), shred.index().into())
                .unwrap()
                .is_none());
        }

        // Flush the slot from cache to disk
        ledger.flush_data_shreds_for_slot_to_fs(0).unwrap();

        // Confirm shreds can be read back from fs, but not from cache
        for shred in shreds.iter() {
            assert_eq!(
                shred.payload,
                ledger
                    .get_data_shred_from_fs(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
            assert!(ledger
                .get_data_shred_from_cache(shred.slot(), shred.index().into())
                .unwrap()
                .is_none());
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
