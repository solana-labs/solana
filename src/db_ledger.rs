//! The `db_ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::entry::Entry;
use crate::genesis_block::GenesisBlock;
use crate::leader_scheduler::DEFAULT_TICKS_PER_SLOT;
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};
use bincode::{deserialize, serialize};
use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use hashbrown::HashMap;
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DBRawIterator, Options, WriteBatch, DB};
use serde::de::DeserializeOwned;
use serde::Serialize;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp;
use std::fs;
use std::io;
use std::iter::once;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
// The Meta column family
pub struct SlotMeta {
    // The total number of consecutive blob starting from index 0
    // we have received for this slot.
    pub consumed: u64,
    // The entry height of the highest blob received for this slot.
    pub received: u64,
    // The number of ticks in the range [0..consumed]. Used to detect when
    // a slot contains all expected ticks, so that components like repair/ReplayStage
    // know to look at the next slot.
    pub consumed_ticks: u64,
    // The number of blocks in this slot
    pub num_blocks: u64,
    // The list of slots that chains to this slot
    pub next_slots: Vec<u64>,
    // True if every block from 0..slot, where slot is the slot index of this slot
    // is full
    pub is_trunk: bool,
}

impl SlotMeta {
    pub fn contains_all_ticks(&self, db_ledger: &DbLedger) -> bool {
        if self.num_blocks == 0 {
            // A placeholder slot does not contain all the ticks
            false
        } else {
            self.num_expected_ticks(db_ledger) <= self.consumed_ticks
        }
    }

    pub fn num_expected_ticks(&self, db_ledger: &DbLedger) -> u64 {
        db_ledger.ticks_per_block * self.num_blocks
    }

    fn new(slot_height: u64, num_blocks: u64) -> Self {
        SlotMeta {
            consumed: 0,
            received: 0,
            consumed_ticks: 0,
            num_blocks,
            next_slots: vec![],
            is_trunk: slot_height == 0,
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct DbLedgerConfig {
    pub ticks_per_block: u64,
}

impl DbLedgerConfig {
    pub fn new(ticks_per_block: u64) -> Self {
        DbLedgerConfig { ticks_per_block }
    }
}

impl Default for DbLedgerConfig {
    fn default() -> Self {
        Self::new(DEFAULT_TICKS_PER_SLOT)
    }
}

// ledger window
pub struct DbLedger {
    // Underlying database is automatically closed in the Drop implementation of DB
    db: Arc<DB>,
    meta_cf: MetaCf,
    data_cf: DataCf,
    erasure_cf: ErasureCf,
    new_blobs_signals: Vec<SyncSender<bool>>,
    ticks_per_block: u64,
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

        // TODO: make these constructor arguments
        // Issue: https://github.com/solana-labs/solana/issues/2458
        let ticks_per_block = DEFAULT_TICKS_PER_SLOT;
        Ok(DbLedger {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
            new_blobs_signals: vec![],
            ticks_per_block,
        })
    }

    pub fn open_with_signal(ledger_path: &str) -> Result<(Self, SyncSender<bool>, Receiver<bool>)> {
        let mut db_ledger = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        db_ledger.new_blobs_signals = vec![signal_sender.clone()];

        Ok((db_ledger, signal_sender, signal_receiver))
    }

    pub fn open_config(ledger_path: &str, config: &DbLedgerConfig) -> Result<Self> {
        let mut db_ledger = Self::open(ledger_path)?;
        db_ledger.ticks_per_block = config.ticks_per_block;
        Ok(db_ledger)
    }

    pub fn open_with_config_signal(
        ledger_path: &str,
        config: &DbLedgerConfig,
    ) -> Result<(Self, SyncSender<bool>, Receiver<bool>)> {
        let mut db_ledger = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        db_ledger.new_blobs_signals = vec![signal_sender.clone()];
        db_ledger.ticks_per_block = config.ticks_per_block;

        Ok((db_ledger, signal_sender, signal_receiver))
    }

    pub fn meta(&self, slot_height: u64) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(&MetaCf::key(slot_height))
    }

    pub fn destroy(ledger_path: &str) -> Result<()> {
        // DB::destroy() fails if `ledger_path` doesn't exist
        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(ledger_path).join(DB_LEDGER_DIRECTORY);
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
        let entries = self.insert_data_blobs(blobs)?;
        Ok(entries)
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
                b.set_index(idx as u64 + index);
                b.set_slot(slot);
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
        let mut write_batch = WriteBatch::default();
        // A map from slot_height to a 2-tuple of metadata: (working copy, backup copy),
        // so we can detect changes to the slot metadata later
        let mut slot_meta_working_set = HashMap::new();
        let new_blobs: Vec<_> = new_blobs.into_iter().collect();
        let mut prev_inserted_blob_datas = HashMap::new();

        let mut consecutive_entries = vec![];
        for blob in new_blobs.iter() {
            let blob = blob.borrow();
            let blob_slot = blob.slot();

            // Check if we've already inserted the slot metadata for this blob's slot
            let entry = slot_meta_working_set.entry(blob_slot).or_insert_with(|| {
                // Store a 2-tuple of the metadata (working copy, backup copy)
                if let Some(mut meta) = self
                    .meta_cf
                    .get_slot_meta(blob_slot)
                    .expect("Expect database get to succeed")
                {
                    // If num_blocks == 0, then this is one of the dummy metadatas inserted
                    // during the chaining process, see the function find_slot_meta_in_cached_state()
                    // for details
                    if meta.num_blocks == 0 {
                        // TODO: derive num_blocks for this metadata from the blob itself
                        // Issue: https://github.com/solana-labs/solana/issues/2459.
                        meta.num_blocks = 1;
                        // Set backup as None so that all the logic for inserting new slots
                        // still runs, as this placeholder slot is essentially equivalent to
                        // inserting a new slot
                        (Rc::new(RefCell::new(meta.clone())), None)
                    } else {
                        (Rc::new(RefCell::new(meta.clone())), Some(meta))
                    }
                } else {
                    // TODO: derive num_blocks for this metadata from the blob itself
                    // Issue: https://github.com/solana-labs/solana/issues/2459
                    (Rc::new(RefCell::new(SlotMeta::new(blob_slot, 1))), None)
                }
            });

            let slot_meta = &mut entry.0.borrow_mut();

            // This slot is full, skip the bogus blob
            if slot_meta.contains_all_ticks(&self) {
                continue;
            }

            let entries = self.insert_data_blob(
                blob,
                &mut prev_inserted_blob_datas,
                slot_meta,
                &mut write_batch,
            );
            if let Ok(entries) = entries {
                consecutive_entries.extend(entries);
            }
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

        // TODO: Delete returning these entries and instead have replay_stage query db_ledger
        // for updates. Returning these entries is to temporarily support current API as to
        // not break functionality in db_window.
        // Issue: https://github.com/solana-labs/solana/issues/2444
        Ok(consecutive_entries)
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
        db_iterator: &mut DbLedgerRawIterator,
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
            .into_iter()
            .map(|slot_height| self.meta_cf.get_slot_meta(*slot_height))
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
            assert!(slot_meta.num_blocks > 0);

            // If:
            // 1) This is a new slot
            // 2) slot_height != 0
            // then try to chain this slot to a previous slot
            if slot_height != 0 {
                let prev_slot_height = slot_height - slot_meta.num_blocks;

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
                if current_slot.contains_all_ticks(self) {
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
        current_slot.is_trunk = prev_slot.is_trunk && prev_slot.contains_all_ticks(self);
    }

    fn is_newly_completed_slot(
        &self,
        slot_meta: &SlotMeta,
        backup_slot_meta: &Option<SlotMeta>,
    ) -> bool {
        slot_meta.contains_all_ticks(self)
            && (backup_slot_meta.is_none()
                || slot_meta.consumed_ticks != backup_slot_meta.as_ref().unwrap().consumed_ticks)
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
        if let Some(slot) = self.meta_cf.get_slot_meta(slot_height)? {
            insert_map.insert(slot_height, Rc::new(RefCell::new(slot)));
            Ok(insert_map.get(&slot_height).unwrap().clone())
        } else {
            // If this slot doesn't exist, make a dummy placeholder slot (denoted by passing
            // 0 for the num_blocks argument to the SlotMeta constructor). This way we
            // remember which slots chained to this one when we eventually get a real blob
            // for this slot
            insert_map.insert(
                slot_height,
                Rc::new(RefCell::new(SlotMeta::new(slot_height, 0))),
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
    ) -> Result<Vec<Entry>> {
        let blob_index = blob_to_insert.index();
        let blob_slot = blob_to_insert.slot();
        let blob_size = blob_to_insert.size();

        if blob_index < slot_meta.consumed
            || prev_inserted_blob_datas.contains_key(&(blob_slot, blob_index))
        {
            return Err(Error::DbLedgerError(DbLedgerError::BlobForIndexExists));
        }

        let (new_consumed, new_consumed_ticks, blob_datas) = {
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

                let blob_to_insert = Cow::Borrowed(&blob_to_insert.data[..]);
                let mut new_consumed_ticks = 0;
                let mut entries = vec![];
                // Check all the consecutive blobs for ticks
                for blob_data in once(&blob_to_insert).chain(blob_datas.iter()) {
                    let serialized_entry_data = &blob_data[BLOB_HEADER_SIZE..];
                    let entry: Entry = deserialize(serialized_entry_data).expect(
                        "Blob made it past validation, so must be deserializable at this point",
                    );
                    if entry.is_tick() {
                        new_consumed_ticks += 1;
                    }
                    entries.push(entry);
                }

                (
                    // Add one because we skipped this current blob when calling
                    // get_slot_consecutive_blobs() earlier
                    slot_meta.consumed + blob_datas.len() as u64 + 1,
                    new_consumed_ticks,
                    entries,
                )
            } else {
                (slot_meta.consumed, 0, vec![])
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
        slot_meta.consumed_ticks += new_consumed_ticks;
        // TODO: Remove returning these entries and instead have replay_stage query db_ledger
        // for updates. Returning these entries is to temporarily support current API as to
        // not break functionality in db_window.
        // Issue: https://github.com/solana-labs/solana/issues/2444
        Ok(blob_datas)
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
        let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
        let mut bootstrap_meta = SlotMeta::new(0, 1);
        let last = blobs.last().unwrap();
        let num_ending_ticks = blobs.iter().skip(2).fold(0, |tick_count, blob| {
            let entry: Entry = deserialize(&blob.data[BLOB_HEADER_SIZE..])
                .expect("Blob has to be deseriablizable");
            tick_count + entry.is_tick() as u64
        });
        bootstrap_meta.consumed = last.index() + 1;
        bootstrap_meta.received = last.index() + 1;
        bootstrap_meta.is_trunk = true;
        bootstrap_meta.consumed_ticks = num_ending_ticks;

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

pub fn create_new_ledger(ledger_path: &str, genesis_block: &GenesisBlock) -> Result<(u64, Hash)> {
    DbLedger::destroy(ledger_path)?;
    genesis_block.write(&ledger_path)?;

    // Add a single tick linked back to the genesis_block to bootstrap the ledger
    let db_ledger = DbLedger::open(ledger_path)?;
    let entries = crate::entry::create_ticks(1, genesis_block.last_id());
    db_ledger.write_entries(DEFAULT_SLOT_HEIGHT, 0, &entries)?;

    Ok((1, entries[0].id))
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
            b.set_index(idx as u64);
            b.set_id(&keypair.pubkey());
            b.set_slot(DEFAULT_SLOT_HEIGHT);
            b
        })
        .collect();

    db_ledger.write_genesis_blobs(&blobs[..])?;
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
    create_new_ledger(&ledger_path, genesis_block).unwrap();
    ledger_path
}

pub fn create_tmp_sample_ledger(
    name: &str,
    num_tokens: u64,
    num_extra_ticks: u64,
    bootstrap_leader_id: Pubkey,
    bootstrap_leader_tokens: u64,
) -> (Keypair, String, u64, Hash) {
    let (genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(num_tokens, bootstrap_leader_id, bootstrap_leader_tokens);
    let ledger_path = get_tmp_ledger_path(name);
    let (mut entry_height, mut last_id) = create_new_ledger(&ledger_path, &genesis_block).unwrap();

    if num_extra_ticks > 0 {
        let entries = crate::entry::create_ticks(num_extra_ticks, last_id);

        let db_ledger = DbLedger::open(&ledger_path).unwrap();
        db_ledger
            .write_entries(DEFAULT_SLOT_HEIGHT, entry_height, &entries)
            .unwrap();
        entry_height += entries.len() as u64;
        last_id = entries.last().unwrap().id
    }
    (mint_keypair, ledger_path, entry_height, last_id)
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
    use crate::entry::{
        create_ticks, make_tiny_test_entries, make_tiny_test_entries_from_id, EntrySlice,
    };
    use crate::packet::index_blobs;
    use solana_sdk::hash::Hash;
    use std::time::Duration;

    #[test]
    fn test_put_get_simple() {
        let ledger_path = get_tmp_ledger_path("test_put_get_simple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Test meta column family
        let meta = SlotMeta::new(DEFAULT_SLOT_HEIGHT, 1);
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
        index_blobs(
            &shared_blobs,
            &Keypair::new().pubkey(),
            &mut 0,
            &mut 0,
            &[slot; 10],
        );

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
        let slot_height = 0;

        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64);
        }

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_basic");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert second blob, we're missing the first blob, so no consecutive
        // blobs starting from slot 0, index 0 should exist.
        ledger.insert_data_blobs(vec![blobs[1]]).unwrap();
        assert!(ledger
            .get_slot_entries(slot_height, 0, None)
            .unwrap()
            .is_empty());

        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(slot_height))
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == 2);

        // Insert first blob, check for consecutive returned entries
        ledger.insert_data_blobs(vec![blobs[0]]).unwrap();
        let result = ledger.get_slot_entries(slot_height, 0, None).unwrap();

        assert_eq!(result, entries);

        let meta = ledger
            .meta_cf
            .get(&MetaCf::key(slot_height))
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert_eq!(meta.consumed, 2);
        assert_eq!(meta.received, 2);
        assert_eq!(meta.consumed_ticks, 0);
        assert!(meta.next_slots.is_empty());
        assert!(meta.is_trunk);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_blobs_multiple() {
        let num_blobs = 10;
        let entries = make_tiny_test_entries(num_blobs);
        let slot_height = 0;
        let shared_blobs = entries.to_shared_blobs();
        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64);
        }
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();

        let ledger_path = get_tmp_ledger_path("test_insert_data_blobs_multiple");
        let ledger = DbLedger::open(&ledger_path).unwrap();

        // Insert blobs in reverse, check for consecutive returned blobs
        for i in (0..num_blobs).rev() {
            ledger.insert_data_blobs(vec![blobs[i]]).unwrap();
            let result = ledger.get_slot_entries(slot_height, 0, None).unwrap();

            let meta = ledger
                .meta_cf
                .get(&MetaCf::key(slot_height))
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
    fn test_insert_slots() {
        test_insert_data_blobs_slots("test_insert_data_blobs_slots_single", false);
        test_insert_data_blobs_slots("test_insert_data_blobs_slots_bulk", true);
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
                w_b.set_index(1 << (i * 8));
                w_b.set_slot(DEFAULT_SLOT_HEIGHT);
            }

            db_ledger
                .write_shared_blobs(&shared_blobs)
                .expect("Expected successful write of blobs");

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
    pub fn test_get_slot_entries1() {
        let db_ledger_path = get_tmp_ledger_path("test_get_slot_entries1");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();
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
            db_ledger
                .write_blobs(&blobs)
                .expect("Expected successful write of blobs");

            assert_eq!(
                db_ledger.get_slot_entries(1, 2, None).unwrap()[..],
                entries[2..4],
            );

            assert_eq!(
                db_ledger.get_slot_entries(1, 12, None).unwrap()[..],
                entries[4..],
            );
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slot_entries2() {
        let db_ledger_path = get_tmp_ledger_path("test_get_slot_entries2");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

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
                db_ledger
                    .write_blobs(&blobs)
                    .expect("Expected successful write of blobs");
                assert_eq!(
                    db_ledger
                        .get_slot_entries(slot_height, index - 1, None)
                        .unwrap(),
                    vec![last_entry],
                );
            }
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_consecutive() {
        let db_ledger_path = get_tmp_ledger_path("test_insert_data_blobs_consecutive");
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();
            let slot = 0;

            // Write entries
            let num_entries = 20 as u64;
            let original_entries = create_ticks(num_entries, Hash::default());
            let shared_blobs = original_entries.clone().to_shared_blobs();
            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(i as u64);
                w_b.set_slot(slot);
            }

            db_ledger
                .write_shared_blobs(shared_blobs.iter().skip(1).step_by(2))
                .unwrap();

            assert_eq!(db_ledger.get_slot_entries(0, 0, None).unwrap(), vec![]);

            let meta_key = MetaCf::key(slot);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed, 0);
            assert_eq!(meta.consumed_ticks, 0);

            db_ledger
                .write_shared_blobs(shared_blobs.iter().step_by(2))
                .unwrap();

            assert_eq!(
                db_ledger.get_slot_entries(0, 0, None).unwrap(),
                original_entries,
            );

            let meta_key = MetaCf::key(slot);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.received, num_entries);
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.consumed_ticks, num_entries);
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
                w_b.set_index(index);
            }

            db_ledger
                .write_shared_blobs(
                    shared_blobs
                        .iter()
                        .skip(num_duplicates)
                        .step_by(num_duplicates * 2),
                )
                .unwrap();

            assert_eq!(db_ledger.get_slot_entries(0, 0, None).unwrap(), vec![]);

            db_ledger
                .write_shared_blobs(shared_blobs.iter().step_by(num_duplicates * 2))
                .unwrap();

            let expected: Vec<_> = original_entries
                .into_iter()
                .step_by(num_duplicates)
                .collect();

            assert_eq!(db_ledger.get_slot_entries(0, 0, None).unwrap(), expected,);

            let meta_key = MetaCf::key(DEFAULT_SLOT_HEIGHT);
            let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
            assert_eq!(meta.consumed, num_entries);
            assert_eq!(meta.received, num_entries);
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

    #[test]
    pub fn test_new_blobs_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_new_blobs_signal");
        let ticks_per_block = 10;
        let config = DbLedgerConfig::new(ticks_per_block);
        let (ledger, _, recvr) = DbLedger::open_with_config_signal(&ledger_path, &config).unwrap();
        let ledger = Arc::new(ledger);

        // Create ticks for slot 0
        let entries = create_ticks(ticks_per_block, Hash::default());
        let mut blobs = entries.to_blobs();

        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64);
        }

        // Insert second blob, but we're missing the first blob, so no consecutive
        // blobs starting from slot 0, index 0 should exist.
        ledger.insert_data_blobs(&blobs[1..2]).unwrap();
        let timer = Duration::new(1, 0);
        assert!(recvr.recv_timeout(timer).is_err());
        // Insert first blob, now we've made a consecutive block
        ledger.insert_data_blobs(&blobs[0..1]).unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());
        // Insert the rest of the ticks
        ledger
            .insert_data_blobs(&blobs[1..ticks_per_block as usize])
            .unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Create some other slots, and send batches of ticks for each slot such that each slot
        // is missing the tick at blob index == slot index - 1. Thus, no consecutive blocks
        // will be formed
        let num_slots = ticks_per_block;
        let mut all_blobs = vec![];
        for slot_index in 1..num_slots + 1 {
            let entries = create_ticks(num_slots, Hash::default());
            let mut blobs = entries.to_blobs();
            for (i, ref mut b) in blobs.iter_mut().enumerate() {
                if (i as u64) < slot_index - 1 {
                    b.set_index(i as u64);
                } else {
                    b.set_index(i as u64 + 1);
                }
                b.set_slot(slot_index as u64);
            }

            all_blobs.extend(blobs);
        }

        // Should be no updates, since no new chains from block 0 were formed
        ledger.insert_data_blobs(all_blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // Insert a blob for each slot that doesn't make a consecutive block, we
        // should get no updates
        let all_blobs: Vec<_> = (1..num_slots + 1)
            .map(|slot_index| {
                let entries = create_ticks(1, Hash::default());;
                let mut blob = entries[0].to_blob();
                blob.set_index(2 * num_slots as u64);
                blob.set_slot(slot_index as u64);
                blob
            })
            .collect();

        ledger.insert_data_blobs(all_blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // For slots 1..num_slots/2, fill in the holes in one batch insertion,
        // so we should only get one signal
        let all_blobs: Vec<_> = (1..num_slots / 2)
            .map(|slot_index| {
                let entries = make_tiny_test_entries(1);
                let mut blob = entries[0].to_blob();
                blob.set_index(slot_index as u64 - 1);
                blob.set_slot(slot_index as u64);
                blob
            })
            .collect();

        ledger.insert_data_blobs(all_blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Fill in the holes for each of the remaining slots, we should get a single update
        // for each
        for slot_index in num_slots / 2..num_slots {
            let entries = make_tiny_test_entries(1);
            let mut blob = entries[0].to_blob();
            blob.set_index(slot_index as u64 - 1);
            blob.set_slot(slot_index as u64);
            ledger.insert_data_blobs(vec![blob]).unwrap();
            assert!(recvr.recv_timeout(timer).is_ok());
            assert!(recvr.try_recv().is_err());
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_handle_chaining_basic() {
        let db_ledger_path = get_tmp_ledger_path("test_handle_chaining_basic");
        {
            let config = DbLedgerConfig::new(1);
            let db_ledger = DbLedger::open_config(&db_ledger_path, &config).unwrap();

            let entries = create_ticks(3, Hash::default());
            let mut blobs = entries.to_blobs();
            for (i, ref mut b) in blobs.iter_mut().enumerate() {
                b.set_index(0 as u64);
                b.set_slot(i as u64);
            }

            // 1) Write to the first slot
            db_ledger.write_blobs(&blobs[1..2]).unwrap();
            let s1 = db_ledger.meta_cf.get_slot_meta(1).unwrap().unwrap();
            assert!(s1.next_slots.is_empty());
            // Slot 1 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s1.is_trunk);
            assert_eq!(s1.num_blocks, 1);
            assert_eq!(s1.consumed_ticks, 1);

            // 2) Write to the second slot
            db_ledger.write_blobs(&blobs[2..3]).unwrap();
            let s2 = db_ledger.meta_cf.get_slot_meta(2).unwrap().unwrap();
            assert!(s2.next_slots.is_empty());
            // Slot 2 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s2.is_trunk);
            assert_eq!(s2.num_blocks, 1);
            assert_eq!(s2.consumed_ticks, 1);

            // Check the first slot again, it should chain to the second slot,
            // but still isn't part of the trunk
            let s1 = db_ledger.meta_cf.get_slot_meta(1).unwrap().unwrap();
            assert_eq!(s1.next_slots, vec![2]);
            assert!(!s1.is_trunk);
            assert_eq!(s1.consumed_ticks, 1);

            // 3) Write to the zeroth slot, check that every slot
            // is now part of the trunk
            db_ledger.write_blobs(&blobs[0..1]).unwrap();
            for i in 0..3 {
                let s = db_ledger.meta_cf.get_slot_meta(i).unwrap().unwrap();
                // The last slot will not chain to any other slots
                if i != 2 {
                    assert_eq!(s.next_slots, vec![i + 1]);
                }
                assert_eq!(s.num_blocks, 1);
                assert_eq!(s.consumed_ticks, 1);
                assert!(s.is_trunk);
            }
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_handle_chaining_missing_slots() {
        let db_ledger_path = get_tmp_ledger_path("test_handle_chaining_missing_slots");
        {
            let mut db_ledger = DbLedger::open(&db_ledger_path).unwrap();
            let num_slots = 30;
            let ticks_per_block = 2;
            db_ledger.ticks_per_block = ticks_per_block as u64;

            let ticks = create_ticks((num_slots / 2) * ticks_per_block, Hash::default());
            let mut blobs = ticks.to_blobs();

            // Leave a gap for every other slot
            for (i, ref mut b) in blobs.iter_mut().enumerate() {
                b.set_index(i as u64 % ticks_per_block);
                b.set_slot(((i / 2) * 2 + 1) as u64);
            }

            db_ledger.write_blobs(&blobs[..]).unwrap();

            // Check metadata
            for i in 0..num_slots {
                // If it's a slot we just inserted, then next_slots should be empty
                // because no slots chain to it yet because we left a gap. However, if it's
                // a slot we haven't inserted, aka one of the gaps, then one of the slots
                // we just inserted will chain to that gap
                let s = db_ledger.meta_cf.get_slot_meta(i as u64).unwrap().unwrap();
                if i % 2 == 0 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                    assert_eq!(s.consumed_ticks, 0);
                } else {
                    assert!(s.next_slots.is_empty());
                    assert_eq!(s.consumed_ticks, ticks_per_block as u64);
                }

                if i == 0 {
                    assert!(s.is_trunk);
                } else {
                    assert!(!s.is_trunk);
                }
            }

            // Fill in the gaps
            for (i, ref mut b) in blobs.iter_mut().enumerate() {
                b.set_index(i as u64 % ticks_per_block);
                b.set_slot(((i / 2) * 2) as u64);
            }

            db_ledger.write_blobs(&blobs[..]).unwrap();

            for i in 0..num_slots {
                // Check that all the slots chain correctly once the missing slots
                // have been filled
                let s = db_ledger.meta_cf.get_slot_meta(i as u64).unwrap().unwrap();
                if i != num_slots - 1 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                } else {
                    assert!(s.next_slots.is_empty());
                }
                assert_eq!(s.consumed_ticks, ticks_per_block as u64);
                assert!(s.is_trunk);
            }
        }

        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_forward_chaining_is_trunk() {
        let db_ledger_path = get_tmp_ledger_path("test_forward_chaining_is_trunk");
        {
            let mut db_ledger = DbLedger::open(&db_ledger_path).unwrap();
            let num_slots = 15;
            let ticks_per_block = 2;
            db_ledger.ticks_per_block = ticks_per_block as u64;

            let entries = create_ticks(num_slots * ticks_per_block, Hash::default());
            let mut blobs = entries.to_blobs();
            for (i, ref mut b) in blobs.iter_mut().enumerate() {
                b.set_index(i as u64 % ticks_per_block);
                b.set_slot(i as u64 / ticks_per_block);
            }

            // Write the blobs such that every 3rd block has a gap in the beginning
            for (slot_index, slot_ticks) in blobs.chunks(ticks_per_block as usize).enumerate() {
                if slot_index % 3 == 0 {
                    db_ledger
                        .write_blobs(&slot_ticks[1..ticks_per_block as usize])
                        .unwrap();
                } else {
                    db_ledger
                        .write_blobs(&slot_ticks[..ticks_per_block as usize])
                        .unwrap();
                }
            }

            // Check metadata
            for i in 0..num_slots {
                let s = db_ledger.meta_cf.get_slot_meta(i as u64).unwrap().unwrap();
                // The last slot will not chain to any other slots
                if i as u64 != num_slots - 1 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                } else {
                    assert!(s.next_slots.is_empty());
                }
                assert_eq!(s.num_blocks, 1);
                if i % 3 == 0 {
                    assert_eq!(s.consumed_ticks, 0);
                } else {
                    assert_eq!(s.consumed_ticks, ticks_per_block as u64);
                }

                // Other than slot 0, no slots should be part of the trunk
                if i != 0 {
                    assert!(!s.is_trunk);
                } else {
                    assert!(s.is_trunk);
                }
            }

            // Iteratively finish every 3rd slot, and check that all slots up to and including
            // slot_index + 3 become part of the trunk
            for (slot_index, slot_ticks) in blobs.chunks(ticks_per_block as usize).enumerate() {
                if slot_index % 3 == 0 {
                    db_ledger.write_blobs(&slot_ticks[0..1]).unwrap();

                    for i in 0..num_slots {
                        let s = db_ledger.meta_cf.get_slot_meta(i as u64).unwrap().unwrap();
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
                    }
                }
            }
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slots_since() {
        let db_ledger_path = get_tmp_ledger_path("test_get_slots_since");

        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Slot doesn't exist
            assert!(db_ledger.get_slots_since(&vec![0]).unwrap().is_empty());

            let mut meta0 = SlotMeta::new(0, 1);
            db_ledger.meta_cf.put_slot_meta(0, &meta0).unwrap();

            // Slot exists, chains to nothing
            assert!(db_ledger.get_slots_since(&vec![0]).unwrap().is_empty());
            meta0.next_slots = vec![1, 2];
            db_ledger.meta_cf.put_slot_meta(0, &meta0).unwrap();

            // Slot exists, chains to some other slots
            assert_eq!(db_ledger.get_slots_since(&vec![0]).unwrap(), vec![1, 2]);
            assert_eq!(db_ledger.get_slots_since(&vec![0, 1]).unwrap(), vec![1, 2]);

            let mut meta3 = SlotMeta::new(3, 1);
            meta3.next_slots = vec![10, 5];
            db_ledger.meta_cf.put_slot_meta(3, &meta3).unwrap();
            assert_eq!(
                db_ledger.get_slots_since(&vec![0, 1, 3]).unwrap(),
                vec![1, 2, 10, 5]
            );
        }

        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    fn test_insert_data_blobs_slots(name: &str, should_bulk_write: bool) {
        let db_ledger_path = get_tmp_ledger_path(name);
        {
            let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

            // Write entries
            let num_entries = 20 as u64;
            let original_entries = make_tiny_test_entries(num_entries as usize);
            let shared_blobs = original_entries.clone().to_shared_blobs();
            for (i, b) in shared_blobs.iter().enumerate() {
                let mut w_b = b.write().unwrap();
                w_b.set_index(i as u64);
                w_b.set_slot(i as u64);
            }

            if should_bulk_write {
                db_ledger.write_shared_blobs(shared_blobs.iter()).unwrap();
            } else {
                for i in 0..num_entries {
                    let i = i as usize;
                    db_ledger
                        .write_shared_blobs(&shared_blobs[i..i + 1])
                        .unwrap();
                }
            }

            for i in 0..num_entries - 1 {
                assert_eq!(
                    db_ledger.get_slot_entries(i, i, None).unwrap()[0],
                    original_entries[i as usize]
                );

                let meta_key = MetaCf::key(i);
                let meta = db_ledger.meta_cf.get(&meta_key).unwrap().unwrap();
                assert_eq!(meta.received, i + 1);
                if i != 0 {
                    assert!(meta.consumed == 0);
                } else {
                    assert!(meta.consumed == 1);
                }
            }
        }
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }
}
