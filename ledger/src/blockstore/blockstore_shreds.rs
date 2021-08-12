//! Blockstore functions specific to the storage of shreds
//!
//! TODO: More documentation

use super::*;
use crate::shred::SHRED_PAYLOAD_SIZE;
use std::collections::BTreeMap;
use std::{fs, io::Read, io::Seek, io::SeekFrom, io::Write};

pub const SHRED_DIRECTORY: &str = "shreds";
pub const DATA_SHRED_DIRECTORY: &str = "data";

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
                    // TODO: a quick search showed we exclusively use get_data_shreds_for_slot()
                    // with index = 0. As such, at the cost of flexibility to get partial slots
                    // (not sure of the use case for this), we could remove this index check
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

    // TODO: change this back to pub(crate); possibly need to make wrapper in blockstore_purge instead
    // of in ledger_cleanup_service
    pub fn flush_data_shreds_for_slot_to_fs(&self, slot: Slot) -> Result<()> {
        {
            // First, check if we have a cache for this slot
            let slot_cache = match self.data_slot_cache(slot) {
                Some(slot_cache) => slot_cache,
                // TODO: depending on how caller uses this, may want to return SlotUnavailable
                None => return Ok(()),
            };

            let path = self.slot_data_shreds_path(slot);
            let temp_path = format!("{}.tmp", &path);
            let temp_path = Path::new(&temp_path);

            // Write contents to a temporary file first. We will later rename the file;
            // in this way, we approximate an atomic file write
            // TODO: Should this check if file already exists instead of clobbering it
            let mut file = fs::File::create(temp_path)?;
            let result: Result<Vec<_>> = slot_cache
                .read()
                .unwrap()
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
        // 2) If the shred is not in the index, perform a regular insert so the
        //    the metadata is updated. Collect all of these until the end for perf.
        for (slot, mut shreds) in recovered_shreds.into_iter() {
            let shred_index_opt = self.index_cf.get(slot)?;
            match shred_index_opt {
                Some(shred_index) => {
                    while !shreds.is_empty() {
                        let shred = shreds.pop().unwrap();
                        let index = shred.index() as u64;

                        // TODO: Maybe a helper that checks metadata but doesn't pull shred out
                        // TODO: Handle case where this shred not in index, full insert
                        let shred_on_disk = self.get_data_shred_from_fs(slot, index)?.is_some();
                        if shred_index.data().is_present(index) && !shred_on_disk {
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

    fn slot_data_shreds_path(&self, slot: Slot) -> String {
        Path::new(&self.data_shred_path)
            .join(slot.to_string())
            .to_str()
            .unwrap()
            .to_string()
    }

    // TODO: move stuff around such that this function doesn't have to be public;
    // find_missing_data_indexes() uses this at the moment
    pub(crate) fn data_slot_cache(&self, slot: Slot) -> Option<Arc<RwLock<ShredCache>>> {
        self.data_shred_cache
            .get(&slot)
            .map(|res| res.value().clone())
    }

    fn get_shred_from_fs(slot_path: &str, index: u64) -> Result<Option<Vec<u8>>> {
        // Shreds are grouped into files by slot, so pull out only the relevant shred from file
        let path = Path::new(slot_path);
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_err) => return Ok(None),
        };
        // Shreds are stored end to end, so the ith shred will occupy
        // bytes [i * SHRED_PAYLOAD_SIZE, (i + 1) * SHRED_PAYLOAD_SIZE)
        let payload_size = SHRED_PAYLOAD_SIZE as u64;
        let metadata = fs::metadata(path)?;
        // Ensure the file is long enough to contain desired shred
        if metadata.len() < (index + 1) * payload_size {
            // TODO: Double check that returning None instead of erroring is correct behavior
            return Ok(None);
        }
        file.seek(SeekFrom::Start(index * payload_size))?;
        let mut buffer = vec![0; SHRED_PAYLOAD_SIZE];
        file.read_exact(&mut buffer)?;
        Ok(Some(buffer))
    }

    fn get_shreds_for_slot_from_fs(
        slot_path: &str,
        start_index: u64,
    ) -> Option<ShredResult<Vec<Shred>>> {
        let path = Path::new(slot_path);
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_err) => return Some(Ok(vec![])),
        };
        // Shreds are stored end to end, so the ith shred will occupy
        // bytes [i * SHRED_PAYLOAD_SIZE, (i + 1) * SHRED_PAYLOAD_SIZE)
        let payload_size = SHRED_PAYLOAD_SIZE as u64;
        let metadata = fs::metadata(path).ok()?;
        // Ensure the file will contain at least one shred so we don't seek past end of file
        if metadata.len() < (start_index + 1) * payload_size {
            // TODO: Double check that empty list and not an error is correct behavior
            return Some(Ok(vec![]));
        }
        // TODO: Check metadata.len() % payload_size == 0 ?
        let num_shreds: usize = ((metadata.len() - (start_index * payload_size)) / payload_size)
            .try_into()
            .unwrap();
        let mut buffers = vec![];
        buffers.reserve(num_shreds);
        // Move the cursor up to proper start position so first read grabs start_index
        file.seek(SeekFrom::Start(start_index * payload_size))
            .ok()?;
        for i in 0..num_shreds {
            let mut buffer = vec![0; SHRED_PAYLOAD_SIZE];
            file.read_exact(&mut buffer).ok()?;
            buffers.insert(i, buffer);
        }

        Some(
            buffers
                .into_iter()
                .map(Shred::new_from_serialized_shred)
                .collect(),
        )
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
