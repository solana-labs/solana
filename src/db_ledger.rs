//! The `db_ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::entry::Entry;
use crate::genesis_block::GenesisBlock;
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};
use bincode::{deserialize, serialize};
use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DBRawIterator, Options, WriteBatch, DB};
use serde::de::DeserializeOwned;
use serde::Serialize;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::borrow::Borrow;
use std::cmp;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

pub type DbLedgerRawIterator = rocksdb::DBRawIterator;

pub const DB_LEDGER_DIRECTORY: &str = "rocksdb";
// A good value for this is the number of cores on the machine
const TOTAL_THREADS: i32 = 8;
const MAX_WRITE_BUFFER_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub enum DbLedgerError {
    BlobForIndexExists,
    InvalidBlobData,
    RocksDb(rocksdb::Error),
}

impl std::convert::From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Error {
        Error::DbLedgerError(DbLedgerError::RocksDb(e))
    }
}

pub trait LedgerColumnFamily {
    type ValueType: DeserializeOwned + Serialize;

    fn get(&self, key: &[u8]) -> Result<Option<Self::ValueType>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;

        if let Some(raw) = data_bytes {
            let result: Self::ValueType = deserialize(&raw)?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn get_bytes(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put_bytes(&self, key: &[u8], serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), &key, &serialized_value)?;
        Ok(())
    }

    fn put(&self, key: &[u8], value: &Self::ValueType) -> Result<()> {
        let db = self.db();
        let serialized = serialize(value)?;
        db.put_cf(self.handle(), &key, &serialized)?;
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        let db = self.db();
        db.delete_cf(self.handle(), &key)?;
        Ok(())
    }

    fn db(&self) -> &Arc<DB>;
    fn handle(&self) -> ColumnFamily;
}

pub trait LedgerColumnFamilyRaw {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.db();
        let data_bytes = db.get_cf(self.handle(), key)?;
        Ok(data_bytes.map(|x| x.to_vec()))
    }

    fn put(&self, key: &[u8], serialized_value: &[u8]) -> Result<()> {
        let db = self.db();
        db.put_cf(self.handle(), &key, &serialized_value)?;
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        let db = self.db();
        db.delete_cf(self.handle(), &key)?;
        Ok(())
    }

    fn raw_iterator(&self) -> DbLedgerRawIterator {
        let db = self.db();
        db.raw_iterator_cf(self.handle())
            .expect("Expected to be able to open database iterator")
    }

    fn handle(&self) -> ColumnFamily;
    fn db(&self) -> &Arc<DB>;
}

#[derive(Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
// The Meta column family
pub struct SlotMeta {
    // The total number of consecutive blob starting from index 0
    // we have received for this slot.
    pub consumed: u64,
    // The entry height of the highest blob received for this slot.
    pub received: u64,
    // The slot the blob with index == "consumed" is in
    pub consumed_slot: u64,
    // The slot the blob with index == "received" is in
    pub received_slot: u64,
}

impl SlotMeta {
    fn new() -> Self {
        SlotMeta {
            consumed: 0,
            received: 0,
            consumed_slot: 0,
            received_slot: 0,
        }
    }
}

pub struct MetaCf {
    db: Arc<DB>,
}

impl MetaCf {
    pub fn new(db: Arc<DB>) -> Self {
        MetaCf { db }
    }

    pub fn key(slot_height: u64) -> Vec<u8> {
        let mut key = vec![0u8; 8];
        BigEndian::write_u64(&mut key[0..8], slot_height);
        key
    }
}

impl LedgerColumnFamily for MetaCf {
    type ValueType = SlotMeta;

    fn db(&self) -> &Arc<DB> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(META_CF).unwrap()
    }
}

// The data column family
pub struct DataCf {
    db: Arc<DB>,
}

impl DataCf {
    pub fn new(db: Arc<DB>) -> Self {
        DataCf { db }
    }

    pub fn get_by_slot_index(&self, slot_height: u64, index: u64) -> Result<Option<Vec<u8>>> {
        let key = Self::key(slot_height, index);
        self.get(&key)
    }

    pub fn put_by_slot_index(
        &self,
        slot_height: u64,
        index: u64,
        serialized_value: &[u8],
    ) -> Result<()> {
        let key = Self::key(slot_height, index);
        self.put(&key, serialized_value)
    }

    pub fn key(slot_height: u64, index: u64) -> Vec<u8> {
        let mut key = vec![0u8; 16];
        BigEndian::write_u64(&mut key[0..8], slot_height);
        BigEndian::write_u64(&mut key[8..16], index);
        key
    }

    pub fn slot_height_from_key(key: &[u8]) -> Result<u64> {
        let mut rdr = io::Cursor::new(&key[0..8]);
        let height = rdr.read_u64::<BigEndian>()?;
        Ok(height)
    }

    pub fn index_from_key(key: &[u8]) -> Result<u64> {
        let mut rdr = io::Cursor::new(&key[8..16]);
        let index = rdr.read_u64::<BigEndian>()?;
        Ok(index)
    }
}

impl LedgerColumnFamilyRaw for DataCf {
    fn db(&self) -> &Arc<DB> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(DATA_CF).unwrap()
    }
}

// The erasure column family
pub struct ErasureCf {
    db: Arc<DB>,
}

impl ErasureCf {
    pub fn new(db: Arc<DB>) -> Self {
        ErasureCf { db }
    }
    pub fn delete_by_slot_index(&self, slot_height: u64, index: u64) -> Result<()> {
        let key = Self::key(slot_height, index);
        self.delete(&key)
    }

    pub fn get_by_slot_index(&self, slot_height: u64, index: u64) -> Result<Option<Vec<u8>>> {
        let key = Self::key(slot_height, index);
        self.get(&key)
    }

    pub fn put_by_slot_index(
        &self,
        slot_height: u64,
        index: u64,
        serialized_value: &[u8],
    ) -> Result<()> {
        let key = Self::key(slot_height, index);
        self.put(&key, serialized_value)
    }

    pub fn key(slot_height: u64, index: u64) -> Vec<u8> {
        DataCf::key(slot_height, index)
    }

    pub fn slot_height_from_key(key: &[u8]) -> Result<u64> {
        DataCf::slot_height_from_key(key)
    }

    pub fn index_from_key(key: &[u8]) -> Result<u64> {
        DataCf::index_from_key(key)
    }
}

impl LedgerColumnFamilyRaw for ErasureCf {
    fn db(&self) -> &Arc<DB> {
        &self.db
    }

    fn handle(&self) -> ColumnFamily {
        self.db.cf_handle(ERASURE_CF).unwrap()
    }
}

// ledger window
pub struct DbLedger {
    // Underlying database is automatically closed in the Drop implementation of DB
    db: Arc<DB>,
    meta_cf: MetaCf,
    data_cf: DataCf,
    erasure_cf: ErasureCf,
}

// TODO: Once we support a window that knows about different leader
// slots, change functions where this is used to take slot height
// as a variable argument
pub const DEFAULT_SLOT_HEIGHT: u64 = 0;
// Column family for metadata about a leader slot
pub const META_CF: &str = "meta";
// Column family for the data in a leader slot
pub const DATA_CF: &str = "data";
// Column family for erasure data
pub const ERASURE_CF: &str = "erasure";

impl DbLedger {
    // Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &str) -> Result<Self> {
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(DB_LEDGER_DIRECTORY);

        // Use default database options
        let db_options = Self::get_db_options();

        // Column family names
        let meta_cf_descriptor = ColumnFamilyDescriptor::new(META_CF, Self::get_cf_options());
        let data_cf_descriptor = ColumnFamilyDescriptor::new(DATA_CF, Self::get_cf_options());
        let erasure_cf_descriptor = ColumnFamilyDescriptor::new(ERASURE_CF, Self::get_cf_options());
        let cfs = vec![
            meta_cf_descriptor,
            data_cf_descriptor,
            erasure_cf_descriptor,
        ];

        // Open the database
        let db = Arc::new(DB::open_cf_descriptors(&db_options, ledger_path, cfs)?);

        // Create the metadata column family
        let meta_cf = MetaCf::new(db.clone());

        // Create the data column family
        let data_cf = DataCf::new(db.clone());

        // Create the erasure column family
        let erasure_cf = ErasureCf::new(db.clone());

        Ok(DbLedger {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
        })
    }

    pub fn meta(&self) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(&MetaCf::key(DEFAULT_SLOT_HEIGHT))
    }

    pub fn destroy(ledger_path: &str) -> Result<()> {
        // DB::destroy() fails if `ledger_path` doesn't exist
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(DB_LEDGER_DIRECTORY);
        DB::destroy(&Options::default(), &ledger_path)?;
        Ok(())
    }

    pub fn write_shared_blobs<I>(&self, shared_blobs: I) -> Result<Vec<Entry>>
    where
        I: IntoIterator,
        I::Item: Borrow<SharedBlob>,
    {
        let c_blobs: Vec<_> = shared_blobs
            .into_iter()
            .map(move |s| s.borrow().clone())
            .collect();

        let r_blobs: Vec<_> = c_blobs.iter().map(move |b| b.read().unwrap()).collect();

        let blobs = r_blobs.iter().map(|s| &**s);

        let new_entries = self.insert_data_blobs(blobs)?;
        Ok(new_entries)
    }

    pub fn write_blobs<'a, I>(&self, blobs: I) -> Result<Vec<Entry>>
    where
        I: IntoIterator,
        I::Item: Borrow<&'a Blob>,
    {
        let blobs = blobs.into_iter().map(|b| *b.borrow());
        let new_entries = self.insert_data_blobs(blobs)?;
        Ok(new_entries)
    }

    pub fn write_entries<I>(&self, slot: u64, index: u64, entries: I) -> Result<Vec<Entry>>
    where
        I: IntoIterator,
        I::Item: Borrow<Entry>,
    {
        let blobs: Vec<_> = entries
            .into_iter()
            .enumerate()
            .map(|(idx, entry)| {
                let mut b = entry.borrow().to_blob();
                b.set_index(idx as u64 + index).unwrap();
                b.set_slot(slot).unwrap();
                b
            })
            .collect();

        self.write_blobs(&blobs)
    }

    pub fn insert_data_blobs<I>(&self, new_blobs: I) -> Result<Vec<Entry>>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let mut new_blobs: Vec<_> = new_blobs.into_iter().collect();

        if new_blobs.is_empty() {
            return Ok(vec![]);
        }

        new_blobs.sort_unstable_by(|b1, b2| {
            b1.borrow()
                .index()
                .unwrap()
                .cmp(&b2.borrow().index().unwrap())
        });

        let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);

        let mut should_write_meta = false;

        let mut meta = {
            if let Some(meta) = self.db.get_cf(self.meta_cf.handle(), &meta_key)? {
                deserialize(&meta)?
            } else {
                should_write_meta = true;
                SlotMeta::new()
            }
        };

        // TODO: Handle if leader sends different blob for same index when the index > consumed
        // The old window implementation would just replace that index.
        let lowest_index = new_blobs[0].borrow().index()?;
        let lowest_slot = new_blobs[0].borrow().slot()?;
        let highest_index = new_blobs.last().unwrap().borrow().index()?;
        let highest_slot = new_blobs.last().unwrap().borrow().slot()?;
        if lowest_index < meta.consumed {
            return Err(Error::DbLedgerError(DbLedgerError::BlobForIndexExists));
        }

        // Index is zero-indexed, while the "received" height starts from 1,
        // so received = index + 1 for the same blob.
        if highest_index >= meta.received {
            meta.received = highest_index + 1;
            meta.received_slot = highest_slot;
            should_write_meta = true;
        }

        let mut consumed_queue = vec![];

        if meta.consumed == lowest_index {
            // Find the next consecutive block of blobs.
            // TODO: account for consecutive blocks that
            // span multiple slots
            should_write_meta = true;
            let mut index_into_blob = 0;
            let mut current_index = lowest_index;
            let mut current_slot = lowest_slot;
            'outer: loop {
                let entry: Entry = {
                    // Try to find the next blob we're looking for in the new_blobs
                    // vector
                    let mut found_blob = None;
                    while index_into_blob < new_blobs.len() {
                        let new_blob = new_blobs[index_into_blob].borrow();
                        let index = new_blob.index()?;

                        // Skip over duplicate blobs with the same index and continue
                        // until we either find the index we're looking for, or detect
                        // that the index doesn't exist in the new_blobs vector.
                        if index > current_index {
                            break;
                        }

                        index_into_blob += 1;

                        if index == current_index {
                            found_blob = Some(new_blob);
                        }
                    }

                    // If we found the blob in the new_blobs vector, process it, otherwise,
                    // look for the blob in the database.
                    if let Some(next_blob) = found_blob {
                        current_slot = next_blob.slot()?;
                        let serialized_entry_data = &next_blob.data
                            [BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + next_blob.size()?];
                        // Verify entries can actually be reconstructed
                        deserialize(serialized_entry_data).expect(
                            "Blob made it past validation, so must be deserializable at this point",
                        )
                    } else {
                        let key = DataCf::key(current_slot, current_index);
                        let blob_data = {
                            if let Some(blob_data) = self.data_cf.get(&key)? {
                                blob_data
                            } else if meta.consumed < meta.received {
                                let key = DataCf::key(current_slot + 1, current_index);
                                if let Some(blob_data) = self.data_cf.get(&key)? {
                                    current_slot += 1;
                                    blob_data
                                } else {
                                    break 'outer;
                                }
                            } else {
                                break 'outer;
                            }
                        };
                        deserialize(&blob_data[BLOB_HEADER_SIZE..])
                            .expect("Blobs in database must be deserializable")
                    }
                };

                consumed_queue.push(entry);
                current_index += 1;
                meta.consumed += 1;
                meta.consumed_slot = current_slot;
            }
        }

        // Commit Step: Atomic write both the metadata and the data
        let mut batch = WriteBatch::default();
        if should_write_meta {
            batch.put_cf(self.meta_cf.handle(), &meta_key, &serialize(&meta)?)?;
        }

        for blob in new_blobs {
            let blob = blob.borrow();
            let key = DataCf::key(blob.slot()?, blob.index()?);
            let serialized_blob_datas = &blob.data[..BLOB_HEADER_SIZE + blob.size()?];
            batch.put_cf(self.data_cf.handle(), &key, serialized_blob_datas)?;
        }

        self.db.write(batch)?;
        Ok(consumed_queue)
    }

    // Writes a list of sorted, consecutive broadcast blobs to the db_ledger
    pub fn write_consecutive_blobs(&self, blobs: &[SharedBlob]) -> Result<()> {
        assert!(!blobs.is_empty());

        let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);

        let mut meta = {
            if let Some(meta) = self.meta_cf.get(&meta_key)? {
                let first = blobs[0].read().unwrap();
                assert_eq!(meta.consumed, first.index()?);
                meta
            } else {
                SlotMeta::new()
            }
        };

        {
            let last = blobs.last().unwrap().read().unwrap();
            meta.consumed = last.index()? + 1;
            meta.consumed_slot = last.slot()?;
            meta.received = cmp::max(meta.received, last.index()? + 1);
            meta.received_slot = cmp::max(meta.received_slot, last.index()?);
        }

        let mut batch = WriteBatch::default();
        batch.put_cf(self.meta_cf.handle(), &meta_key, &serialize(&meta)?)?;
        for blob in blobs {
            let blob = blob.read().unwrap();
            let key = DataCf::key(blob.slot()?, blob.index()?);
            let serialized_blob_datas = &blob.data[..BLOB_HEADER_SIZE + blob.size()?];
            batch.put_cf(self.data_cf.handle(), &key, serialized_blob_datas)?;
        }
        self.db.write(batch)?;
        Ok(())
    }

    // Fill 'buf' with num_blobs or most number of consecutive
    // whole blobs that fit into buf.len()
    //
    // Return tuple of (number of blob read, total size of blobs read)
    pub fn read_blobs_bytes(
        &self,
        start_index: u64,
        num_blobs: u64,
        buf: &mut [u8],
        slot_height: u64,
    ) -> Result<(u64, u64)> {
        let start_key = DataCf::key(slot_height, start_index);
        let mut db_iterator = self.db.raw_iterator_cf(self.data_cf.handle())?;
        db_iterator.seek(&start_key);
        let mut total_blobs = 0;
        let mut total_current_size = 0;
        for expected_index in start_index..start_index + num_blobs {
            if !db_iterator.valid() {
                if expected_index == start_index {
                    return Err(Error::IO(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Blob at start_index not found",
                    )));
                } else {
                    break;
                }
            }

            // Check key is the next sequential key based on
            // blob index
            let key = &db_iterator.key().expect("Expected valid key");
            let index = DataCf::index_from_key(key)?;
            if index != expected_index {
                break;
            }

            // Get the blob data
            let value = &db_iterator.value();

            if value.is_none() {
                break;
            }

            let value = value.as_ref().unwrap();
            let blob_data_len = value.len();

            if total_current_size + blob_data_len > buf.len() {
                break;
            }

            buf[total_current_size..total_current_size + value.len()].copy_from_slice(value);
            total_current_size += blob_data_len;
            total_blobs += 1;

            // TODO: Change this logic to support looking for data
            // that spans multiple leader slots, once we support
            // a window that knows about different leader slots
            db_iterator.next();
        }

        Ok((total_blobs, total_current_size as u64))
    }

    /// Return an iterator for all the entries in the given file.
    pub fn read_ledger(&self) -> Result<impl Iterator<Item = Entry>> {
        let mut db_iterator = self.db.raw_iterator_cf(self.data_cf.handle())?;

        db_iterator.seek_to_first();
        Ok(EntryIterator {
            db_iterator,
            last_id: None,
        })
    }

    pub fn get_coding_blob_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.erasure_cf.get_by_slot_index(slot, index)
    }
    pub fn delete_coding_blob(&self, slot: u64, index: u64) -> Result<()> {
        self.erasure_cf.delete_by_slot_index(slot, index)
    }
    pub fn get_data_blob_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.data_cf.get_by_slot_index(slot, index)
    }
    pub fn put_coding_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.erasure_cf.put_by_slot_index(slot, index, bytes)
    }

    pub fn put_data_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.data_cf.put_by_slot_index(slot, index, bytes)
    }

    pub fn get_data_blob(&self, slot: u64, index: u64) -> Result<Option<Blob>> {
        let bytes = self.get_data_blob_bytes(slot, index)?;
        Ok(bytes.map(|bytes| {
            let blob = Blob::new(&bytes);
            assert!(blob.slot().unwrap() == slot);
            assert!(blob.index().unwrap() == index);
            blob
        }))
    }

    pub fn get_entries_bytes(
        &self,
        _start_index: u64,
        _num_entries: u64,
        _buf: &mut [u8],
    ) -> io::Result<(u64, u64)> {
        Err(io::Error::new(io::ErrorKind::Other, "TODO"))
    }

    // Given a start and end entry index, find all the missing
    // indexes in the ledger in the range [start_index, end_index)
    fn find_missing_indexes(
        db_iterator: &mut DbLedgerRawIterator,
        slot: u64,
        start_index: u64,
        end_index: u64,
        key: &dyn Fn(u64, u64) -> Vec<u8>,
        index_from_key: &dyn Fn(&[u8]) -> Result<u64>,
        max_missing: usize,
    ) -> Vec<u64> {
        if start_index >= end_index || max_missing == 0 {
            return vec![];
        }

        let mut missing_indexes = vec![];

        // Seek to the first blob with index >= start_index
        db_iterator.seek(&key(slot, start_index));

        // The index of the first missing blob in the slot
        let mut prev_index = start_index;
        'outer: loop {
            if !db_iterator.valid() {
                for i in prev_index..end_index {
                    missing_indexes.push(i);
                    if missing_indexes.len() == max_missing {
                        break;
                    }
                }
                break;
            }
            let current_key = db_iterator.key().expect("Expect a valid key");
            let current_index = index_from_key(&current_key)
                .expect("Expect to be able to parse index from valid key");
            let upper_index = cmp::min(current_index, end_index);
            for i in prev_index..upper_index {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break 'outer;
                }
            }
            if current_index >= end_index {
                break;
            }

            prev_index = current_index + 1;
            db_iterator.next();
        }

        missing_indexes
    }

    pub fn find_missing_data_indexes(
        &self,
        slot: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
    ) -> Vec<u64> {
        let mut db_iterator = self.data_cf.raw_iterator();

        Self::find_missing_indexes(
            &mut db_iterator,
            slot,
            start_index,
            end_index,
            &DataCf::key,
            &DataCf::index_from_key,
            max_missing,
        )
    }

    pub fn find_missing_coding_indexes(
        &self,
        slot: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
    ) -> Vec<u64> {
        let mut db_iterator = self.erasure_cf.raw_iterator();

        Self::find_missing_indexes(
            &mut db_iterator,
            slot,
            start_index,
            end_index,
            &ErasureCf::key,
            &ErasureCf::index_from_key,
            max_missing,
        )
    }

    fn get_cf_options() -> Options {
        let mut options = Options::default();
        options.set_max_write_buffer_number(32);
        options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE);
        options.set_max_bytes_for_level_base(MAX_WRITE_BUFFER_SIZE as u64);
        options
    }

    fn get_db_options() -> Options {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);
        options.increase_parallelism(TOTAL_THREADS);
        options.set_max_background_flushes(4);
        options.set_max_background_compactions(4);
        options.set_max_write_buffer_number(32);
        options.set_write_buffer_size(MAX_WRITE_BUFFER_SIZE);
        options.set_max_bytes_for_level_base(MAX_WRITE_BUFFER_SIZE as u64);
        options
    }
}

// TODO: all this goes away with EntryTree
struct EntryIterator {
    db_iterator: DBRawIterator,

    // TODO: remove me when replay_stage is iterating by block (EntryTree)
    //    this verification is duplicating that of replay_stage, which
    //    can do this in parallel
    last_id: Option<Hash>,
    // https://github.com/rust-rocksdb/rust-rocksdb/issues/234
    //   rocksdb issue: the _db_ledger member must be lower in the struct to prevent a crash
    //   when the db_iterator member above is dropped.
    //   _db_ledger is unused, but dropping _db_ledger results in a broken db_iterator
    //   you have to hold the database open in order to iterate over it, and in order
    //   for db_iterator to be able to run Drop
    //    _db_ledger: DbLedger,
}

impl Iterator for EntryIterator {
    type Item = Entry;

    fn next(&mut self) -> Option<Entry> {
        if self.db_iterator.valid() {
            if let Some(value) = self.db_iterator.value() {
                if let Ok(entry) = deserialize::<Entry>(&value[BLOB_HEADER_SIZE..]) {
                    if let Some(last_id) = self.last_id {
                        if !entry.verify(&last_id) {
                            return None;
                        }
                    }
                    self.db_iterator.next();
                    self.last_id = Some(entry.id);
                    return Some(entry);
                }
            }
        }
        None
    }
}

pub fn create_empty_ledger(ledger_path: &str, genesis_block: &GenesisBlock) -> Result<()> {
    DbLedger::destroy(ledger_path)?;
    DbLedger::open(ledger_path)?;
    genesis_block.write(&ledger_path)?;
    Ok(())
}

pub fn genesis<'a, I>(ledger_path: &str, keypair: &Keypair, entries: I) -> Result<()>
where
    I: IntoIterator<Item = &'a Entry>,
{
    let db_ledger = DbLedger::open(ledger_path)?;

    // TODO sign these blobs with keypair
    let blobs: Vec<_> = entries
        .into_iter()
        .enumerate()
        .map(|(idx, entry)| {
            let mut b = entry.borrow().to_blob();
            b.set_index(idx as u64).unwrap();
            b.set_id(&keypair.pubkey()).unwrap();
            b.set_slot(DEFAULT_SLOT_HEIGHT).unwrap();
            b
        })
        .collect();

    db_ledger.write_blobs(&blobs[..])?;
    Ok(())
}

pub fn get_tmp_ledger_path(name: &str) -> String {
    use std::env;
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    let path = format!("{}/tmp/ledger-{}-{}", out_dir, name, keypair.pubkey());

    // whack any possible collision
    let _ignored = fs::remove_dir_all(&path);

    path
}

pub fn create_tmp_ledger(name: &str, genesis_block: &GenesisBlock) -> String {
    let ledger_path = get_tmp_ledger_path(name);
    create_empty_ledger(&ledger_path, genesis_block).unwrap();
    ledger_path
}

pub fn create_tmp_genesis(
    name: &str,
    num: u64,
    bootstrap_leader_id: Pubkey,
    bootstrap_leader_tokens: u64,
) -> (GenesisBlock, Keypair, String) {
    let (genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(num, bootstrap_leader_id, bootstrap_leader_tokens);
    let ledger_path = create_tmp_ledger(name, &genesis_block);

    (genesis_block, mint_keypair, ledger_path)
}

pub fn create_tmp_sample_ledger(
    name: &str,
    num_tokens: u64,
    num_ending_ticks: u64,
    bootstrap_leader_id: Pubkey,
    bootstrap_leader_tokens: u64,
) -> (GenesisBlock, Keypair, String, Vec<Entry>) {
    let (genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(num_tokens, bootstrap_leader_id, bootstrap_leader_tokens);
    let path = get_tmp_ledger_path(name);
    create_empty_ledger(&path, &genesis_block).unwrap();

    let entries = crate::entry::create_ticks(num_ending_ticks, genesis_block.last_id());

    let db_ledger = DbLedger::open(&path).unwrap();
    db_ledger
        .write_entries(DEFAULT_SLOT_HEIGHT, 0, &entries)
        .unwrap();
    (genesis_block, mint_keypair, path, entries)
}

pub fn tmp_copy_ledger(from: &str, name: &str) -> String {
    let path = get_tmp_ledger_path(name);

    let db_ledger = DbLedger::open(from).unwrap();
    let ledger_entries = db_ledger.read_ledger().unwrap();
    let genesis_block = GenesisBlock::load(from).unwrap();

    DbLedger::destroy(&path).expect("Expected successful database destruction");
    let db_ledger = DbLedger::open(&path).unwrap();
    db_ledger
        .write_entries(DEFAULT_SLOT_HEIGHT, 0, ledger_entries)
        .unwrap();
    genesis_block.write(&path).unwrap();

    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{make_tiny_test_entries, make_tiny_test_entries_from_id, EntrySlice};
    use crate::packet::index_blobs;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_put_get_simple() {
        let ledger_path = get_tmp_ledger_path("test_put_get_simple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Test meta column family
        let meta = SlotMeta::new();
        let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
        ledger.meta_cf.put(&meta_key, &meta).unwrap();
        let result = ledger
            .meta_cf
            .get(&meta_key)
            .unwrap()
            .expect("Expected meta object to exist");

        assert_eq!(result, meta);

        // Test erasure column family
        let erasure = vec![1u8; 16];
        let erasure_key = ErasureCf::key(DEFAULT_SLOT_HEIGHT, 0);
        ledger.erasure_cf.put(&erasure_key, &erasure).unwrap();

        let result = ledger
            .erasure_cf
            .get(&erasure_key)
            .unwrap()
            .expect("Expected erasure object to exist");

        assert_eq!(result, erasure);

        // Test data column family
        let data = vec![2u8; 16];
        let data_key = DataCf::key(DEFAULT_SLOT_HEIGHT, 0);
        ledger.data_cf.put(&data_key, &data).unwrap();

        let result = ledger
            .data_cf
            .get(&data_key)
            .unwrap()
            .expect("Expected data object to exist");

        assert_eq!(result, data);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_read_blobs_bytes() {
        let shared_blobs = make_tiny_test_entries(10).to_shared_blobs();
        let slot = DEFAULT_SLOT_HEIGHT;
        index_blobs(&shared_blobs, &Keypair::new().pubkey(), 0, &[slot; 10]);

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_read_blobs_bytes");
        let ledger = DbLedger::open(&ledger_path).unwrap();
        ledger.write_blobs(&blobs).unwrap();

        let mut buf = [0; 1024];
        let (num_blobs, bytes) = ledger.read_blobs_bytes(0, 1, &mut buf, slot).unwrap();
        let bytes = bytes as usize;
        assert_eq!(num_blobs, 1);
        {
            let blob_data = &buf[..bytes];
            assert_eq!(blob_data, &blobs[0].data[..bytes]);
        }

        let (num_blobs, bytes2) = ledger.read_blobs_bytes(0, 2, &mut buf, slot).unwrap();
        let bytes2 = bytes2 as usize;
        assert_eq!(num_blobs, 2);
        assert!(bytes2 > bytes);
        {
            let blob_data_1 = &buf[..bytes];
            assert_eq!(blob_data_1, &blobs[0].data[..bytes]);

            let blob_data_2 = &buf[bytes..bytes2];
            assert_eq!(blob_data_2, &blobs[1].data[..bytes2 - bytes]);
        }

        // buf size part-way into blob[1], should just return blob[0]
        let mut buf = vec![0; bytes + 1];
        let (num_blobs, bytes3) = ledger.read_blobs_bytes(0, 2, &mut buf, slot).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes3 = bytes3 as usize;
        assert_eq!(bytes3, bytes);

        let mut buf = vec![0; bytes2 - 1];
        let (num_blobs, bytes4) = ledger.read_blobs_bytes(0, 2, &mut buf, slot).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes4 = bytes4 as usize;
        assert_eq!(bytes4, bytes);

        let mut buf = vec![0; bytes * 2];
        let (num_blobs, bytes6) = ledger.read_blobs_bytes(9, 1, &mut buf, slot).unwrap();
        assert_eq!(num_blobs, 1);
        let bytes6 = bytes6 as usize;

        {
            let blob_data = &buf[..bytes6];
            assert_eq!(blob_data, &blobs[9].data[..bytes6]);
        }

        // Read out of range
        assert!(ledger.read_blobs_bytes(20, 2, &mut buf, slot).is_err());

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_basic() {
        let entries = make_tiny_test_entries(2);
        let shared_blobs = entries.to_shared_blobs();

        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64).unwrap();
        }

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_basic");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert second blob, we're missing the first blob, so should return nothing
        let result = ledger.insert_data_blobs(vec![blobs[1]]).unwrap();

        assert!(result.len() == 0);
        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(DEFAULT_SLOT_HEIGHT))
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == 2);

        // Insert first blob, check for consecutive returned entries
        let result = ledger.insert_data_blobs(vec![blobs[0]]).unwrap();

        assert_eq!(result, entries);

        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(DEFAULT_SLOT_HEIGHT))
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert!(meta.consumed == 2 && meta.received == 2);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_multiple() {
        let num_blobs = 10;
        let entries = make_tiny_test_entries(num_blobs);
        let shared_blobs = entries.to_shared_blobs();
        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64).unwrap();
        }
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_multiple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert blobs in reverse, check for consecutive returned blobs
        for i in (0..num_blobs).rev() {
            let result = ledger.insert_data_blobs(vec![blobs[i]]).unwrap();

            let meta = ledger
                .meta_cf
                .get(&MetaCf::key(DEFAULT_SLOT_HEIGHT))
                .unwrap()
                .expect("Expected metadata object to exist");
            if i != 0 {
                assert_eq!(result.len(), 0);
                assert!(meta.consumed == 0 && meta.received == num_blobs as u64);
            } else {
                assert_eq!(result, entries);
                assert!(meta.consumed == num_blobs as u64 && meta.received == num_blobs as u64);
            }
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_slots() {
        let num_blobs = 10;
        let entries = make_tiny_test_entries(num_blobs);
        let shared_blobs = entries.to_shared_blobs();
        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64).unwrap();
        }
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_slots");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert last blob into next slot
        let result = ledger
            .insert_data_blobs(vec![*blobs.last().unwrap()])
            .unwrap();
        assert_eq!(result.len(), 0);

        // Insert blobs into first slot, check for consecutive blobs
        for i in (0..num_blobs - 1).rev() {
            let result = ledger.insert_data_blobs(vec![blobs[i]]).unwrap();
            let meta = ledger
                .meta_cf
                .get(&MetaCf::key(DEFAULT_SLOT_HEIGHT))
                .unwrap()
                .expect("Expected metadata object to exist");
            if i != 0 {
                assert_eq!(result.len(), 0);
                assert!(meta.consumed == 0 && meta.received == num_blobs as u64);
            } else {
                assert_eq!(result, entries);
                assert!(meta.consumed == num_blobs as u64 && meta.received == num_blobs as u64);
            }
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_iteration_order() {
        let slot = 0;
        let db_ledger_path = get_tmp_ledger_path("test_iteration_order");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Write entries
            let num_entries = 8;
            let entries = make_tiny_test_entries(num_entries);
            let shared_blobs = entries.to_shared_blobs();

            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(1 << (i * 8)).unwrap();
                w_b.set_slot(DEFAULT_SLOT_HEIGHT).unwrap();
            }

            assert_eq!(
                db_ledger
                    .write_shared_blobs(&shared_blobs)
                    .expect("Expected successful write of blobs"),
                vec![]
            );
            let mut db_iterator = db_ledger
                .db
                .raw_iterator_cf(db_ledger.data_cf.handle())
                .expect("Expected to be able to open database iterator");

            db_iterator.seek(&DataCf::key(slot, 1));

            // Iterate through ledger
            for i in 0..num_entries {
                assert!(db_iterator.valid());
                let current_key = db_iterator.key().expect("Expected a valid key");
                let current_index = DataCf::index_from_key(&current_key)
                    .expect("Expect to be able to parse index from valid key");
                assert_eq!(current_index, (1 as u64) << (i * 8));
                db_iterator.next();
            }
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_bulk() {
        let db_ledger_path = get_tmp_ledger_path("test_insert_data_blobs_bulk");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Write entries
            let num_entries = 20 as u64;
            let original_entries = make_tiny_test_entries(num_entries as usize);
            let shared_blobs = original_entries.clone().to_shared_blobs();
            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(i as u64).unwrap();
                w_b.set_slot(i as u64).unwrap();
            }

            assert_eq!(
                db_ledger
                    .write_shared_blobs(shared_blobs.iter().skip(1).step_by(2))
                    .unwrap(),
                vec![]
            );

            assert_eq!(
                db_ledger
                    .write_shared_blobs(shared_blobs.iter().step_by(2))
                    .unwrap(),
                original_entries
            );

            let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed_slot, num_entries - 1);
            assert_eq!(meta.received_slot, num_entries - 1);
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_duplicate() {
        // Create RocksDb ledger
        let db_ledger_path = get_tmp_ledger_path("test_insert_data_blobs_duplicate");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Write entries
            let num_entries = 10 as u64;
            let num_duplicates = 2;
            let original_entries: Vec<Entry> = make_tiny_test_entries(num_entries as usize)
                .into_iter()
                .flat_map(|e| vec![e; num_duplicates])
                .collect();

            let shared_blobs = original_entries.clone().to_shared_blobs();
            for (i, b) in shared_blobs.iter().enumerate() {
                let index = (i / 2) as u64;
                let mut w_b = b.write().unwrap();
                w_b.set_index(index).unwrap();
                w_b.set_slot(index).unwrap();
            }

            assert_eq!(
                db_ledger
                    .write_shared_blobs(
                        shared_blobs
                            .iter()
                            .skip(num_duplicates)
                            .step_by(num_duplicates * 2)
                    )
                    .unwrap(),
                vec![]
            );

            let expected: Vec<_> = original_entries
                .into_iter()
                .step_by(num_duplicates)
                .collect();

            assert_eq!(
                db_ledger
                    .write_shared_blobs(shared_blobs.iter().step_by(num_duplicates * 2))
                    .unwrap(),
                expected,
            );

            let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed_slot, num_entries - 1);
            assert_eq!(meta.received_slot, num_entries - 1);
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_write_consecutive_blobs() {
        let db_ledger_path = get_tmp_ledger_path("test_write_consecutive_blobs");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Write entries
            let num_entries = 20 as u64;
            let original_entries = make_tiny_test_entries(num_entries as usize);
            let shared_blobs = original_entries.to_shared_blobs();
            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(i as u64).unwrap();
                w_b.set_slot(i as u64).unwrap();
            }

            db_ledger
                .write_consecutive_blobs(&shared_blobs)
                .expect("Expect successful blob writes");

            let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed_slot, num_entries - 1);
            assert_eq!(meta.received_slot, num_entries - 1);

            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(num_entries + i as u64).unwrap();
                w_b.set_slot(num_entries + i as u64).unwrap();
            }

            db_ledger
                .write_consecutive_blobs(&shared_blobs)
                .expect("Expect successful blob writes");

            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, 2 * num_entries);
            assert_eq!(meta.received, 2 * num_entries);
            assert_eq!(meta.consumed_slot, 2 * num_entries - 1);
            assert_eq!(meta.received_slot, 2 * num_entries - 1);
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_genesis_and_entry_iterator() {
        let entries = make_tiny_test_entries_from_id(&Hash::default(), 10);

        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            genesis(&ledger_path, &Keypair::new(), &entries).unwrap();

            let ledger = DbLedger::open(&ledger_path).expect("open failed");

            let read_entries: Vec<Entry> =
                ledger.read_ledger().expect("read_ledger failed").collect();
            assert!(read_entries.verify(&Hash::default()));
            assert_eq!(entries, read_entries);
        }

        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }
    #[test]
    pub fn test_entry_iterator_up_to_consumed() {
        let entries = make_tiny_test_entries_from_id(&Hash::default(), 3);
        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            // put entries except last 2 into ledger
            genesis(&ledger_path, &Keypair::new(), &entries[..entries.len() - 2]).unwrap();

            let ledger = DbLedger::open(&ledger_path).expect("open failed");

            // now write the last entry, ledger has a hole in it one before the end
            // +-+-+-+-+-+-+-+    +-+
            // | | | | | | | |    | |
            // +-+-+-+-+-+-+-+    +-+
            ledger
                .write_entries(
                    0u64,
                    (entries.len() - 1) as u64,
                    &entries[entries.len() - 1..],
                )
                .unwrap();

            let read_entries: Vec<Entry> =
                ledger.read_ledger().expect("read_ledger failed").collect();
            assert!(read_entries.verify(&Hash::default()));

            // enumeration should stop at the hole
            assert_eq!(entries[..entries.len() - 2].to_vec(), read_entries);
        }

        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

}
