//! Write Ahead Log (WAL) for shred storage
//! For general information on a WAL (Write Ahead Log), see
//! https://en.wikipedia.org/wiki/Write-ahead_logging
//!
//! This WAL will record shred operations, namely, insertions and deletions. In the event of a
//! system crash / reboot, the WAL will be played back to establish the state that was lost in
//! memory. As shreds are more permanently persisted to non-volatile storage in the database,
//! individual log files in the WAL will become unnecessary and can be cleaned up.
//!
//! With the existence of delete operations, order matters and the WAL must replayed in the same
//! order that it was written.

use super::*;
use crate::shred::SHRED_PAYLOAD_SIZE;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{cmp, fs, io::Read, io::Write, ops::Range};

pub const SHRED_WAL_DIRECTORY: &str = "log";

pub struct ShredWAL {
    // Directory where WAL file(s) will be stored
    wal_path: PathBuf,
    // The maximum number of shreds allowed in a single WAL file
    max_shreds: usize,
    // The number of shreds written to current WAL file
    num_cur_shreds: usize,
    // ID to current WAL file
    cur_id: u128,
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
        // New cur_id queried each time validator restarted; use .as_micros() to avoid
        // filename collision from same timestamp if validator restarts very quickly
        let cur_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros();
        let mut max_slots = BTreeMap::new();
        max_slots.insert(cur_id, 0);
        fs::create_dir_all(&wal_path)?;

        let wal = Self {
            wal_path,
            max_shreds,
            num_cur_shreds: 0,
            cur_id,
            max_slots,
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
        let mut log = self.open_log(shreds.len())?;
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
        self.num_cur_shreds += shreds.len();
        let cur_max_slot = *self.max_slots.get(&self.cur_id).unwrap();
        self.max_slots
            .insert(self.cur_id, cmp::max(cur_max_slot, insert_max_slot));
        Ok(())
    }

    // Mark the specified range of slots as purged in the log.
    // std::ops::Range is of the format [start, end)
    pub fn log_shred_purge(&mut self, range: Range<Slot>) -> Result<()> {
        let entry = Self::encode_deletion_entry(&range);
        // A purge entry occupies the space of one shred
        let mut log = self.open_log(1)?;
        log.write_all(&entry).map_err(|err| {
            BlockstoreError::Io(IoError::new(
                ErrorKind::Other,
                format!(
                    "Unable to log purge from slot {} to slot {} in wal: {}",
                    range.start, range.end, err
                ),
            ))
        })?;
        self.num_cur_shreds += 1;
        Ok(())
    }

    // Purge log files that contain shreds in slot(s) <= lowest_cleanup_slot
    // The strategy here is to clear log files based on chronological order; however,
    // purges may not necessarily clear the oldest chronological data. As such, this
    // method should be used by caller that knows when lowest_cleanup_slot has advanced
    pub fn purge_logs(&mut self, lowest_cleanup_slot: Slot) {
        // Can't modify max_slots while iterating so update a duplicate structure
        // as we iterate and then replace at the end
        let mut updated_max_slots = self.max_slots.clone();
        for (id, _) in self
            .max_slots
            .iter()
            .filter(|(_, max_slot)| **max_slot <= lowest_cleanup_slot)
        {
            if *id != self.cur_id {
                updated_max_slots.remove(id);
                let _ = fs::remove_file(self.id_path(*id));
            }
        }
        self.max_slots = updated_max_slots;
    }

    // Path to WAL file with specified id
    fn id_path(&self, id: u128) -> PathBuf {
        self.wal_path.join(id.to_string())
    }

    // Generate a new log ID and update metadata accordingly
    // Log ID's will be strictly monotonically increasing
    fn update_log_id(&mut self) {
        self.cur_id += 1;
        self.num_cur_shreds = 0;
        self.max_slots.insert(self.cur_id, 0);
        trace!("New shred wal id: {}", self.cur_id);
    }

    // Returns file handle to current log; creates a new log if
    // - A log does not yet exist
    // - The write would push current log over max capacity; write_size is assumed
    //   to be smaller than self.max_shreds. If this weren't the case, write_size
    //   would dictate how large a log might be with current implemtnation
    fn open_log(&mut self, write_size: usize) -> std::io::Result<fs::File> {
        if self.num_cur_shreds + write_size > self.max_shreds {
            self.update_log_id();
        }
        fs::OpenOptions::new()
            // .create(true) creates a new file, or opens if it already exists
            .create(true)
            // .append(true) ensures any contents in existing log are kept
            .append(true)
            .open(self.id_path(self.cur_id))
    }

    // Extract the range deletion information from an entry
    // std::ops::Range is of the format [start, end)
    fn decode_deletion_entry(buffer: &[u8]) -> Range<Slot> {
        let start = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
        let end = u64::from_le_bytes(buffer[8..16].try_into().unwrap());
        Range::<Slot> { start, end }
    }

    // Encode a deletion range into an entry
    // std::ops::Range is of the format [start, end)
    fn encode_deletion_entry(range: &Range<Slot>) -> Vec<u8> {
        let mut entry = vec![0; WAL_ENTRY_SIZE];
        entry[0..8].copy_from_slice(&range.start.to_le_bytes());
        entry[8..16].copy_from_slice(&range.end.to_le_bytes());
        entry[SHRED_PAYLOAD_SIZE..WAL_ENTRY_SIZE]
            .copy_from_slice(&WAL_ENTRY_DELETION.to_le_bytes());
        entry
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::get_tmp_ledger_path;

    fn test_flatten_recovered_shreds(
        recovered_shreds: RecoveredShreds,
    ) -> HashMap<(u64, u64), Shred> {
        let mut result = HashMap::new();
        for (slot, shreds) in recovered_shreds.into_iter() {
            for shred in shreds {
                result.insert((slot, shred.index().into()), shred);
            }
        }
        result
    }

    #[test]
    fn test_wal_write_and_recover() {
        solana_logger::setup();
        // Construct the shreds
        let entries_per_slot = 25;
        let num_slots = 3;
        let (mut shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);
        // Keep a copy of shreds to check against at end
        let all_shreds: HashMap<(u64, u64), Shred> = shreds
            .iter()
            .cloned()
            .map(|shred| ((shred.slot(), shred.index().into()), shred))
            .collect();
        let shreds_per_slot = shreds.len() / num_slots as usize;

        // Construct ShredWAL with log files sized to fit one slot
        let ledger_path = get_tmp_ledger_path!();
        let mut shred_wal = ShredWAL::new(&ledger_path, shreds_per_slot).unwrap();

        // Insert the shreds, one slot per write
        for _ in 0..num_slots {
            let write_shreds: HashMap<(u64, u64), Shred> = shreds
                .drain(0..shreds_per_slot)
                .collect::<Vec<_>>()
                .into_iter()
                .map(|shred| ((shred.slot(), shred.index().into()), shred))
                .collect();
            shred_wal.log_shred_write(write_shreds).unwrap();
        }

        // Read the shreds back from logs and ensure everything we inserted is present
        let recoverd_shreds = test_flatten_recovered_shreds(shred_wal.recover().unwrap());
        assert_eq!(all_shreds.len(), recoverd_shreds.len());
        for (_, shred) in all_shreds.iter() {
            assert_eq!(
                shred,
                recoverd_shreds
                    .get(&(shred.slot(), shred.index().into()))
                    .unwrap()
            );
        }

        drop(shred_wal);
        fs::remove_dir_all(&ledger_path).unwrap();
    }

    #[test]
    fn test_wal_purge() {
        solana_logger::setup();
        // Construct the shreds
        let entries_per_slot = 25;
        let num_slots = 16;
        let (mut shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);
        // Keep a copy of shreds to check against at end
        let all_shreds: HashMap<(u64, u64), Shred> = shreds
            .iter()
            .cloned()
            .map(|shred| ((shred.slot(), shred.index().into()), shred))
            .collect();
        let shreds_per_slot = shreds.len() / num_slots as usize;

        // Construct ShredWAL with log files sized to fit one slot
        let ledger_path = get_tmp_ledger_path!();
        let mut shred_wal = ShredWAL::new(&ledger_path, 4 * shreds_per_slot).unwrap();

        // Insert half the shreds, two slots per write
        // (num_slots / 4) / (2 * shreds_per_slot) = 1/2 slots
        for _ in 0..(num_slots / 4) {
            let write_shreds: HashMap<(u64, u64), Shred> = shreds
                .drain(0..2 * shreds_per_slot)
                .collect::<Vec<_>>()
                .into_iter()
                .map(|shred| ((shred.slot(), shred.index() as u64), shred))
                .collect();
            shred_wal.log_shred_write(write_shreds).unwrap();
        }

        // Purge slots [0, 4), slots [4, 7] remain
        shred_wal.log_shred_purge(0..4).unwrap();

        // Insert the second half of the shreds, two slots per write
        // (num_slots / 4) / (2 * shreds_per_slot) = 1/2 slots
        for _ in 0..(num_slots / 4) {
            let write_shreds: HashMap<(u64, u64), Shred> = shreds
                .drain(0..2 * shreds_per_slot)
                .collect::<Vec<_>>()
                .into_iter()
                .map(|shred| ((shred.slot(), shred.index().into()), shred))
                .collect();
            shred_wal.log_shred_write(write_shreds).unwrap();
        }

        // Purge slots [6, 9) and [14, 16); slots [4, 5], [9, 13] remain
        shred_wal.log_shred_purge(6..9).unwrap();
        shred_wal.log_shred_purge(14..16).unwrap();

        // Read the shreds back from logs and ensure everything we inserted is present
        let recoverd_shreds = test_flatten_recovered_shreds(shred_wal.recover().unwrap());
        for ((slot, index), shred) in all_shreds.iter() {
            match slot {
                4 | 5 | 9..=13 => {
                    assert_eq!(shred, recoverd_shreds.get(&(*slot, *index)).unwrap());
                }
                _ => {
                    assert!(recoverd_shreds.get(&(*slot, *index)).is_none());
                }
            }
        }

        drop(shred_wal);
        fs::remove_dir_all(&ledger_path).unwrap();
    }

    #[test]
    fn test_wal_encode_decode_deletion_entry() {
        let range = 13..17;
        let entry = ShredWAL::encode_deletion_entry(&range);
        let decoded_range = ShredWAL::decode_deletion_entry(&entry);
        assert_eq!(range, decoded_range);
    }

    // TODO: test ShredWAL::purge_logs(&mut self, last_flush_slot: Slot) {
}
