//! The `block_tree` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::entry::Entry;
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};
use bincode::{deserialize, serialize};
use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use hashbrown::HashMap;
use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBRawIterator, IteratorMode, Options, WriteBatch, DB,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp;
use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;

pub type BlocktreeRawIterator = rocksdb::DBRawIterator;

pub const BLOCKTREE_DIRECTORY: &str = "rocksdb";
// A good value for this is the number of cores on the machine
const TOTAL_THREADS: i32 = 8;
const MAX_WRITE_BUFFER_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub enum BlocktreeError {
    BlobForIndexExists,
    InvalidBlobData,
    RocksDb(rocksdb::Error),
}

impl std::convert::From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Error {
        Error::BlocktreeError(BlocktreeError::RocksDb(e))
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

    fn raw_iterator(&self) -> BlocktreeRawIterator {
        let db = self.db();
        db.raw_iterator_cf(self.handle())
            .expect("Expected to be able to open database iterator")
    }

    fn handle(&self) -> ColumnFamily;
    fn db(&self) -> &Arc<DB>;
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
// The Meta column family
pub struct SlotMeta {
    pub slot_height: u64,
    // The total number of consecutive blob starting from index 0
    // we have received for this slot.
    pub consumed: u64,
    // The entry height of the highest blob received for this slot.
    pub received: u64,
    // The index of the blob that is flagged as the last blob for this slot
    pub last_index: u64,
    // The parent slot of this slot
    pub parent_slot: u64,
    // The list of slots that chains to this slot
    pub next_slots: Vec<u64>,
    // True if every block from 0..slot, where slot is the slot index of this slot
    // is full
    pub is_trunk: bool,
}

impl SlotMeta {
    pub fn is_full(&self) -> bool {
        self.consumed > self.last_index
    }

    fn new(slot_height: u64, parent_slot: u64) -> Self {
        SlotMeta {
            slot_height,
            consumed: 0,
            received: 0,
            parent_slot,
            next_slots: vec![],
            is_trunk: slot_height == 0,
            last_index: std::u64::MAX,
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

    pub fn get_slot_meta(&self, slot_height: u64) -> Result<Option<SlotMeta>> {
        let key = Self::key(slot_height);
        self.get(&key)
    }

    pub fn put_slot_meta(&self, slot_height: u64, slot_meta: &SlotMeta) -> Result<()> {
        let key = Self::key(slot_height);
        self.put(&key, slot_meta)
    }

    pub fn index_from_key(key: &[u8]) -> Result<u64> {
        let mut rdr = io::Cursor::new(&key[..]);
        let index = rdr.read_u64::<BigEndian>()?;
        Ok(index)
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
pub struct Blocktree {
    // Underlying database is automatically closed in the Drop implementation of DB
    db: Arc<DB>,
    meta_cf: MetaCf,
    data_cf: DataCf,
    erasure_cf: ErasureCf,
    new_blobs_signals: Vec<SyncSender<bool>>,
    ticks_per_slot: u64,
}

// Column family for metadata about a leader slot
pub const META_CF: &str = "meta";
// Column family for the data in a leader slot
pub const DATA_CF: &str = "data";
// Column family for erasure data
pub const ERASURE_CF: &str = "erasure";

impl Blocktree {
    // Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &str) -> Result<Self> {
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(BLOCKTREE_DIRECTORY);

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

        let ticks_per_slot = DEFAULT_TICKS_PER_SLOT;
        Ok(Blocktree {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
            new_blobs_signals: vec![],
            ticks_per_slot,
        })
    }

    pub fn open_with_signal(ledger_path: &str) -> Result<(Self, Receiver<bool>)> {
        let mut blocktree = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        blocktree.new_blobs_signals = vec![signal_sender];

        Ok((blocktree, signal_receiver))
    }

    pub fn open_config(ledger_path: &str, ticks_per_slot: u64) -> Result<Self> {
        let mut blocktree = Self::open(ledger_path)?;
        blocktree.ticks_per_slot = ticks_per_slot;
        Ok(blocktree)
    }

    pub fn open_with_config_signal(
        ledger_path: &str,
        ticks_per_slot: u64,
    ) -> Result<(Self, Receiver<bool>)> {
        let mut blocktree = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        blocktree.new_blobs_signals = vec![signal_sender];
        blocktree.ticks_per_slot = ticks_per_slot;

        Ok((blocktree, signal_receiver))
    }

    pub fn meta(&self, slot_height: u64) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(&MetaCf::key(slot_height))
    }

    pub fn destroy(ledger_path: &str) -> Result<()> {
        // DB::destroy() fails if `ledger_path` doesn't exist
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(BLOCKTREE_DIRECTORY);
        DB::destroy(&Options::default(), &ledger_path)?;
        Ok(())
    }

    pub fn get_next_slot(&self, slot_height: u64) -> Result<Option<u64>> {
        let mut db_iterator = self.db.raw_iterator_cf(self.meta_cf.handle())?;
        db_iterator.seek(&MetaCf::key(slot_height + 1));
        if !db_iterator.valid() {
            Ok(None)
        } else {
            let key = &db_iterator.key().expect("Expected valid key");
            Ok(Some(MetaCf::index_from_key(&key)?))
        }
    }

    pub fn write_shared_blobs<I>(&self, shared_blobs: I) -> Result<()>
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

        self.insert_data_blobs(blobs)
    }

    pub fn write_blobs<I>(&self, blobs: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        self.insert_data_blobs(blobs)
    }

    pub fn write_entries<I>(
        &self,
        start_slot: u64,
        num_ticks_in_start_slot: u64,
        start_index: u64,
        entries: I,
    ) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Entry>,
    {
        let ticks_per_slot = self.ticks_per_slot;

        assert!(num_ticks_in_start_slot < ticks_per_slot);
        let mut remaining_ticks_in_slot = ticks_per_slot - num_ticks_in_start_slot;

        let mut blobs = vec![];
        let mut current_index = start_index;
        let mut current_slot = start_slot;
        let mut parent_slot = {
            if current_slot == 0 {
                current_slot
            } else {
                current_slot - 1
            }
        };
        // Find all the entries for start_slot
        for entry in entries {
            if remaining_ticks_in_slot == 0 {
                current_slot += 1;
                current_index = 0;
                parent_slot = current_slot - 1;
                remaining_ticks_in_slot = ticks_per_slot;
            }

            let mut b = entry.borrow().to_blob();

            if entry.borrow().is_tick() {
                remaining_ticks_in_slot -= 1;
                if remaining_ticks_in_slot == 0 {
                    b.set_is_last_in_slot();
                }
            }

            b.set_index(current_index);
            b.set_slot(current_slot);
            b.set_parent(parent_slot);
            blobs.push(b);

            current_index += 1;
        }

        self.write_blobs(&blobs)
    }

    pub fn insert_data_blobs<I>(&self, new_blobs: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let mut write_batch = WriteBatch::default();
        // A map from slot_height to a 2-tuple of metadata: (working copy, backup copy),
        // so we can detect changes to the slot metadata later
        let mut slot_meta_working_set = HashMap::new();
        let new_blobs: Vec<_> = new_blobs.into_iter().collect();
        let mut prev_inserted_blob_datas = HashMap::new();

        for blob in new_blobs.iter() {
            let blob = blob.borrow();
            let blob_slot = blob.slot();
            let parent_slot = blob.parent();

            // Check if we've already inserted the slot metadata for this blob's slot
            let entry = slot_meta_working_set.entry(blob_slot).or_insert_with(|| {
                // Store a 2-tuple of the metadata (working copy, backup copy)
                if let Some(mut meta) = self
                    .meta(blob_slot)
                    .expect("Expect database get to succeed")
                {
                    // If parent_slot == std::u64::MAX, then this is one of the dummy metadatas inserted
                    // during the chaining process, see the function find_slot_meta_in_cached_state()
                    // for details
                    if meta.parent_slot == std::u64::MAX {
                        meta.parent_slot = parent_slot;
                        // Set backup as None so that all the logic for inserting new slots
                        // still runs, as this placeholder slot is essentially equivalent to
                        // inserting a new slot
                        (Rc::new(RefCell::new(meta.clone())), None)
                    } else {
                        (Rc::new(RefCell::new(meta.clone())), Some(meta))
                    }
                } else {
                    (
                        Rc::new(RefCell::new(SlotMeta::new(blob_slot, parent_slot))),
                        None,
                    )
                }
            });

            let slot_meta = &mut entry.0.borrow_mut();

            // This slot is full, skip the bogus blob
            if slot_meta.is_full() {
                continue;
            }

            let _ = self.insert_data_blob(
                blob,
                &mut prev_inserted_blob_datas,
                slot_meta,
                &mut write_batch,
            );
        }

        // Handle chaining for the working set
        self.handle_chaining(&mut write_batch, &slot_meta_working_set)?;
        let mut should_signal = false;

        // Check if any metadata was changed, if so, insert the new version of the
        // metadata into the write batch
        for (slot_height, (meta_copy, meta_backup)) in slot_meta_working_set.iter() {
            let meta: &SlotMeta = &RefCell::borrow(&*meta_copy);
            // Check if the working copy of the metadata has changed
            if Some(meta) != meta_backup.as_ref() {
                should_signal = should_signal || Self::slot_has_updates(meta, &meta_backup);
                write_batch.put_cf(
                    self.meta_cf.handle(),
                    &MetaCf::key(*slot_height),
                    &serialize(&meta)?,
                )?;
            }
        }

        self.db.write(write_batch)?;
        if should_signal {
            for signal in self.new_blobs_signals.iter() {
                let _ = signal.try_send(true);
            }
        }

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

    pub fn read_ledger_blobs(&self) -> impl Iterator<Item = Blob> {
        self.db
            .iterator_cf(self.data_cf.handle(), IteratorMode::Start)
            .unwrap()
            .map(|(_, blob_data)| Blob::new(&blob_data))
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

    pub fn put_data_raw(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.data_cf.put(key, value)
    }

    pub fn put_data_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.data_cf.put_by_slot_index(slot, index, bytes)
    }

    pub fn get_data_blob(&self, slot_height: u64, blob_index: u64) -> Result<Option<Blob>> {
        let bytes = self.get_data_blob_bytes(slot_height, blob_index)?;
        Ok(bytes.map(|bytes| {
            let blob = Blob::new(&bytes);
            assert!(blob.slot() == slot_height);
            assert!(blob.index() == blob_index);
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
    // for the slot with slot_height == slot
    fn find_missing_indexes(
        db_iterator: &mut BlocktreeRawIterator,
        slot: u64,
        start_index: u64,
        end_index: u64,
        key: &dyn Fn(u64, u64) -> Vec<u8>,
        slot_height_from_key: &dyn Fn(&[u8]) -> Result<u64>,
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
            let current_slot = slot_height_from_key(&current_key)
                .expect("Expect to be able to parse slot from valid key");
            let current_index = {
                if current_slot > slot {
                    end_index
                } else {
                    index_from_key(&current_key)
                        .expect("Expect to be able to parse index from valid key")
                }
            };
            let upper_index = cmp::min(current_index, end_index);

            for i in prev_index..upper_index {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break 'outer;
                }
            }

            if current_slot > slot {
                break;
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
            &DataCf::slot_height_from_key,
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
            &ErasureCf::slot_height_from_key,
            &ErasureCf::index_from_key,
            max_missing,
        )
    }
    /// Returns the entry vector for the slot starting with `blob_start_index`
    pub fn get_slot_entries(
        &self,
        slot_height: u64,
        blob_start_index: u64,
        max_entries: Option<u64>,
    ) -> Result<Vec<Entry>> {
        // Find the next consecutive block of blobs.
        let consecutive_blobs = self.get_slot_consecutive_blobs(
            slot_height,
            &HashMap::new(),
            blob_start_index,
            max_entries,
        )?;
        Ok(Self::deserialize_blobs(&consecutive_blobs))
    }

    // Returns slots connecting to any element of the list `slot_heights`.
    pub fn get_slots_since(&self, slot_heights: &[u64]) -> Result<Vec<u64>> {
        // Return error if there was a database error during lookup of any of the
        // slot indexes
        let slots: Result<Vec<Option<SlotMeta>>> = slot_heights
            .iter()
            .map(|slot_height| self.meta(*slot_height))
            .collect();

        let slots = slots?;
        let slots: Vec<_> = slots
            .into_iter()
            .filter_map(|x| x)
            .flat_map(|x| x.next_slots)
            .collect();

        Ok(slots)
    }

    fn deserialize_blobs<I>(blob_datas: &[I]) -> Vec<Entry>
    where
        I: Borrow<[u8]>,
    {
        blob_datas
            .iter()
            .map(|blob_data| {
                let serialized_entry_data = &blob_data.borrow()[BLOB_HEADER_SIZE..];
                let entry: Entry = deserialize(serialized_entry_data)
                    .expect("Ledger should only contain well formed data");
                entry
            })
            .collect()
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

    fn slot_has_updates(slot_meta: &SlotMeta, slot_meta_backup: &Option<SlotMeta>) -> bool {
        // We should signal that there are updates if we extended the chain of consecutive blocks starting
        // from block 0, which is true iff:
        // 1) The block with index prev_block_index is itself part of the trunk of consecutive blocks
        // starting from block 0,
        slot_meta.is_trunk &&
        // AND either:
        // 1) The slot didn't exist in the database before, and now we have a consecutive
        // block for that slot
        ((slot_meta_backup.is_none() && slot_meta.consumed != 0) ||
        // OR
        // 2) The slot did exist, but now we have a new consecutive block for that slot
        (slot_meta_backup.is_some() && slot_meta_backup.as_ref().unwrap().consumed != slot_meta.consumed))
    }

    // Chaining based on latest discussion here: https://github.com/solana-labs/solana/pull/2253
    fn handle_chaining(
        &self,
        write_batch: &mut WriteBatch,
        working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    ) -> Result<()> {
        let mut new_chained_slots = HashMap::new();
        let working_set_slot_heights: Vec<_> = working_set.iter().map(|s| *s.0).collect();
        for slot_height in working_set_slot_heights {
            self.handle_chaining_for_slot(working_set, &mut new_chained_slots, slot_height)?;
        }

        // Write all the newly changed slots in new_chained_slots to the write_batch
        for (slot_height, meta_copy) in new_chained_slots.iter() {
            let meta: &SlotMeta = &RefCell::borrow(&*meta_copy);
            write_batch.put_cf(
                self.meta_cf.handle(),
                &MetaCf::key(*slot_height),
                &serialize(meta)?,
            )?;
        }
        Ok(())
    }

    fn handle_chaining_for_slot(
        &self,
        working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot_height: u64,
    ) -> Result<()> {
        let (meta_copy, meta_backup) = working_set
            .get(&slot_height)
            .expect("Slot must exist in the working_set hashmap");
        {
            let mut slot_meta = meta_copy.borrow_mut();

            // If:
            // 1) This is a new slot
            // 2) slot_height != 0
            // then try to chain this slot to a previous slot
            if slot_height != 0 {
                let prev_slot_height = slot_meta.parent_slot;

                // Check if slot_meta is a new slot
                if meta_backup.is_none() {
                    let prev_slot = self.find_slot_meta_else_create(
                        working_set,
                        new_chained_slots,
                        prev_slot_height,
                    )?;

                    // This is a newly inserted slot so:
                    // 1) Chain to the previous slot, and also
                    // 2) Determine whether to set the is_trunk flag
                    self.chain_new_slot_to_prev_slot(
                        &mut prev_slot.borrow_mut(),
                        slot_height,
                        &mut slot_meta,
                    );
                }
            }
        }

        if self.is_newly_completed_slot(&RefCell::borrow(&*meta_copy), meta_backup)
            && RefCell::borrow(&*meta_copy).is_trunk
        {
            // This is a newly inserted slot and slot.is_trunk is true, so go through
            // and update all child slots with is_trunk if applicable
            let mut next_slots: Vec<(u64, Rc<RefCell<(SlotMeta)>>)> =
                vec![(slot_height, meta_copy.clone())];
            while !next_slots.is_empty() {
                let (_, current_slot) = next_slots.pop().unwrap();
                current_slot.borrow_mut().is_trunk = true;

                let current_slot = &RefCell::borrow(&*current_slot);
                if current_slot.is_full() {
                    for next_slot_index in current_slot.next_slots.iter() {
                        let next_slot = self.find_slot_meta_else_create(
                            working_set,
                            new_chained_slots,
                            *next_slot_index,
                        )?;
                        next_slots.push((*next_slot_index, next_slot));
                    }
                }
            }
        }

        Ok(())
    }

    fn chain_new_slot_to_prev_slot(
        &self,
        prev_slot: &mut SlotMeta,
        current_slot_height: u64,
        current_slot: &mut SlotMeta,
    ) {
        prev_slot.next_slots.push(current_slot_height);
        current_slot.is_trunk = prev_slot.is_trunk && prev_slot.is_full();
    }

    fn is_newly_completed_slot(
        &self,
        slot_meta: &SlotMeta,
        backup_slot_meta: &Option<SlotMeta>,
    ) -> bool {
        slot_meta.is_full()
            && (backup_slot_meta.is_none()
                || slot_meta.consumed != backup_slot_meta.as_ref().unwrap().consumed)
    }

    // 1) Find the slot metadata in the cache of dirty slot metadata we've previously touched,
    // else:
    // 2) Search the database for that slot metadata. If still no luck, then
    // 3) Create a dummy placeholder slot in the database
    fn find_slot_meta_else_create<'a>(
        &self,
        working_set: &'a HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        chained_slots: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot_index: u64,
    ) -> Result<Rc<RefCell<SlotMeta>>> {
        let result = self.find_slot_meta_in_cached_state(working_set, chained_slots, slot_index)?;
        if let Some(slot) = result {
            Ok(slot)
        } else {
            self.find_slot_meta_in_db_else_create(slot_index, chained_slots)
        }
    }

    // Search the database for that slot metadata. If still no luck, then
    // create a dummy placeholder slot in the database
    fn find_slot_meta_in_db_else_create<'a>(
        &self,
        slot_height: u64,
        insert_map: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    ) -> Result<Rc<RefCell<SlotMeta>>> {
        if let Some(slot) = self.meta(slot_height)? {
            insert_map.insert(slot_height, Rc::new(RefCell::new(slot)));
            Ok(insert_map.get(&slot_height).unwrap().clone())
        } else {
            // If this slot doesn't exist, make a placeholder slot. This way we
            // remember which slots chained to this one when we eventually get a real blob
            // for this slot
            insert_map.insert(
                slot_height,
                Rc::new(RefCell::new(SlotMeta::new(slot_height, std::u64::MAX))),
            );
            Ok(insert_map.get(&slot_height).unwrap().clone())
        }
    }

    // Find the slot metadata in the cache of dirty slot metadata we've previously touched
    fn find_slot_meta_in_cached_state<'a>(
        &self,
        working_set: &'a HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        chained_slots: &'a HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot_height: u64,
    ) -> Result<Option<Rc<RefCell<SlotMeta>>>> {
        if let Some((entry, _)) = working_set.get(&slot_height) {
            Ok(Some(entry.clone()))
        } else if let Some(entry) = chained_slots.get(&slot_height) {
            Ok(Some(entry.clone()))
        } else {
            Ok(None)
        }
    }

    /// Insert a blob into ledger, updating the slot_meta if necessary
    fn insert_data_blob<'a>(
        &self,
        blob_to_insert: &'a Blob,
        prev_inserted_blob_datas: &mut HashMap<(u64, u64), &'a [u8]>,
        slot_meta: &mut SlotMeta,
        write_batch: &mut WriteBatch,
    ) -> Result<()> {
        let blob_index = blob_to_insert.index();
        let blob_slot = blob_to_insert.slot();
        let blob_size = blob_to_insert.size();

        if blob_index < slot_meta.consumed
            || prev_inserted_blob_datas.contains_key(&(blob_slot, blob_index))
        {
            return Err(Error::BlocktreeError(BlocktreeError::BlobForIndexExists));
        }

        let new_consumed = {
            if slot_meta.consumed == blob_index {
                let blob_datas = self.get_slot_consecutive_blobs(
                    blob_slot,
                    prev_inserted_blob_datas,
                    // Don't start looking for consecutive blobs at blob_index,
                    // because we haven't inserted/committed the new blob_to_insert
                    // into the database or prev_inserted_blob_datas hashmap yet.
                    blob_index + 1,
                    None,
                )?;

                // Add one because we skipped this current blob when calling
                // get_slot_consecutive_blobs() earlier
                slot_meta.consumed + blob_datas.len() as u64 + 1
            } else {
                slot_meta.consumed
            }
        };

        let key = DataCf::key(blob_slot, blob_index);
        let serialized_blob_data = &blob_to_insert.data[..BLOB_HEADER_SIZE + blob_size];

        // Commit step: commit all changes to the mutable structures at once, or none at all.
        // We don't want only some of these changes going through.
        write_batch.put_cf(self.data_cf.handle(), &key, serialized_blob_data)?;
        prev_inserted_blob_datas.insert((blob_slot, blob_index), serialized_blob_data);
        // Index is zero-indexed, while the "received" height starts from 1,
        // so received = index + 1 for the same blob.
        slot_meta.received = cmp::max(blob_index + 1, slot_meta.received);
        slot_meta.consumed = new_consumed;
        slot_meta.last_index = {
            // If the last slot hasn't been set before, then
            // set it to this blob index
            if slot_meta.last_index == std::u64::MAX {
                if blob_to_insert.is_last_in_slot() {
                    blob_index
                } else {
                    std::u64::MAX
                }
            } else {
                slot_meta.last_index
            }
        };
        Ok(())
    }

    /// Returns the next consumed index and the number of ticks in the new consumed
    /// range
    fn get_slot_consecutive_blobs<'a>(
        &self,
        slot_height: u64,
        prev_inserted_blob_datas: &HashMap<(u64, u64), &'a [u8]>,
        mut current_index: u64,
        max_blobs: Option<u64>,
    ) -> Result<Vec<Cow<'a, [u8]>>> {
        let mut blobs: Vec<Cow<[u8]>> = vec![];
        loop {
            if Some(blobs.len() as u64) == max_blobs {
                break;
            }
            // Try to find the next blob we're looking for in the prev_inserted_blob_datas
            if let Some(prev_blob_data) =
                prev_inserted_blob_datas.get(&(slot_height, current_index))
            {
                blobs.push(Cow::Borrowed(*prev_blob_data));
            } else if let Some(blob_data) =
                self.data_cf.get_by_slot_index(slot_height, current_index)?
            {
                // Try to find the next blob we're looking for in the database
                blobs.push(Cow::Owned(blob_data));
            } else {
                break;
            }

            current_index += 1;
        }

        Ok(blobs)
    }

    // Handle special case of writing genesis blobs. For instance, the first two entries
    // don't count as ticks, even if they're empty entries
    fn write_genesis_blobs(&self, blobs: &[Blob]) -> Result<()> {
        // TODO: change bootstrap height to number of slots
        let meta_key = MetaCf::key(0);
        let mut bootstrap_meta = SlotMeta::new(0, 1);
        let last = blobs.last().unwrap();

        bootstrap_meta.consumed = last.index() + 1;
        bootstrap_meta.received = last.index() + 1;
        bootstrap_meta.is_trunk = true;

        let mut batch = WriteBatch::default();
        batch.put_cf(
            self.meta_cf.handle(),
            &meta_key,
            &serialize(&bootstrap_meta)?,
        )?;
        for blob in blobs {
            let key = DataCf::key(blob.slot(), blob.index());
            let serialized_blob_datas = &blob.data[..BLOB_HEADER_SIZE + blob.size()];
            batch.put_cf(self.data_cf.handle(), &key, serialized_blob_datas)?;
        }
        self.db.write(batch)?;
        Ok(())
    }
}

// TODO: all this goes away with Blocktree
struct EntryIterator {
    db_iterator: DBRawIterator,

    // TODO: remove me when replay_stage is iterating by block (Blocktree)
    //    this verification is duplicating that of replay_stage, which
    //    can do this in parallel
    last_id: Option<Hash>,
    // https://github.com/rust-rocksdb/rust-rocksdb/issues/234
    //   rocksdb issue: the _blocktree member must be lower in the struct to prevent a crash
    //   when the db_iterator member above is dropped.
    //   _blocktree is unused, but dropping _blocktree results in a broken db_iterator
    //   you have to hold the database open in order to iterate over it, and in order
    //   for db_iterator to be able to run Drop
    //    _blocktree: Blocktree,
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

// Creates a new ledger with slot 0 full of ticks (and only ticks).
//
// Returns the last_id that can be used to start slot 1 entries with.
pub fn create_new_ledger(ledger_path: &str, genesis_block: &GenesisBlock) -> Result<Hash> {
    let ticks_per_slot = genesis_block.ticks_per_slot;
    Blocktree::destroy(ledger_path)?;
    genesis_block.write(&ledger_path)?;

    // Fill slot 0 with ticks that link back to the genesis_block to bootstrap the ledger.
    let blocktree = Blocktree::open_config(ledger_path, ticks_per_slot)?;
    let entries = crate::entry::create_ticks(genesis_block.ticks_per_slot, genesis_block.last_id());
    blocktree.write_entries(0, 0, 0, &entries)?;

    Ok(entries.last().unwrap().id)
}

pub fn genesis<'a, I>(ledger_path: &str, entries: I) -> Result<()>
where
    I: IntoIterator<Item = &'a Entry>,
{
    let blocktree = Blocktree::open(ledger_path)?;

    // TODO sign these blobs with keypair
    let blobs: Vec<_> = entries
        .into_iter()
        .enumerate()
        .map(|(idx, entry)| {
            let mut b = entry.borrow().to_blob();
            b.set_index(idx as u64);
            b.forward(true);
            b.set_slot(0);
            b
        })
        .collect();

    blocktree.write_genesis_blobs(&blobs[..])?;
    Ok(())
}

#[macro_export]
macro_rules! tmp_ledger_name {
    () => {
        &format!("{}-{}", file!(), line!())
    };
}

#[macro_export]
macro_rules! get_tmp_ledger_path {
    () => {
        get_tmp_ledger_path(tmp_ledger_name!())
    };
}

pub fn get_tmp_ledger_path(name: &str) -> String {
    use std::env;
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    let path = format!("{}/tmp/ledger/{}-{}", out_dir, name, keypair.pubkey());

    // whack any possible collision
    let _ignored = fs::remove_dir_all(&path);

    path
}

#[macro_export]
macro_rules! create_new_tmp_ledger {
    ($genesis_block:expr) => {
        create_new_tmp_ledger(tmp_ledger_name!(), $genesis_block)
    };
}

// Same as `create_new_ledger()` but use a temporary ledger name based on the provided `name`
//
// Note: like `create_new_ledger` the returned ledger will have slot 0 full of ticks (and only
// ticks)
pub fn create_new_tmp_ledger(name: &str, genesis_block: &GenesisBlock) -> (String, Hash) {
    let ledger_path = get_tmp_ledger_path(name);
    let last_id = create_new_ledger(&ledger_path, genesis_block).unwrap();
    (ledger_path, last_id)
}

#[macro_export]
macro_rules! tmp_copy_blocktree {
    ($from:expr) => {
        tmp_copy_blocktree($from, tmp_ledger_name!())
    };
}

pub fn tmp_copy_blocktree(from: &str, name: &str) -> String {
    let path = get_tmp_ledger_path(name);

    let blocktree = Blocktree::open(from).unwrap();
    let blobs = blocktree.read_ledger_blobs();
    let genesis_block = GenesisBlock::load(from).unwrap();

    Blocktree::destroy(&path).expect("Expected successful database destruction");
    let blocktree = Blocktree::open(&path).unwrap();
    blocktree.write_blobs(blobs).unwrap();
    genesis_block.write(&path).unwrap();

    path
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::entry::{
        create_ticks, make_tiny_test_entries, make_tiny_test_entries_from_id, Entry, EntrySlice,
    };
    use crate::packet::index_blobs;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use solana_sdk::hash::Hash;
    use std::cmp::min;
    use std::collections::HashSet;
    use std::iter::once;
    use std::time::Duration;

    #[test]
    fn test_write_entries() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let ticks_per_slot = 10;
            let num_slots = 10;
            let num_ticks = ticks_per_slot * num_slots;
            let ledger = Blocktree::open_config(&ledger_path, ticks_per_slot).unwrap();

            let ticks = create_ticks(num_ticks, Hash::default());
            ledger.write_entries(0, 0, 0, ticks.clone()).unwrap();

            for i in 0..num_slots {
                let meta = ledger.meta(i).unwrap().unwrap();
                assert_eq!(meta.consumed, ticks_per_slot);
                assert_eq!(meta.received, ticks_per_slot);
                assert_eq!(meta.last_index, ticks_per_slot - 1);
                if i == num_slots - 1 {
                    assert!(meta.next_slots.is_empty());
                } else {
                    assert_eq!(meta.next_slots, vec![i + 1]);
                }
                if i == 0 {
                    assert_eq!(meta.parent_slot, 0);
                } else {
                    assert_eq!(meta.parent_slot, i - 1);
                }

                assert_eq!(
                    &ticks[(i * ticks_per_slot) as usize..((i + 1) * ticks_per_slot) as usize],
                    &ledger.get_slot_entries(i, 0, None).unwrap()[..]
                );
            }

            // Simulate writing to the end of a slot with existing ticks
            ledger
                .write_entries(
                    num_slots,
                    ticks_per_slot - 1,
                    ticks_per_slot - 2,
                    &ticks[0..2],
                )
                .unwrap();

            let meta = ledger.meta(num_slots).unwrap().unwrap();
            assert_eq!(meta.consumed, 0);
            // received blob was ticks_per_slot - 2, so received should be ticks_per_slot - 2 + 1
            assert_eq!(meta.received, ticks_per_slot - 1);
            // last blob index ticks_per_slot - 2 because that's the blob that made tick_height == ticks_per_slot
            // for the slot
            assert_eq!(meta.last_index, ticks_per_slot - 2);
            assert_eq!(meta.parent_slot, num_slots - 1);
            assert_eq!(meta.next_slots, vec![num_slots + 1]);
            assert_eq!(
                &ticks[0..1],
                &ledger
                    .get_slot_entries(num_slots, ticks_per_slot - 2, None)
                    .unwrap()[..]
            );

            // We wrote two entries, the second should spill into slot num_slots + 1
            let meta = ledger.meta(num_slots + 1).unwrap().unwrap();
            assert_eq!(meta.consumed, 1);
            assert_eq!(meta.received, 1);
            assert_eq!(meta.last_index, std::u64::MAX);
            assert_eq!(meta.parent_slot, num_slots);
            assert!(meta.next_slots.is_empty());

            assert_eq!(
                &ticks[1..2],
                &ledger.get_slot_entries(num_slots + 1, 0, None).unwrap()[..]
            );
        }
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_put_get_simple() {
        let ledger_path = get_tmp_ledger_path("test_put_get_simple");
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Test meta column family
        let meta = SlotMeta::new(0, 1);
        let meta_key = MetaCf::key(0);
        ledger.meta_cf.put(&meta_key, &meta).unwrap();
        let result = ledger
            .meta_cf
            .get(&meta_key)
            .unwrap()
            .expect("Expected meta object to exist");

        assert_eq!(result, meta);

        // Test erasure column family
        let erasure = vec![1u8; 16];
        let erasure_key = ErasureCf::key(0, 0);
        ledger.erasure_cf.put(&erasure_key, &erasure).unwrap();

        let result = ledger
            .erasure_cf
            .get(&erasure_key)
            .unwrap()
            .expect("Expected erasure object to exist");

        assert_eq!(result, erasure);

        // Test data column family
        let data = vec![2u8; 16];
        let data_key = DataCf::key(0, 0);
        ledger.data_cf.put(&data_key, &data).unwrap();

        let result = ledger
            .data_cf
            .get(&data_key)
            .unwrap()
            .expect("Expected data object to exist");

        assert_eq!(result, data);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_read_blobs_bytes() {
        let shared_blobs = make_tiny_test_entries(10).to_shared_blobs();
        let slot = 0;
        index_blobs(&shared_blobs, &mut 0, slot);

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_read_blobs_bytes");
        let ledger = Blocktree::open(&ledger_path).unwrap();
        ledger.write_blobs(blobs.clone()).unwrap();

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
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_basic() {
        let num_entries = 5;
        assert!(num_entries > 1);

        let (blobs, entries) = make_slot_entries(0, 0, num_entries);

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_basic");
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert last blob, we're missing the other blobs, so no consecutive
        // blobs starting from slot 0, index 0 should exist.
        ledger
            .insert_data_blobs(once(&blobs[num_entries as usize - 1]))
            .unwrap();
        assert!(ledger.get_slot_entries(0, 0, None).unwrap().is_empty());

        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(0))
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == num_entries);

        // Insert the other blobs, check for consecutive returned entries
        ledger
            .insert_data_blobs(&blobs[0..(num_entries - 1) as usize])
            .unwrap();
        let result = ledger.get_slot_entries(0, 0, None).unwrap();

        assert_eq!(result, entries);

        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(0))
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert_eq!(meta.consumed, num_entries);
        assert_eq!(meta.received, num_entries);
        assert_eq!(meta.parent_slot, 0);
        assert_eq!(meta.last_index, num_entries - 1);
        assert!(meta.next_slots.is_empty());
        assert!(meta.is_trunk);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_reverse() {
        let num_entries = 10;
        let (blobs, entries) = make_slot_entries(0, 0, num_entries);

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_reverse");
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert blobs in reverse, check for consecutive returned blobs
        for i in (0..num_entries).rev() {
            ledger.insert_data_blobs(once(&blobs[i as usize])).unwrap();
            let result = ledger.get_slot_entries(0, 0, None).unwrap();

            let meta = ledger
                .meta_cf
                .get(&MetaCf::key(0))
                .unwrap()
                .expect("Expected metadata object to exist");
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_entries - 1);
            if i != 0 {
                assert_eq!(result.len(), 0);
                assert!(meta.consumed == 0 && meta.received == num_entries as u64);
            } else {
                assert_eq!(result, entries);
                assert!(meta.consumed == num_entries as u64 && meta.received == num_entries as u64);
            }
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_slots() {
        test_insert_data_blobs_slots("test_insert_data_blobs_slots_single", false);
        test_insert_data_blobs_slots("test_insert_data_blobs_slots_bulk", true);
    }

    #[test]
    pub fn test_iteration_order() {
        let slot = 0;
        let blocktree_path = get_tmp_ledger_path("test_iteration_order");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Write entries
            let num_entries = 8;
            let entries = make_tiny_test_entries(num_entries);
            let shared_blobs = entries.to_shared_blobs();

            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(1 << (i * 8));
                w_b.set_slot(0);
            }

            blocktree
                .write_shared_blobs(&shared_blobs)
                .expect("Expected successful write of blobs");

            let mut db_iterator = blocktree
                .db
                .raw_iterator_cf(blocktree.data_cf.handle())
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
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slot_entries1() {
        let blocktree_path = get_tmp_ledger_path("test_get_slot_entries1");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let entries = make_tiny_test_entries(8);
            let mut blobs = entries.clone().to_blobs();
            for (i, b) in blobs.iter_mut().enumerate() {
                b.set_slot(1);
                if i < 4 {
                    b.set_index(i as u64);
                } else {
                    b.set_index(8 + i as u64);
                }
            }
            blocktree
                .write_blobs(&blobs)
                .expect("Expected successful write of blobs");

            assert_eq!(
                blocktree.get_slot_entries(1, 2, None).unwrap()[..],
                entries[2..4],
            );

            assert_eq!(
                blocktree.get_slot_entries(1, 12, None).unwrap()[..],
                entries[4..],
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slot_entries2() {
        let blocktree_path = get_tmp_ledger_path("test_get_slot_entries2");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Write entries
            let num_slots = 5 as u64;
            let mut index = 0;
            for slot_height in 0..num_slots {
                let entries = make_tiny_test_entries(slot_height as usize + 1);
                let last_entry = entries.last().unwrap().clone();
                let mut blobs = entries.clone().to_blobs();
                for b in blobs.iter_mut() {
                    b.set_index(index);
                    b.set_slot(slot_height as u64);
                    index += 1;
                }
                blocktree
                    .write_blobs(&blobs)
                    .expect("Expected successful write of blobs");
                assert_eq!(
                    blocktree
                        .get_slot_entries(slot_height, index - 1, None)
                        .unwrap(),
                    vec![last_entry],
                );
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_consecutive() {
        let blocktree_path = get_tmp_ledger_path("test_insert_data_blobs_consecutive");
        {
            let blocktree = Blocktree::open_config(&blocktree_path, 32).unwrap();
            let slot = 0;
            let parent_slot = 0;
            // Write entries
            let num_entries = 21 as u64;
            let (blobs, original_entries) = make_slot_entries(slot, parent_slot, num_entries);

            blocktree
                .write_blobs(blobs.iter().skip(1).step_by(2))
                .unwrap();

            assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), vec![]);

            let meta_key = MetaCf::key(slot);
            let meta = blocktree.meta_cf.get(&meta_key).unwrap().unwrap();
            if num_entries % 2 == 0 {
                assert_eq!(meta.received, num_entries);
            } else {
                assert_eq!(meta.received, num_entries - 1);
            }
            assert_eq!(meta.consumed, 0);
            assert_eq!(meta.parent_slot, 0);
            if num_entries % 2 == 0 {
                assert_eq!(meta.last_index, num_entries - 1);
            } else {
                assert_eq!(meta.last_index, std::u64::MAX);
            }

            blocktree.write_blobs(blobs.iter().step_by(2)).unwrap();

            assert_eq!(
                blocktree.get_slot_entries(0, 0, None).unwrap(),
                original_entries,
            );

            let meta_key = MetaCf::key(slot);
            let meta = blocktree.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_entries - 1);
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_duplicate() {
        // Create RocksDb ledger
        let blocktree_path = get_tmp_ledger_path("test_insert_data_blobs_duplicate");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Make duplicate entries and blobs
            let num_duplicates = 2;
            let num_unique_entries = 10;
            let (original_entries, blobs) = {
                let (blobs, entries) = make_slot_entries(0, 0, num_unique_entries);
                let entries: Vec<_> = entries
                    .into_iter()
                    .flat_map(|e| vec![e.clone(), e])
                    .collect();
                let blobs: Vec<_> = blobs.into_iter().flat_map(|b| vec![b.clone(), b]).collect();
                (entries, blobs)
            };

            blocktree
                .write_blobs(
                    blobs
                        .iter()
                        .skip(num_duplicates as usize)
                        .step_by(num_duplicates as usize * 2),
                )
                .unwrap();

            assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), vec![]);

            blocktree
                .write_blobs(blobs.iter().step_by(num_duplicates as usize * 2))
                .unwrap();

            let expected: Vec<_> = original_entries
                .into_iter()
                .step_by(num_duplicates as usize)
                .collect();

            assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), expected,);

            let meta_key = MetaCf::key(0);
            let meta = blocktree.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, num_unique_entries);
            assert_eq!(meta.received, num_unique_entries);
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_unique_entries - 1);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_genesis_and_entry_iterator() {
        let entries = make_tiny_test_entries_from_id(&Hash::default(), 10);

        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            genesis(&ledger_path, &entries).unwrap();

            let ledger = Blocktree::open(&ledger_path).expect("open failed");

            let read_entries: Vec<Entry> =
                ledger.read_ledger().expect("read_ledger failed").collect();
            assert!(read_entries.verify(&Hash::default()));
            assert_eq!(entries, read_entries);
        }

        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }
    #[test]
    pub fn test_entry_iterator_up_to_consumed() {
        let entries = make_tiny_test_entries_from_id(&Hash::default(), 3);
        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            // put entries except last 2 into ledger
            genesis(&ledger_path, &entries[..entries.len() - 2]).unwrap();

            let ledger = Blocktree::open(&ledger_path).expect("open failed");

            // now write the last entry, ledger has a hole in it one before the end
            // +-+-+-+-+-+-+-+    +-+
            // | | | | | | | |    | |
            // +-+-+-+-+-+-+-+    +-+
            ledger
                .write_entries(
                    0u64,
                    0,
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

        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_new_blobs_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_new_blobs_signal");
        let (ledger, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;
        // Create entries for slot 0
        let (blobs, _) = make_slot_entries(0, 0, entries_per_slot);

        // Insert second blob, but we're missing the first blob, so no consecutive
        // blobs starting from slot 0, index 0 should exist.
        ledger.insert_data_blobs(once(&blobs[1])).unwrap();
        let timer = Duration::new(1, 0);
        assert!(recvr.recv_timeout(timer).is_err());
        // Insert first blob, now we've made a consecutive block
        ledger.insert_data_blobs(once(&blobs[0])).unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());
        // Insert the rest of the ticks
        ledger
            .insert_data_blobs(&blobs[1..entries_per_slot as usize])
            .unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Create some other slots, and send batches of ticks for each slot such that each slot
        // is missing the tick at blob index == slot index - 1. Thus, no consecutive blocks
        // will be formed
        let num_slots = entries_per_slot;
        let mut blobs: Vec<Blob> = vec![];
        let mut missing_blobs = vec![];
        for slot_height in 1..num_slots + 1 {
            let (mut slot_blobs, _) =
                make_slot_entries(slot_height, slot_height - 1, entries_per_slot);
            let missing_blob = slot_blobs.remove(slot_height as usize - 1);
            blobs.extend(slot_blobs);
            missing_blobs.push(missing_blob);
        }

        // Should be no updates, since no new chains from block 0 were formed
        ledger.insert_data_blobs(blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // Insert a blob for each slot that doesn't make a consecutive block, we
        // should get no updates
        let blobs: Vec<_> = (1..num_slots + 1)
            .flat_map(|slot_height| {
                let (mut blob, _) = make_slot_entries(slot_height, slot_height - 1, 1);
                blob[0].set_index(2 * num_slots as u64);
                blob
            })
            .collect();

        ledger.insert_data_blobs(blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // For slots 1..num_slots/2, fill in the holes in one batch insertion,
        // so we should only get one signal
        ledger
            .insert_data_blobs(&missing_blobs[..(num_slots / 2) as usize])
            .unwrap();
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Fill in the holes for each of the remaining slots, we should get a single update
        // for each
        for missing_blob in &missing_blobs[(num_slots / 2) as usize..] {
            ledger
                .insert_data_blobs(vec![missing_blob.clone()])
                .unwrap();
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_handle_chaining_basic() {
        let blocktree_path = get_tmp_ledger_path("test_handle_chaining_basic");
        {
            let entries_per_slot = 2;
            let num_slots = 3;
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Construct the blobs
            let (blobs, _) = make_many_slot_entries(0, num_slots, entries_per_slot);

            // 1) Write to the first slot
            blocktree
                .write_blobs(&blobs[entries_per_slot as usize..2 * entries_per_slot as usize])
                .unwrap();
            let s1 = blocktree.meta(1).unwrap().unwrap();
            assert!(s1.next_slots.is_empty());
            // Slot 1 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s1.is_trunk);
            assert_eq!(s1.parent_slot, 0);
            assert_eq!(s1.last_index, entries_per_slot - 1);

            // 2) Write to the second slot
            blocktree
                .write_blobs(&blobs[2 * entries_per_slot as usize..3 * entries_per_slot as usize])
                .unwrap();
            let s2 = blocktree.meta(2).unwrap().unwrap();
            assert!(s2.next_slots.is_empty());
            // Slot 2 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s2.is_trunk);
            assert_eq!(s2.parent_slot, 1);
            assert_eq!(s2.last_index, entries_per_slot - 1);

            // Check the first slot again, it should chain to the second slot,
            // but still isn't part of the trunk
            let s1 = blocktree.meta(1).unwrap().unwrap();
            assert_eq!(s1.next_slots, vec![2]);
            assert!(!s1.is_trunk);
            assert_eq!(s1.parent_slot, 0);
            assert_eq!(s1.last_index, entries_per_slot - 1);

            // 3) Write to the zeroth slot, check that every slot
            // is now part of the trunk
            blocktree
                .write_blobs(&blobs[0..entries_per_slot as usize])
                .unwrap();
            for i in 0..3 {
                let s = blocktree.meta(i).unwrap().unwrap();
                // The last slot will not chain to any other slots
                if i != 2 {
                    assert_eq!(s.next_slots, vec![i + 1]);
                }
                if i == 0 {
                    assert_eq!(s.parent_slot, 0);
                } else {
                    assert_eq!(s.parent_slot, i - 1);
                }
                assert_eq!(s.last_index, entries_per_slot - 1);
                assert!(s.is_trunk);
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_handle_chaining_missing_slots() {
        let blocktree_path = get_tmp_ledger_path("test_handle_chaining_missing_slots");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 30;
            let entries_per_slot = 2;

            // Separate every other slot into two separate vectors
            let mut slots = vec![];
            let mut missing_slots = vec![];
            for slot_height in 0..num_slots {
                let parent_slot = {
                    if slot_height == 0 {
                        0
                    } else {
                        slot_height - 1
                    }
                };
                let (slot_blobs, _) = make_slot_entries(slot_height, parent_slot, entries_per_slot);

                if slot_height % 2 == 1 {
                    slots.extend(slot_blobs);
                } else {
                    missing_slots.extend(slot_blobs);
                }
            }

            // Write the blobs for every other slot
            blocktree.write_blobs(&slots).unwrap();

            // Check metadata
            for i in 0..num_slots {
                // If "i" is the index of a slot we just inserted, then next_slots should be empty
                // for slot "i" because no slots chain to that slot, because slot i + 1 is missing.
                // However, if it's a slot we haven't inserted, aka one of the gaps, then one of the slots
                // we just inserted will chain to that gap, so next_slots for that placeholder
                // slot won't be empty, but the parent slot is unknown so should equal std::u64::MAX.
                let s = blocktree.meta(i as u64).unwrap().unwrap();
                if i % 2 == 0 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                    assert_eq!(s.parent_slot, std::u64::MAX);
                } else {
                    assert!(s.next_slots.is_empty());
                    assert_eq!(s.parent_slot, i - 1);
                }

                if i == 0 {
                    assert!(s.is_trunk);
                } else {
                    assert!(!s.is_trunk);
                }
            }

            // Write the blobs for the other half of the slots that we didn't insert earlier
            blocktree.write_blobs(&missing_slots[..]).unwrap();

            for i in 0..num_slots {
                // Check that all the slots chain correctly once the missing slots
                // have been filled
                let s = blocktree.meta(i as u64).unwrap().unwrap();
                if i != num_slots - 1 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                } else {
                    assert!(s.next_slots.is_empty());
                }

                if i == 0 {
                    assert_eq!(s.parent_slot, 0);
                } else {
                    assert_eq!(s.parent_slot, i - 1);
                }
                assert_eq!(s.last_index, entries_per_slot - 1);
                assert!(s.is_trunk);
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_forward_chaining_is_trunk() {
        let blocktree_path = get_tmp_ledger_path("test_forward_chaining_is_trunk");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 15;
            let entries_per_slot = 2;
            assert!(entries_per_slot > 1);

            let (blobs, _) = make_many_slot_entries(0, num_slots, entries_per_slot);

            // Write the blobs such that every 3rd slot has a gap in the beginning
            for (slot_height, slot_ticks) in blobs.chunks(entries_per_slot as usize).enumerate() {
                if slot_height % 3 == 0 {
                    blocktree
                        .write_blobs(&slot_ticks[1..entries_per_slot as usize])
                        .unwrap();
                } else {
                    blocktree
                        .write_blobs(&slot_ticks[..entries_per_slot as usize])
                        .unwrap();
                }
            }

            // Check metadata
            for i in 0..num_slots {
                let s = blocktree.meta(i as u64).unwrap().unwrap();
                // The last slot will not chain to any other slots
                if i as u64 != num_slots - 1 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                } else {
                    assert!(s.next_slots.is_empty());
                }

                if i == 0 {
                    assert_eq!(s.parent_slot, 0);
                } else {
                    assert_eq!(s.parent_slot, i - 1);
                }

                assert_eq!(s.last_index, entries_per_slot - 1);

                // Other than slot 0, no slots should be part of the trunk
                if i != 0 {
                    assert!(!s.is_trunk);
                } else {
                    assert!(s.is_trunk);
                }
            }

            // Iteratively finish every 3rd slot, and check that all slots up to and including
            // slot_index + 3 become part of the trunk
            for (slot_index, slot_ticks) in blobs.chunks(entries_per_slot as usize).enumerate() {
                if slot_index % 3 == 0 {
                    blocktree.write_blobs(&slot_ticks[0..1]).unwrap();

                    for i in 0..num_slots {
                        let s = blocktree.meta(i as u64).unwrap().unwrap();
                        if i != num_slots - 1 {
                            assert_eq!(s.next_slots, vec![i as u64 + 1]);
                        } else {
                            assert!(s.next_slots.is_empty());
                        }
                        if i <= slot_index as u64 + 3 {
                            assert!(s.is_trunk);
                        } else {
                            assert!(!s.is_trunk);
                        }

                        if i == 0 {
                            assert_eq!(s.parent_slot, 0);
                        } else {
                            assert_eq!(s.parent_slot, i - 1);
                        }

                        assert_eq!(s.last_index, entries_per_slot - 1);
                    }
                }
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_chaining_tree() {
        let blocktree_path = get_tmp_ledger_path("test_chaining_forks");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_tree_levels = 6;
            assert!(num_tree_levels > 1);
            let branching_factor: u64 = 4;
            // Number of slots that will be in the tree
            let num_slots = (branching_factor.pow(num_tree_levels) - 1) / (branching_factor - 1);
            let entries_per_slot = 2;
            assert!(entries_per_slot > 1);

            let (mut blobs, _) = make_many_slot_entries(0, num_slots, entries_per_slot);

            // Insert tree one slot at a time in a random order
            let mut slots: Vec<_> = (0..num_slots).collect();

            // Get blobs for the slot
            slots.shuffle(&mut thread_rng());
            for slot_height in slots {
                // Get blobs for the slot "slot_height"
                let slot_blobs = &mut blobs[(slot_height * entries_per_slot) as usize
                    ..((slot_height + 1) * entries_per_slot) as usize];
                for blob in slot_blobs.iter_mut() {
                    // Get the parent slot of the slot in the tree
                    let slot_parent = {
                        if slot_height == 0 {
                            0
                        } else {
                            (slot_height - 1) / branching_factor
                        }
                    };
                    blob.set_parent(slot_parent);
                }

                blocktree.write_blobs(slot_blobs).unwrap();
            }

            // Make sure everything chains correctly
            let last_level =
                (branching_factor.pow(num_tree_levels - 1) - 1) / (branching_factor - 1);
            for slot_height in 0..num_slots {
                let slot_meta = blocktree.meta(slot_height).unwrap().unwrap();
                assert_eq!(slot_meta.consumed, entries_per_slot);
                assert_eq!(slot_meta.received, entries_per_slot);
                let slot_parent = {
                    if slot_height == 0 {
                        0
                    } else {
                        (slot_height - 1) / branching_factor
                    }
                };
                assert_eq!(slot_meta.parent_slot, slot_parent);

                let expected_children: HashSet<_> = {
                    if slot_height >= last_level {
                        HashSet::new()
                    } else {
                        let first_child_slot =
                            min(num_slots - 1, slot_height * branching_factor + 1);
                        let last_child_slot =
                            min(num_slots - 1, (slot_height + 1) * branching_factor);
                        (first_child_slot..last_child_slot + 1).collect()
                    }
                };

                let result: HashSet<_> = slot_meta.next_slots.iter().cloned().collect();
                if expected_children.len() != 0 {
                    assert_eq!(slot_meta.next_slots.len(), branching_factor as usize);
                } else {
                    assert_eq!(slot_meta.next_slots.len(), 0);
                }
                assert_eq!(expected_children, result);
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slots_since() {
        let blocktree_path = get_tmp_ledger_path("test_get_slots_since");

        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Slot doesn't exist
            assert!(blocktree.get_slots_since(&vec![0]).unwrap().is_empty());

            let mut meta0 = SlotMeta::new(0, 1);
            blocktree.meta_cf.put_slot_meta(0, &meta0).unwrap();

            // Slot exists, chains to nothing
            assert!(blocktree.get_slots_since(&vec![0]).unwrap().is_empty());
            meta0.next_slots = vec![1, 2];
            blocktree.meta_cf.put_slot_meta(0, &meta0).unwrap();

            // Slot exists, chains to some other slots
            assert_eq!(blocktree.get_slots_since(&vec![0]).unwrap(), vec![1, 2]);
            assert_eq!(blocktree.get_slots_since(&vec![0, 1]).unwrap(), vec![1, 2]);

            let mut meta3 = SlotMeta::new(3, 1);
            meta3.next_slots = vec![10, 5];
            blocktree.meta_cf.put_slot_meta(3, &meta3).unwrap();
            assert_eq!(
                blocktree.get_slots_since(&vec![0, 1, 3]).unwrap(),
                vec![1, 2, 10, 5]
            );
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    fn test_insert_data_blobs_slots(name: &str, should_bulk_write: bool) {
        let blocktree_path = get_tmp_ledger_path(name);
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create blobs and entries
            let num_entries = 20 as u64;
            let mut entries = vec![];
            let mut blobs = vec![];
            for slot_height in 0..num_entries {
                let parent_slot = {
                    if slot_height == 0 {
                        0
                    } else {
                        slot_height - 1
                    }
                };

                let (mut blob, entry) = make_slot_entries(slot_height, parent_slot, 1);
                blob[0].set_index(slot_height);
                blobs.extend(blob);
                entries.extend(entry);
            }

            // Write blobs to the database
            if should_bulk_write {
                blocktree.write_blobs(blobs.iter()).unwrap();
            } else {
                for i in 0..num_entries {
                    let i = i as usize;
                    blocktree.write_blobs(&blobs[i..i + 1]).unwrap();
                }
            }

            for i in 0..num_entries - 1 {
                assert_eq!(
                    blocktree.get_slot_entries(i, i, None).unwrap()[0],
                    entries[i as usize]
                );

                let meta_key = MetaCf::key(i);
                let meta = blocktree.meta_cf.get(&meta_key).unwrap().unwrap();
                assert_eq!(meta.received, i + 1);
                assert_eq!(meta.last_index, i);
                if i != 0 {
                    assert_eq!(meta.parent_slot, i - 1);
                    assert!(meta.consumed == 0);
                } else {
                    assert_eq!(meta.parent_slot, 0);
                    assert!(meta.consumed == 1);
                }
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    pub fn entries_to_blobs(entries: &Vec<Entry>, slot_height: u64, parent_slot: u64) -> Vec<Blob> {
        let mut blobs = entries.clone().to_blobs();
        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64);
            b.set_slot(slot_height);
            b.set_parent(parent_slot);
        }

        blobs.last_mut().unwrap().set_is_last_in_slot();
        blobs
    }

    pub fn make_slot_entries(
        slot_height: u64,
        parent_slot: u64,
        num_entries: u64,
    ) -> (Vec<Blob>, Vec<Entry>) {
        let entries = make_tiny_test_entries(num_entries as usize);
        let blobs = entries_to_blobs(&entries, slot_height, parent_slot);
        (blobs, entries)
    }

    pub fn make_many_slot_entries(
        start_slot_height: u64,
        num_slots: u64,
        entries_per_slot: u64,
    ) -> (Vec<Blob>, Vec<Entry>) {
        let mut blobs = vec![];
        let mut entries = vec![];
        for slot_height in start_slot_height..start_slot_height + num_slots {
            let parent_slot = if slot_height == 0 { 0 } else { slot_height - 1 };

            let (slot_blobs, slot_entries) =
                make_slot_entries(slot_height, parent_slot, entries_per_slot);
            blobs.extend(slot_blobs);
            entries.extend(slot_entries);
        }

        (blobs, entries)
    }
}
