//! Blockstore functions specific to the storage of shreds
//!
//! TODO: More documentation
use {
    super::*,
    crate::shred::{ShredType, DATA_SHRED, SHRED_PAYLOAD_SIZE},
    serde::{Deserialize, Serialize},
    solana_measure::measure::Measure,
    std::{
        collections::BTreeMap,
        fs,
        io::{BufWriter, Read, Seek, SeekFrom, Write},
        ops::Bound::{Included, Unbounded},
    },
};

pub(crate) const SHRED_DIRECTORY: &str = "shreds";
pub(crate) const DATA_SHRED_DIRECTORY: &str = "data";

pub(crate) type ShredCache = BTreeMap<u64, Vec<u8>>;
pub(crate) type ShredFileIndex = BTreeMap<u32, u32>;

/// Store shreds on the filesystem in a slot-per-file manner. The
/// file format consists of a header, an index, and a data section.
/// - The header contains basic metadata
/// - The index section contains a serialized BTreeMap mapping shred
///   index to offset in the data section
/// - The data section contained the serialized shreds end to ened
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct ShredFileHeader {
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
    fn new(slot: Slot, num_shreds: usize, index_size: usize, data_size: usize) -> Self {
        let num_shreds = num_shreds as u32;
        let header_size = SIZE_OF_SHRED_FILE_HEADER as u32;
        let index_size = index_size as u32;
        let data_size = data_size as u32;
        Self {
            slot,
            shred_type: ShredType(DATA_SHRED),
            num_shreds,
            index_offset: header_size,
            index_size,
            data_offset: header_size + index_size,
            data_size,
        }
    }

    // Given a shred cache, generate an index of all the shreds present and
    // their individual offsets if they were serialized into a single buffer.
    fn new_shred_index(cache: &ShredCache) -> (ShredFileIndex, usize) {
        let mut offset = 0;
        let index: ShredFileIndex = cache
            .iter()
            .map(|(index, shred)| {
                let result = (*index as u32, offset as u32);
                offset += shred.len();
                result
            })
            .collect();
        // At end of loop, offset would be location of next shred. Since the offset
        // is zero-indexed, this is also the size of all shreds end-to-end
        (index, offset)
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
        let mut flush_timer = Measure::start("flush_timer");

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
        // We'll write contents to a temporary file first, and then rename
        // to desired file such that the write is "atomic".
        let tmp_path = format!("{}.tmp", path.to_str().unwrap());
        let tmp_path = Path::new(&tmp_path);
        let mut tmp_file = fs::File::create(tmp_path)?;
        if path.exists() {
            // There is a file for this slot already, meaning it was previously flushed.
            // We'll have to merge the contents of cache with contents of that file
            let mut cur_file = fs::File::open(&path)?;
            let (header, mut index) = Blockstore::read_shred_file_metadata(&mut cur_file)?;
            let mut flushed_shreds = vec![0; header.data_size.try_into().unwrap()];
            cur_file.read_exact(&mut flushed_shreds)?;
            drop(cur_file);

            // We assume that the number of shreds in slot_cache is much smaller than number of
            // shreds already on disk. This fits the case of the last few shreds trickling in late
            // while the majority of them came in before the slot was initially flushed.
            //
            // First, insert elements from cache into existing index. Use dummy offset for the
            // new entries for now, we'll need to update all offsets later since the new entries
            // could invalidate previously calculated offsets.
            let slot_cache = slot_cache.read().unwrap();
            let mut cache_data_size = 0;
            for (idx, shred) in slot_cache.iter() {
                cache_data_size += shred.len();
                index.insert(*idx as u32, 0);
            }
            // Figre out how large the new file will be, and create a buffer to store all of it.
            // Use a regular buffer here instead of BufWriter because BufWriter must be filled
            // sequentially. Sequential writes would require updating the index first, writing it
            // and then writing the shreds as we iterate through the index a second time. With a
            // regular buffer, we can write shreds as we update the index, and then write the index
            // at the end. This will save us the second pass of the index.
            let data_size = flushed_shreds.len() + cache_data_size;
            let serialized_index_size: usize =
                bincode::serialized_size(&index)?.try_into().unwrap();
            let header = ShredFileHeader::new(slot, index.len(), serialized_index_size, data_size);
            let serialized_header = bincode::serialize(&header)?;
            let mut write_buffer =
                vec![0; SIZE_OF_SHRED_FILE_HEADER + serialized_index_size + data_size];
            write_buffer[..SIZE_OF_SHRED_FILE_HEADER].copy_from_slice(&serialized_header[..]);

            // Iterate through the index and search for items from the cache. When these are found,
            // we know we need to update offset for all downstream elements.
            let mut cache_iter = slot_cache.iter();
            let mut cache_item = cache_iter.next();

            let mut offset_adjustment = 0;
            let mut buffer_write_position = header.data_offset as usize;
            let mut flushed_read_position = 0;
            let mut flushed_read_size = 0;

            for (idx, offset) in index.iter_mut() {
                if cache_item.is_some() && *idx == *cache_item.unwrap().0 as u32 {
                    if flushed_read_size != 0 {
                        // This could be the case if first index is from the cache or adjacent
                        // indexes are both from the cache
                        write_buffer
                            [buffer_write_position..buffer_write_position + flushed_read_size]
                            .copy_from_slice(
                                &flushed_shreds[flushed_read_position
                                    ..flushed_read_position + flushed_read_size],
                            );
                        buffer_write_position += flushed_read_size;
                        flushed_read_position += flushed_read_size;
                        flushed_read_size = 0;
                    }

                    *offset = buffer_write_position as u32 - header.data_offset;

                    let shred_size = cache_item.unwrap().1.len();
                    write_buffer[buffer_write_position..buffer_write_position + shred_size]
                        .copy_from_slice(cache_item.unwrap().1);
                    buffer_write_position += shred_size;

                    offset_adjustment += shred_size as u32;
                    cache_item = cache_iter.next();
                } else {
                    flushed_read_size += SHRED_PAYLOAD_SIZE;
                    *offset += offset_adjustment;
                }
            }
            // If the last shred in index was not from cache, then we need to force this write
            if flushed_read_size != 0 {
                write_buffer[buffer_write_position..buffer_write_position + flushed_read_size]
                    .copy_from_slice(
                        &flushed_shreds
                            [flushed_read_position..flushed_read_position + flushed_read_size],
                    );
            }

            // The index is now updated so we can serialize and push into buffer
            let serialized_index = bincode::serialize(&index)?;
            let header_write_offset: usize = header.index_offset as usize;
            write_buffer[header_write_offset..header_write_offset + serialized_index_size]
                .copy_from_slice(&serialized_index[..]);

            tmp_file.write_all(&write_buffer)?;
        } else {
            // No data for this slot on disk, just need to dump the contents of the cache
            let slot_cache = slot_cache.read().unwrap();
            let (index, data_size) = ShredFileHeader::new_shred_index(&slot_cache);
            let serialized_index = bincode::serialize(&index)?;
            let serialized_header = bincode::serialize(&ShredFileHeader::new(
                slot,
                index.len(),
                serialized_index.len(),
                data_size,
            ))?;
            let mut write_buffer = BufWriter::with_capacity(
                SIZE_OF_SHRED_FILE_HEADER + serialized_index.len() + data_size,
                tmp_file,
            );
            write_buffer.write_all(&serialized_header)?;
            write_buffer.write_all(&serialized_index)?;
            let result: Result<Vec<_>> = slot_cache
                .iter()
                .map(|(_, shred)| {
                    write_buffer.write_all(shred).map_err(|err| {
                        BlockstoreError::Io(IoError::new(
                            ErrorKind::Other,
                            format!("Unable to write slot {}: {}", slot, err),
                        ))
                    })
                })
                .collect();

            let _result = result?;
            write_buffer.flush()?;
        }
        // Now, the temporary file has been completely written so perform the rename and then drop
        // the slot from the cache. fs::rename() will replace existing file if there is one.
        fs::rename(tmp_path, &path)?;
        self.data_shred_cache.remove(&slot);
        self.data_shred_cache_slots.lock().unwrap().remove(&slot);
        flush_timer.stop();
        debug!("Flush took {}us", flush_timer.as_us());
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
        // There several possible scenarios for what we need to do with shreds from the WAL
        // 1) If the shred is in index ...
        //    - If the shred is on disk, it is already accounted for and do nothing
        //    - If the shred is not on disk, it was in memory and lost when process died.
        //      So, re-insert it into the cache directly (to avoid having it in WAL twice)
        // 2) If the shred is not in the index (including the case where there is no index for
        //    the slot), perform a regular insert so the metadata is updated. Collect all of
        //    these until the end for single insert.
        for (slot, mut shreds) in recovered_shreds.into_iter() {
            let shred_db_index_opt = self.index_cf.get(slot)?;
            match shred_db_index_opt {
                Some(shred_db_index) => {
                    // Shreds are stored by slot on file with an index of which shreds are present
                    let path = self.slot_data_shreds_path(slot);
                    let shred_file_index = match fs::File::open(&path) {
                        Ok(mut file) => {
                            let (_header, file_index) =
                                Blockstore::read_shred_file_metadata(&mut file)?;
                            Some(file_index)
                        }
                        Err(_err) => None,
                    };

                    while !shreds.is_empty() {
                        let shred = shreds.pop().unwrap();
                        let index = shred.index() as u64;

                        if !shred_db_index.data().is_present(index) {
                            full_insert_shreds.push(shred);
                        } else if shred_file_index.is_none()
                            || shred_file_index
                                .as_ref()
                                .unwrap()
                                .get(&(index as u32))
                                .is_none()
                        {
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

    pub(crate) fn slot_data_shreds_path(&self, slot: Slot) -> PathBuf {
        self.data_shred_path.join(slot.to_string())
    }

    // Convenience wrapper to retrieve a single shred payload from fs
    fn get_shred_from_fs(slot_path: &Path, index: u64) -> Result<Option<Vec<u8>>> {
        // Use the same value for start and end index to signify we only want one payload
        let mut payloads =
            Self::get_shred_payloads_for_slot_from_fs(slot_path, index, Some(index))?;
        Ok(payloads.pop())
    }

    pub(crate) fn read_shred_file_metadata(
        file: &mut fs::File,
    ) -> Result<(ShredFileHeader, ShredFileIndex)> {
        let mut header_buffer = vec![0; SIZE_OF_SHRED_FILE_HEADER];
        file.read_exact(&mut header_buffer)?;
        let header: ShredFileHeader = bincode::deserialize(&header_buffer)?;

        let mut index_buffer = vec![0; header.index_size.try_into().unwrap()];
        file.read_exact(&mut index_buffer)?;
        let file_index: ShredFileIndex = bincode::deserialize(&index_buffer)?;

        Ok((header, file_index))
    }

    // Retrieve shreds from fs
    // If end_index is Some(), range is inclusive on both ends
    // If end_index is None, range is inclusive on start, unbounded on end
    fn get_shred_payloads_for_slot_from_fs(
        slot_path: &Path,
        start_index: u64,
        end_index: Option<u64>,
    ) -> Result<Vec<Vec<u8>>> {
        let mut file = match fs::File::open(slot_path) {
            Ok(file) => file,
            Err(_err) => return Ok(Vec::new()),
        };
        let (_header, file_index) = Blockstore::read_shred_file_metadata(&mut file)?;

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
        if let Some((&_low_bound, &seek_offset)) = file_index.range((start, end)).next() {
            // Prior to this call, the cursor is after the index / at begginning of data
            // section. The offsets in ShredFileIndex are zero-indexed from this point,
            // so we just seek forward whatever value was in the index.
            file.seek(SeekFrom::Current(seek_offset as i64))?;
        } else {
            return Ok(buffers);
        }

        for (_index, _offset) in file_index.range((start, end)) {
            let mut buffer = vec![0; SHRED_PAYLOAD_SIZE];
            file.read_exact(&mut buffer)?;
            buffers.push(buffer);
        }
        Ok(buffers)
    }

    /*
    pub(crate) fn is_data_shred_on_fs(&self, slot: Slot) -> bool {
        let path = self.slot_data_shreds_path(slot);
        let file = fs::File::open(path);
        return match file {
            Ok(mut file) => {
                // TODO: Should below use .unwrap(); file exists so only fails if file screwed up
                let (_, file_index) = Blockstore::read_shred_file_metadata(&mut file).unwrap();
                file_index.get(&(slot as u32)).is_some()
            }
            Err(_err) => false,
        };
    }
    */

    // Used for tests only
    pub fn is_data_shred_in_cache(&self, slot: Slot, index: u64) -> bool {
        self.get_data_shred_from_cache(slot, index)
            .unwrap()
            .is_some()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{get_tmp_ledger_path_auto_delete, shred::max_ticks_per_n_shreds};

    #[test]
    fn test_get_data_shred_from_cache() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Create a bunch of shreds and insert them
        let num_slots = 5;
        let num_entries_per_slot = max_ticks_per_n_shreds(20, None);
        let (shreds, _) = make_many_slot_entries(0, num_slots, num_entries_per_slot);
        blockstore
            .insert_shreds(shreds.clone(), None, false)
            .unwrap();

        // Ensure that all shreds inserted into cache can be retrieved
        for shred in shreds.iter() {
            assert_eq!(
                shred.payload,
                blockstore
                    .get_data_shred_from_cache(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
        }
        // Try retrieving a shred that wasn't inserted
        assert!(blockstore
            .get_data_shred_from_cache(num_slots + 1, 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_flush_data_shreds_for_full_slot_to_fs() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Create a bunch of shreds and insert them
        let num_entries = max_ticks_per_n_shreds(10, None);
        let (shreds, _) = make_slot_entries(0, 0, num_entries);
        blockstore
            .insert_shreds(shreds.clone(), None, false)
            .unwrap();

        // Just inserted shreds in cache only, not yet on disk
        for shred in shreds.iter() {
            assert!(blockstore
                .get_data_shred_from_fs(shred.slot(), shred.index().into())
                .unwrap()
                .is_none());
        }

        // Flush the slot from cache to disk
        blockstore.flush_data_shreds_for_slot_to_fs(0).unwrap();

        // Confirm shreds can be read back from fs, but not from cache
        for shred in shreds.iter() {
            assert_eq!(
                shred.payload,
                blockstore
                    .get_data_shred_from_fs(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
            assert!(blockstore
                .get_data_shred_from_cache(shred.slot(), shred.index().into())
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn test_flush_data_shreds_for_partial_slot_to_fs() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        // Create a bunch of shreds
        let num_entries = max_ticks_per_n_shreds(10, None);
        let (shreds, _) = make_slot_entries(0, 0, num_entries);
        // Divide up the shreds to check the bounds conditions for the
        // shreds1 = [0, 3, 5, 6, 9] which has first, last and adjacent shreds
        // shreds2 = [1, 2, 4, 7, 8]
        let mut shreds1 = Vec::new();
        let mut shreds2 = Vec::new();
        for (i, shred) in shreds.clone().into_iter().enumerate() {
            if i % 3 == 0 || i == 5 {
                shreds1.push(shred);
            } else {
                shreds2.push(shred);
            }
        }

        // Insert and flush shreds1 - this will be a straightforward flush
        blockstore
            .insert_shreds(shreds1.clone(), None, false)
            .unwrap();
        blockstore.flush_data_shreds_for_slot_to_fs(0).unwrap();

        // Confirm shreds can be read back from fs
        for shred in shreds1.iter() {
            assert_eq!(
                shred.payload,
                blockstore
                    .get_data_shred_from_fs(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
        }

        // Insert and flush shreds2 - this will perform merge of cache and already flushed shreds
        blockstore
            .insert_shreds(shreds2.clone(), None, false)
            .unwrap();
        blockstore.flush_data_shreds_for_slot_to_fs(0).unwrap();

        // Confirm all shreds can be read back from fs
        for shred in shreds.iter() {
            assert_eq!(
                shred.payload,
                blockstore
                    .get_data_shred_from_fs(shred.slot(), shred.index().into())
                    .unwrap()
                    .unwrap()
            );
        }
    }
}
