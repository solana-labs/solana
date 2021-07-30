// Blockstore functions specific to the storage of shreds

use super::*;
use crate::shred::SHRED_PAYLOAD_SIZE;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{cmp, fs, io::Read, io::Seek, io::SeekFrom, io::Write, ops::Range};

pub const SHRED_DIRECTORY: &str = "shreds";
pub const SHRED_WAL_DIRECTORY: &str = "log";
pub const DATA_SHRED_DIRECTORY: &str = "data";

// For general information on a WAL (Write Ahead Log), see
// https://en.wikipedia.org/wiki/Write-ahead_logging
//
// This WAL will record shred operations, namely, insertions and deletions. In the event of a
// system crash / reboot, the WAL will be played back to establish the state that was lost in
// memory. As shreds are more permanently persisted to non-volatile storage in the database,
// parts of the WAL may become unnecessary and can then be cleaned up.
//
// With the existence of delete operations, order matters and the WAL must replayed in the same
// order that it was written.
pub struct ShredWAL {
    // Directory where WAL file(s) will be stored
    wal_path: PathBuf,
    // The maximum number of shreds allowed in a single WAL file
    max_shreds: usize,
    // The number of shreds written to current WAL file
    cur_shreds: usize,
    // ID to current WAL file
    cur_id: Option<u128>,
    // Map of WAL ID to max slot contained in WAL
    max_slots: BTreeMap<u128, u64>,
}

// TODO: revisit this value / correlate it to the cache size and/or flush interval
// Shreds are 1228 bytes, so WAL filesize will be at most 1228 * 1024 * 128 = 153.5 MB
pub const DEFAULT_MAX_WAL_SHREDS: usize = 1024 * 128;

// WAL entries consist of a SHRED_PAYLOAD_SIZE data section and 4 byte identifier footer
// Types of WAL entries:
// - Insertion: | Payload | Type |
// - Deletion:  | Purge From Slot | Purge To Slot | Zero Padding | Type |
//   - Purge To Slot is exclusive, so this number should have a +1 increment baked in
const WAL_ENTRY_SIZE: usize = SHRED_PAYLOAD_SIZE + 4;
// Constants used to identify WAL entries
const WAL_ENTRY_INSERTION: u32 = 0xFFFF0000;
const WAL_ENTRY_DELETION: u32 = 0x0000FFFF;

type RecoveredShreds = HashMap<Slot, Vec<Shred>>;

impl ShredWAL {
    pub fn new(shred_db_path: &Path, max_shreds: usize) -> Result<ShredWAL> {
        let wal_path = shred_db_path.join(SHRED_WAL_DIRECTORY);
        fs::create_dir_all(&wal_path)?;
        let wal = Self {
            wal_path,
            max_shreds,
            cur_shreds: 0,
            cur_id: None,
            max_slots: BTreeMap::new(),
        };
        Ok(wal)
    }

    // Recover shreds from log files at specified path
    pub fn recover(&mut self) -> Result<RecoveredShreds> {
        assert!(&self.wal_path.is_dir());
        let mut buffer_map = HashMap::new();
        // Log filenames are the timestamp at which they're created; we want to proceed
        // through log files in the same order that they were created (ascending)
        // fs::read_dir() doesn't guaranteed results to be sorted, so we explicitly sort
        let dir = fs::read_dir(&self.wal_path)?;
        let mut logs: Vec<_> = dir.filter_map(|log| log.ok()).collect();
        logs.sort_by_key(|log| log.path());
        for log in logs {
            // TODO: better error handling below line? We can probably fail
            // if there is some unknown file in this directory
            let log_id: u128 = log.file_name().to_str().unwrap().parse().unwrap();
            let mut log_max_slot = 0;

            let path = self.wal_path.join(log_id.to_string());
            let mut file = fs::File::open(path)?;
            // In the event of a quick restart, make sure WAL has been completely flushed
            // from OS buffer to disk before attempting to read from it.
            file.sync_all()?;
            loop {
                let mut buffer = vec![0; WAL_ENTRY_SIZE];
                match file.read_exact(&mut buffer).ok() {
                    Some(_) => {
                        let entry_type = u32::from_le_bytes(
                            buffer[SHRED_PAYLOAD_SIZE..WAL_ENTRY_SIZE]
                                .try_into()
                                .unwrap(),
                        );
                        match entry_type {
                            WAL_ENTRY_INSERTION => {
                                let slot = Shred::get_slot_from_data(&buffer).unwrap();
                                let buffers = buffer_map.entry(slot).or_insert_with(Vec::new);
                                buffers.push(buffer);
                                log_max_slot = cmp::max(log_max_slot, slot);
                            }
                            WAL_ENTRY_DELETION => {
                                for slot in Self::decode_deletion_entry(&buffer) {
                                    buffer_map.remove(&slot);
                                }
                            }
                            _ => {
                                warn!("Invalid WAL entry found of type ID: {:#X}", entry_type);
                                // TODO: panic here ?
                            }
                        }
                    }
                    None => break,
                };
            }
            self.max_slots.insert(log_id, log_max_slot);
        }

        // Shred::new_from_serialized_shred() will strip off the type identifier from buffers
        let mut shred_map: RecoveredShreds = HashMap::new();
        for (slot, buffers) in buffer_map.into_iter() {
            let shreds: ShredResult<Vec<_>> = buffers
                .into_iter()
                .map(Shred::new_from_serialized_shred)
                .collect();
            let shreds = shreds.map_err(|err| {
                BlockstoreError::InvalidShredData(Box::new(bincode::ErrorKind::Custom(format!(
                    "Could not reconstruct shred from shred payload: {:?}",
                    err
                ))))
            })?;
            shred_map.insert(slot, shreds);
        }
        Ok(shred_map)
    }

    // Write the supplied shred payloads into the log
    // This function modifies the shreds in-place, so this function consumes the shred HashMap
    // instead of borrowing to avoid corrupted shreds being used later
    pub fn log_shred_write(&mut self, mut shreds: HashMap<(u64, u64), Shred>) -> Result<()> {
        let mut log = self.open_or_create_log(shreds.len())?;
        let mut insert_max_slot = 0;
        let result: Result<Vec<_>> = shreds
            .iter_mut()
            .map(|((slot, index), shred)| {
                insert_max_slot = cmp::max(insert_max_slot, *slot);
                shred.payload.extend(WAL_ENTRY_INSERTION.to_le_bytes());
                log.write_all(&shred.payload).map_err(|err| {
                    // TODO: slot / index possibly not relevant, also should we panic?
                    BlockstoreError::Io(IoError::new(
                        ErrorKind::Other,
                        format!(
                            "Unable to write shred (slot {}, index {}) to wal: {}",
                            slot, index, err
                        ),
                    ))
                })
            })
            .collect();
        // Check that all of the individual writes succeeded and then update state
        let _result = result?;
        self.cur_shreds += shreds.len();
        let cur_max_slot = *self.max_slots.get(&self.cur_id.unwrap()).unwrap();
        self.max_slots.insert(
            self.cur_id.unwrap(),
            cmp::max(cur_max_slot, insert_max_slot),
        );
        Ok(())
    }

    // Mark the specified range of slots as purged in the log
    pub fn log_shred_purge(&mut self, range: Range<Slot>) -> Result<()> {
        let entry = Self::encode_deletion_entry(&range);
        // A purge entry occupies the space of one shred
        let mut log = self.open_or_create_log(1)?;
        log.write_all(&entry).map_err(|err| {
            BlockstoreError::Io(IoError::new(
                ErrorKind::Other,
                format!(
                    "Unable to log purge from slot {} to slot {} in wal: {}",
                    range.start, range.end, err
                ),
            ))
        })?;
        Ok(())
    }

    // Path to WAL file with specified id
    fn id_path(&self, id: u128) -> PathBuf {
        self.wal_path.join(id.to_string())
    }

    // Opens a file handle to current log; creates a new file if one doesn't exist
    // or if the new write would push the existing log over size capacity
    fn open_or_create_log(&mut self, write_size: usize) -> std::io::Result<fs::File> {
        // Check if write would push WAL size over limit
        if self.cur_id.is_none() || self.cur_shreds + write_size > self.max_shreds {
            // .as_millis() provides enough granularity to avoid filename collision;
            // observed collisions with .as_secs() with quick validator restart
            self.cur_id = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            );
            self.cur_shreds = 0;
            self.max_slots.insert(self.cur_id.unwrap(), 0);
            fs::File::create(self.id_path(self.cur_id.unwrap()))
        } else {
            fs::OpenOptions::new()
                .append(true)
                .open(self.id_path(self.cur_id.unwrap()))
        }
    }

    // Extract the range deletion information from an entry
    // The range should be treated as [start, end)
    fn decode_deletion_entry(buffer: &[u8]) -> Range<Slot> {
        let start = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
        let end = u64::from_le_bytes(buffer[8..16].try_into().unwrap());
        Range::<Slot> { start, end }
    }

    //
    fn encode_deletion_entry(range: &Range<Slot>) -> Vec<u8> {
        let mut entry = vec![0; WAL_ENTRY_SIZE];
        entry[0..8].copy_from_slice(&range.start.to_le_bytes());
        entry[8..16].copy_from_slice(&range.end.to_le_bytes());
        entry[SHRED_PAYLOAD_SIZE..WAL_ENTRY_SIZE]
            .copy_from_slice(&WAL_ENTRY_DELETION.to_le_bytes());
        entry
    }

    // Purge log files where the newest shreds in the log are
    // older than the max_purge_slot
    /*
    // TODO: plug this back in shortly
    pub fn purge_logs(&mut self, max_purge_slot: Slot) {
        let mut ids_to_purge = vec![];
        for (id, _) in self
            .max_slots
            .iter()
            .filter(|(_, max_slot)| **max_slot < max_purge_slot)
        {
            match self.cur_id {
                // Do not delete the current log file
                Some(cur_id) if *id != cur_id => {
                    // Collect which files are getting purged
                    ids_to_purge.push(cur_id);
                    let _ = fs::remove_file(self.id_path(*id));
                }
                _ => {}
            }
        }

        for id in ids_to_purge.iter() {
            self.max_slots.remove(id);
        }
    }
    */
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

    /// Purge an entire slot of data shreds
    pub(crate) fn purge_data_shreds(&self, slot: Slot) {
        // Remove from the cache; no issues if the slot had previously been flushed
        self.data_shred_cache.remove(&slot);
        // Could get errors such as file doesn't exist; we don't care so just eat the error
        let _ = fs::remove_file(self.slot_data_shreds_path(slot));
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
}
