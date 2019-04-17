//! The `block_tree` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::entry::Entry;
use crate::erasure;
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};
#[cfg(feature = "kvstore")]
use solana_kvstore as kvstore;

use bincode::deserialize;

use hashbrown::HashMap;

#[cfg(not(feature = "kvstore"))]
use rocksdb;

use solana_metrics::counter::Counter;

use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};

use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp;
use std::fs;
use std::io;
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, RwLock};

pub use self::meta::*;

mod db;
mod meta;

macro_rules! db_imports {
    { $mod:ident, $db:ident, $db_path:expr } => {
        mod $mod;

        use $mod::$db;
        use db::columns as cf;

        pub use db::columns;

        pub type Database = db::Database<$db>;
        pub type Cursor<C>  = db::Cursor<$db, C>;
        pub type LedgerColumn<C> = db::LedgerColumn<$db, C>;
        pub type WriteBatch = db::WriteBatch<$db>;

        pub trait Column: db::Column<$db> {}
        impl<C: db::Column<$db>> Column for C {}

        pub const BLOCKTREE_DIRECTORY: &str = $db_path;
    };
}

#[cfg(not(feature = "kvstore"))]
db_imports! {rocks, Rocks, "rocksdb"}
#[cfg(feature = "kvstore")]
db_imports! {kvs, Kvs, "kvstore"}

#[derive(Debug)]
pub enum BlocktreeError {
    BlobForIndexExists,
    InvalidBlobData,
    RocksDb(rocksdb::Error),
    #[cfg(feature = "kvstore")]
    KvsDb(kvstore::Error),
}

// ledger window
pub struct Blocktree {
    db: Arc<Database>,
    meta_cf: LedgerColumn<cf::SlotMeta>,
    data_cf: LedgerColumn<cf::Data>,
    erasure_cf: LedgerColumn<cf::Coding>,
    erasure_meta_cf: LedgerColumn<cf::ErasureMeta>,
    orphans_cf: LedgerColumn<cf::Orphans>,
    session: Arc<erasure::Session>,
    pub new_blobs_signals: Vec<SyncSender<bool>>,
    pub root_slot: RwLock<u64>,
}

// Column family for metadata about a leader slot
pub const META_CF: &str = "meta";
// Column family for the data in a leader slot
pub const DATA_CF: &str = "data";
// Column family for erasure data
pub const ERASURE_CF: &str = "erasure";
pub const ERASURE_META_CF: &str = "erasure_meta";
// Column family for orphans data
pub const ORPHANS_CF: &str = "orphans";

impl Blocktree {
    /// Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &str) -> Result<Blocktree> {
        use std::path::Path;

        fs::create_dir_all(&ledger_path)?;
        let ledger_path = Path::new(&ledger_path).join(BLOCKTREE_DIRECTORY);

        // Open the database
        let db = Arc::new(Database::open(&ledger_path)?);

        // Create the metadata column family
        let meta_cf = LedgerColumn::new(&db);

        // Create the data column family
        let data_cf = LedgerColumn::new(&db);

        // Create the erasure column family
        let erasure_cf = LedgerColumn::new(&db);

        let erasure_meta_cf = LedgerColumn::new(&db);

        // Create the orphans column family. An "orphan" is defined as
        // the head of a detached chain of slots, i.e. a slot with no
        // known parent
        let orphans_cf = LedgerColumn::new(&db);

        // setup erasure
        let session = Arc::new(erasure::Session::default());

        Ok(Blocktree {
            db,
            meta_cf,
            data_cf,
            erasure_cf,
            erasure_meta_cf,
            orphans_cf,
            session,
            new_blobs_signals: vec![],
            root_slot: RwLock::new(0),
        })
    }

    pub fn open_with_signal(ledger_path: &str) -> Result<(Self, Receiver<bool>)> {
        let mut blocktree = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        blocktree.new_blobs_signals = vec![signal_sender];

        Ok((blocktree, signal_receiver))
    }

    pub fn destroy(ledger_path: &str) -> Result<()> {
        // Database::destroy() fails is the path doesn't exist
        fs::create_dir_all(ledger_path)?;
        let path = std::path::Path::new(ledger_path).join(BLOCKTREE_DIRECTORY);
        Database::destroy(&path)
    }

    pub fn meta(&self, slot: u64) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(slot)
    }

    pub fn orphan(&self, slot: u64) -> Result<Option<bool>> {
        self.orphans_cf.get(slot)
    }

    pub fn get_next_slot(&self, slot: u64) -> Result<Option<u64>> {
        let mut db_iterator = self.db.cursor::<cf::SlotMeta>()?;
        db_iterator.seek(slot + 1);
        if !db_iterator.valid() {
            Ok(None)
        } else {
            let next_slot = db_iterator.key().expect("Expected valid key");
            Ok(Some(next_slot))
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
        ticks_per_slot: u64,
        entries: I,
    ) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Entry>,
    {
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
        let mut write_batch = self.db.batch()?;
        // A map from slot to a 2-tuple of metadata: (working copy, backup copy),
        // so we can detect changes to the slot metadata later
        let mut slot_meta_working_set = HashMap::new();
        let mut erasure_meta_working_set = HashMap::new();
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
                    let backup = Some(meta.clone());
                    // If parent_slot == std::u64::MAX, then this is one of the orphans inserted
                    // during the chaining process, see the function find_slot_meta_in_cached_state()
                    // for details. Slots that are orphans are missing a parent_slot, so we should
                    // fill in the parent now that we know it.
                    if Self::is_orphan(&meta) {
                        meta.parent_slot = parent_slot;
                    }

                    (Rc::new(RefCell::new(meta)), backup)
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

            let set_index = ErasureMeta::set_index_for(blob.index());
            let erasure_meta_entry = erasure_meta_working_set
                .entry((blob_slot, set_index))
                .or_insert_with(|| {
                    self.erasure_meta_cf
                        .get((blob_slot, set_index))
                        .expect("Expect database get to succeed")
                        .unwrap_or_else(|| ErasureMeta::new(set_index))
                });

            erasure_meta_entry.set_data_present(blob.index(), true);

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
        for (slot, (meta, meta_backup)) in slot_meta_working_set.iter() {
            let meta: &SlotMeta = &RefCell::borrow(&*meta);
            // Check if the working copy of the metadata has changed
            if Some(meta) != meta_backup.as_ref() {
                should_signal = should_signal || Self::slot_has_updates(meta, &meta_backup);
                write_batch.put::<cf::SlotMeta>(*slot, &meta)?;
            }
        }

        for ((slot, set_index), erasure_meta) in erasure_meta_working_set.iter() {
            write_batch.put::<cf::ErasureMeta>((*slot, *set_index), erasure_meta)?;
        }

        self.db.write(write_batch)?;

        if should_signal {
            for signal in self.new_blobs_signals.iter() {
                let _ = signal.try_send(true);
            }
        }

        for ((slot, set_index), erasure_meta) in erasure_meta_working_set.into_iter() {
            self.try_erasure_recover(&erasure_meta, slot, set_index)?;
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
        slot: u64,
    ) -> Result<(u64, u64)> {
        let mut db_iterator = self.db.cursor::<cf::Data>()?;
        db_iterator.seek((slot, start_index));
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
            let (_, index) = db_iterator.key().expect("Expected valid key");
            if index != expected_index {
                break;
            }

            // Get the blob data
            let value = &db_iterator.value_bytes();

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

    pub fn get_coding_blob_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.erasure_cf.get_bytes((slot, index))
    }

    pub fn delete_coding_blob(&self, slot: u64, index: u64) -> Result<()> {
        let set_index = ErasureMeta::set_index_for(index);

        let mut erasure_meta = self
            .erasure_meta_cf
            .get((slot, set_index))?
            .unwrap_or_else(|| ErasureMeta::new(set_index));

        erasure_meta.set_coding_present(index, false);

        let mut batch = self.db.batch()?;

        batch.delete::<cf::Coding>((slot, index))?;
        batch.put::<cf::ErasureMeta>((slot, set_index), &erasure_meta)?;

        self.db.write(batch)?;
        Ok(())
    }

    pub fn get_data_blob_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.data_cf.get_bytes((slot, index))
    }

    /// For benchmarks, testing, and setup.
    /// Does no metadata tracking. Use with care.
    pub fn put_data_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.data_cf.put_bytes((slot, index), bytes)
    }

    pub fn put_coding_blob_bytes_raw(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.erasure_cf.put_bytes((slot, index), bytes)
    }

    /// this function will insert coding blobs and also automatically track erasure-related
    /// metadata. If recovery is available it will be done
    pub fn put_coding_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        let set_index = ErasureMeta::set_index_for(index);
        let mut erasure_meta = self
            .erasure_meta_cf
            .get((slot, set_index))?
            .unwrap_or_else(|| ErasureMeta::new(set_index));

        erasure_meta.set_coding_present(index, true);

        let mut writebatch = self.db.batch()?;

        writebatch.put_bytes::<cf::Coding>((slot, index), bytes)?;

        writebatch.put::<cf::ErasureMeta>((slot, set_index), &erasure_meta)?;

        self.db.write(writebatch)?;

        self.try_erasure_recover(&erasure_meta, slot, set_index)
    }

    fn try_erasure_recover(
        &self,
        erasure_meta: &ErasureMeta,
        slot: u64,
        set_index: u64,
    ) -> Result<()> {
        match erasure_meta.status() {
            ErasureMetaStatus::CanRecover => {
                let recovered = self.recover(slot, set_index)?;
                inc_new_counter_info!("blocktree-erasure-blobs_recovered", recovered);
            }
            ErasureMetaStatus::StillNeed(needed) => {
                inc_new_counter_info!("blocktree-erasure-blobs_needed", needed)
            }
            ErasureMetaStatus::DataFull => inc_new_counter_info!("blocktree-erasure-complete", 1),
        }
        Ok(())
    }

    pub fn get_data_blob(&self, slot: u64, blob_index: u64) -> Result<Option<Blob>> {
        let bytes = self.get_data_blob_bytes(slot, blob_index)?;
        Ok(bytes.map(|bytes| {
            let blob = Blob::new(&bytes);
            assert!(blob.slot() == slot);
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
    // for the slot with the specified slot
    fn find_missing_indexes<C>(
        db_iterator: &mut Cursor<C>,
        slot: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
    ) -> Vec<u64>
    where
        C: Column<Index = (u64, u64)>,
    {
        if start_index >= end_index || max_missing == 0 {
            return vec![];
        }

        let mut missing_indexes = vec![];

        // Seek to the first blob with index >= start_index
        db_iterator.seek((slot, start_index));

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
            let (current_slot, index) = db_iterator.key().expect("Expect a valid key");
            let current_index = {
                if current_slot > slot {
                    end_index
                } else {
                    index
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
        if let Ok(mut db_iterator) = self.data_cf.cursor() {
            Self::find_missing_indexes(&mut db_iterator, slot, start_index, end_index, max_missing)
        } else {
            vec![]
        }
    }

    /// Returns the entry vector for the slot starting with `blob_start_index`
    pub fn get_slot_entries(
        &self,
        slot: u64,
        blob_start_index: u64,
        max_entries: Option<u64>,
    ) -> Result<Vec<Entry>> {
        self.get_slot_entries_with_blob_count(slot, blob_start_index, max_entries)
            .map(|x| x.0)
    }

    pub fn read_ledger_blobs(&self) -> impl Iterator<Item = Blob> {
        self.data_cf
            .iter()
            .unwrap()
            .map(|(_, blob_data)| Blob::new(&blob_data))
    }

    /// Return an iterator for all the entries in the given file.
    pub fn read_ledger(&self) -> Result<impl Iterator<Item = Entry>> {
        use crate::entry::EntrySlice;
        use std::collections::VecDeque;

        struct EntryIterator {
            db_iterator: Cursor<cf::Data>,

            // TODO: remove me when replay_stage is iterating by block (Blocktree)
            //    this verification is duplicating that of replay_stage, which
            //    can do this in parallel
            blockhash: Option<Hash>,
            // https://github.com/rust-rocksdb/rust-rocksdb/issues/234
            //   rocksdb issue: the _blocktree member must be lower in the struct to prevent a crash
            //   when the db_iterator member above is dropped.
            //   _blocktree is unused, but dropping _blocktree results in a broken db_iterator
            //   you have to hold the database open in order to iterate over it, and in order
            //   for db_iterator to be able to run Drop
            //    _blocktree: Blocktree,
            entries: VecDeque<Entry>,
        }

        impl Iterator for EntryIterator {
            type Item = Entry;

            fn next(&mut self) -> Option<Entry> {
                if !self.entries.is_empty() {
                    return Some(self.entries.pop_front().unwrap());
                }

                if self.db_iterator.valid() {
                    if let Some(value) = self.db_iterator.value_bytes() {
                        if let Ok(next_entries) =
                            deserialize::<Vec<Entry>>(&value[BLOB_HEADER_SIZE..])
                        {
                            if let Some(blockhash) = self.blockhash {
                                if !next_entries.verify(&blockhash) {
                                    return None;
                                }
                            }
                            self.db_iterator.next();
                            if next_entries.is_empty() {
                                return None;
                            }
                            self.entries = VecDeque::from(next_entries);
                            let entry = self.entries.pop_front().unwrap();
                            self.blockhash = Some(entry.hash);
                            return Some(entry);
                        }
                    }
                }
                None
            }
        }
        let mut db_iterator = self.data_cf.cursor()?;

        db_iterator.seek_to_first();
        Ok(EntryIterator {
            entries: VecDeque::new(),
            db_iterator,
            blockhash: None,
        })
    }

    pub fn get_slot_entries_with_blob_count(
        &self,
        slot: u64,
        blob_start_index: u64,
        max_entries: Option<u64>,
    ) -> Result<(Vec<Entry>, usize)> {
        // Find the next consecutive block of blobs.
        let consecutive_blobs =
            self.get_slot_consecutive_blobs(slot, &HashMap::new(), blob_start_index, max_entries)?;
        let num = consecutive_blobs.len();
        Ok((Self::deserialize_blobs(&consecutive_blobs), num))
    }

    // Returns slots connecting to any element of the list `slots`.
    pub fn get_slots_since(&self, slots: &[u64]) -> Result<HashMap<u64, Vec<u64>>> {
        // Return error if there was a database error during lookup of any of the
        // slot indexes
        let slot_metas: Result<Vec<Option<SlotMeta>>> =
            slots.iter().map(|slot| self.meta(*slot)).collect();

        let slot_metas = slot_metas?;
        let result: HashMap<u64, Vec<u64>> = slots
            .iter()
            .zip(slot_metas)
            .filter_map(|(height, meta)| meta.map(|meta| (*height, meta.next_slots)))
            .collect();

        Ok(result)
    }

    pub fn deserialize_blob_data(data: &[u8]) -> Result<Vec<Entry>> {
        let entries = deserialize(data)?;
        Ok(entries)
    }

    pub fn is_root(&self, slot: u64) -> bool {
        if let Ok(Some(meta)) = self.meta(slot) {
            meta.is_root
        } else {
            false
        }
    }

    pub fn set_root(&self, slot: u64) -> Result<()> {
        *self.root_slot.write().unwrap() = slot;

        if let Some(mut meta) = self.meta_cf.get(slot)? {
            meta.is_root = true;
            self.meta_cf.put(slot, &meta)?;
        }
        Ok(())
    }

    pub fn get_orphans(&self, max: Option<usize>) -> Vec<u64> {
        let mut results = vec![];
        let mut iter = self.orphans_cf.cursor().unwrap();
        iter.seek_to_first();
        while iter.valid() {
            if let Some(max) = max {
                if results.len() > max {
                    break;
                }
            }
            results.push(iter.key().unwrap());
            iter.next();
        }
        results
    }

    fn deserialize_blobs<I>(blob_datas: &[I]) -> Vec<Entry>
    where
        I: Borrow<[u8]>,
    {
        blob_datas
            .iter()
            .flat_map(|blob_data| {
                let serialized_entries_data = &blob_data.borrow()[BLOB_HEADER_SIZE..];
                Self::deserialize_blob_data(serialized_entries_data)
                    .expect("Ledger should only contain well formed data")
            })
            .collect()
    }

    fn slot_has_updates(slot_meta: &SlotMeta, slot_meta_backup: &Option<SlotMeta>) -> bool {
        // We should signal that there are updates if we extended the chain of consecutive blocks starting
        // from block 0, which is true iff:
        // 1) The block with index prev_block_index is itself part of the trunk of consecutive blocks
        // starting from block 0,
        slot_meta.is_connected &&
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
        let working_set_slots: Vec<_> = working_set.iter().map(|s| *s.0).collect();
        for slot in working_set_slots {
            self.handle_chaining_for_slot(write_batch, working_set, &mut new_chained_slots, slot)?;
        }

        // Write all the newly changed slots in new_chained_slots to the write_batch
        for (slot, meta) in new_chained_slots.iter() {
            let meta: &SlotMeta = &RefCell::borrow(&*meta);
            write_batch.put::<cf::SlotMeta>(*slot, meta)?;
        }
        Ok(())
    }

    fn handle_chaining_for_slot(
        &self,
        write_batch: &mut WriteBatch,
        working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot: u64,
    ) -> Result<()> {
        let (meta, meta_backup) = working_set
            .get(&slot)
            .expect("Slot must exist in the working_set hashmap");

        {
            let is_orphan = meta_backup.is_some() && Self::is_orphan(meta_backup.as_ref().unwrap());

            let mut meta_mut = meta.borrow_mut();

            // If:
            // 1) This is a new slot
            // 2) slot != 0
            // then try to chain this slot to a previous slot
            if slot != 0 {
                let prev_slot = meta_mut.parent_slot;

                // Check if the slot represented by meta_mut is either a new slot or a orphan.
                // In both cases we need to run the chaining logic b/c the parent on the slot was
                // previously unknown.
                if meta_backup.is_none() || is_orphan {
                    let prev_slot_meta =
                        self.find_slot_meta_else_create(working_set, new_chained_slots, prev_slot)?;

                    // This is a newly inserted slot so run the chaining logic
                    self.chain_new_slot_to_prev_slot(
                        &mut prev_slot_meta.borrow_mut(),
                        slot,
                        &mut meta_mut,
                    );

                    if Self::is_orphan(&RefCell::borrow(&*prev_slot_meta)) {
                        write_batch.put::<cf::Orphans>(prev_slot, &true)?;
                    }
                }
            }

            // At this point this slot has received a parent, so no longer a orphan
            if is_orphan {
                write_batch.delete::<cf::Orphans>(slot)?;
            }
        }

        // This is a newly inserted slot and slot.is_connected is true, so update all
        // child slots so that their `is_connected` = true
        let should_propagate_is_connected =
            Self::is_newly_completed_slot(&RefCell::borrow(&*meta), meta_backup)
                && RefCell::borrow(&*meta).is_connected;

        if should_propagate_is_connected {
            // slot_function returns a boolean indicating whether to explore the children
            // of the input slot
            let slot_function = |slot: &mut SlotMeta| {
                slot.is_connected = true;

                // We don't want to set the is_connected flag on the children of non-full
                // slots
                slot.is_full()
            };

            self.traverse_children_mut(slot, &meta, working_set, new_chained_slots, slot_function)?;
        }

        Ok(())
    }

    fn traverse_children_mut<F>(
        &self,
        slot: u64,
        slot_meta: &Rc<RefCell<(SlotMeta)>>,
        working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot_function: F,
    ) -> Result<()>
    where
        F: Fn(&mut SlotMeta) -> bool,
    {
        let mut next_slots: Vec<(u64, Rc<RefCell<(SlotMeta)>>)> = vec![(slot, slot_meta.clone())];
        while !next_slots.is_empty() {
            let (_, current_slot) = next_slots.pop().unwrap();
            // Check whether we should explore the children of this slot
            if slot_function(&mut current_slot.borrow_mut()) {
                let current_slot = &RefCell::borrow(&*current_slot);
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

        Ok(())
    }

    fn is_orphan(meta: &SlotMeta) -> bool {
        // If we have no parent, then this is the head of a detached chain of
        // slots
        !meta.is_parent_set()
    }

    // 1) Chain current_slot to the previous slot defined by prev_slot_meta
    // 2) Determine whether to set the is_connected flag
    fn chain_new_slot_to_prev_slot(
        &self,
        prev_slot_meta: &mut SlotMeta,
        current_slot: u64,
        current_slot_meta: &mut SlotMeta,
    ) {
        prev_slot_meta.next_slots.push(current_slot);
        current_slot_meta.is_connected = prev_slot_meta.is_connected && prev_slot_meta.is_full();
    }

    fn is_newly_completed_slot(slot_meta: &SlotMeta, backup_slot_meta: &Option<SlotMeta>) -> bool {
        slot_meta.is_full()
            && (backup_slot_meta.is_none()
                || Self::is_orphan(&backup_slot_meta.as_ref().unwrap())
                || slot_meta.consumed != backup_slot_meta.as_ref().unwrap().consumed)
    }

    // 1) Find the slot metadata in the cache of dirty slot metadata we've previously touched,
    // else:
    // 2) Search the database for that slot metadata. If still no luck, then:
    // 3) Create a dummy orphan slot in the database
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
    // create a dummy orphan slot in the database
    fn find_slot_meta_in_db_else_create<'a>(
        &self,
        slot: u64,
        insert_map: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    ) -> Result<Rc<RefCell<SlotMeta>>> {
        if let Some(slot_meta) = self.meta(slot)? {
            insert_map.insert(slot, Rc::new(RefCell::new(slot_meta)));
            Ok(insert_map.get(&slot).unwrap().clone())
        } else {
            // If this slot doesn't exist, make a orphan slot. This way we
            // remember which slots chained to this one when we eventually get a real blob
            // for this slot
            insert_map.insert(
                slot,
                Rc::new(RefCell::new(SlotMeta::new(slot, std::u64::MAX))),
            );
            Ok(insert_map.get(&slot).unwrap().clone())
        }
    }

    // Find the slot metadata in the cache of dirty slot metadata we've previously touched
    fn find_slot_meta_in_cached_state<'a>(
        &self,
        working_set: &'a HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        chained_slots: &'a HashMap<u64, Rc<RefCell<SlotMeta>>>,
        slot: u64,
    ) -> Result<Option<Rc<RefCell<SlotMeta>>>> {
        if let Some((entry, _)) = working_set.get(&slot) {
            Ok(Some(entry.clone()))
        } else if let Some(entry) = chained_slots.get(&slot) {
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

        let serialized_blob_data = &blob_to_insert.data[..BLOB_HEADER_SIZE + blob_size];

        // Commit step: commit all changes to the mutable structures at once, or none at all.
        // We don't want only some of these changes going through.
        write_batch.put_bytes::<cf::Data>((blob_slot, blob_index), serialized_blob_data)?;
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

    /// Attempts recovery using erasure coding
    fn recover(&self, slot: u64, set_index: u64) -> Result<usize> {
        use crate::erasure::{ERASURE_SET_SIZE, NUM_DATA};

        let erasure_meta = self.erasure_meta_cf.get((slot, set_index))?.unwrap();

        let start_idx = erasure_meta.start_index();
        let (data_end_idx, coding_end_idx) = erasure_meta.end_indexes();

        let present = &mut [true; ERASURE_SET_SIZE];
        let mut blobs = Vec::with_capacity(ERASURE_SET_SIZE);
        let mut size = 0;

        for i in start_idx..coding_end_idx {
            if erasure_meta.is_coding_present(i) {
                let mut blob_bytes = self
                    .erasure_cf
                    .get_bytes((slot, i))?
                    .expect("erasure_meta must have no false positives");

                blob_bytes.drain(..BLOB_HEADER_SIZE);

                if size == 0 {
                    size = blob_bytes.len();
                }

                blobs.push(blob_bytes);
            } else {
                let set_relative_idx = (i - start_idx) as usize + NUM_DATA;
                blobs.push(vec![0; size]);
                present[set_relative_idx] = false;
            }
        }

        assert_ne!(size, 0);

        for i in start_idx..data_end_idx {
            let set_relative_idx = (i - start_idx) as usize;

            if erasure_meta.is_data_present(i) {
                let mut blob_bytes = self
                    .data_cf
                    .get_bytes((slot, i))?
                    .expect("erasure_meta must have no false positives");

                // If data is too short, extend it with zeroes
                blob_bytes.resize(size, 0u8);

                blobs.insert(set_relative_idx, blob_bytes);
            } else {
                blobs.insert(set_relative_idx, vec![0u8; size]);
                // data erasures must come before any coding erasures if present
                present[set_relative_idx] = false;
            }
        }

        let (recovered_data, recovered_coding) = self
            .session
            .reconstruct_blobs(&mut blobs, present, size, start_idx, slot)?;

        let amount_recovered = recovered_data.len() + recovered_coding.len();

        trace!(
            "[recover] reconstruction OK slot: {}, indexes: [{},{})",
            slot,
            start_idx,
            data_end_idx
        );

        self.write_blobs(recovered_data)?;

        for blob in recovered_coding {
            self.put_coding_blob_bytes_raw(slot, blob.index(), &blob.data[..])?;
        }

        Ok(amount_recovered)
    }

    /// Returns the next consumed index and the number of ticks in the new consumed
    /// range
    fn get_slot_consecutive_blobs<'a>(
        &self,
        slot: u64,
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
            if let Some(prev_blob_data) = prev_inserted_blob_datas.get(&(slot, current_index)) {
                blobs.push(Cow::Borrowed(*prev_blob_data));
            } else if let Some(blob_data) = self.data_cf.get_bytes((slot, current_index))? {
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
        let mut bootstrap_meta = SlotMeta::new(0, 1);
        let last = blobs.last().unwrap();

        bootstrap_meta.consumed = last.index() + 1;
        bootstrap_meta.received = last.index() + 1;
        bootstrap_meta.is_connected = true;

        let mut batch = self.db.batch()?;
        batch.put::<cf::SlotMeta>(0, &bootstrap_meta)?;
        for blob in blobs {
            let serialized_blob_datas = &blob.data[..BLOB_HEADER_SIZE + blob.size()];
            batch.put_bytes::<cf::Data>((blob.slot(), blob.index()), serialized_blob_datas)?;
        }
        self.db.write(batch)?;
        Ok(())
    }
}

// Creates a new ledger with slot 0 full of ticks (and only ticks).
//
// Returns the blockhash that can be used to append entries with.
pub fn create_new_ledger(ledger_path: &str, genesis_block: &GenesisBlock) -> Result<Hash> {
    let ticks_per_slot = genesis_block.ticks_per_slot;
    Blocktree::destroy(ledger_path)?;
    genesis_block.write(&ledger_path)?;

    // Fill slot 0 with ticks that link back to the genesis_block to bootstrap the ledger.
    let blocktree = Blocktree::open(ledger_path)?;
    let entries = crate::entry::create_ticks(ticks_per_slot, genesis_block.hash());
    blocktree.write_entries(0, 0, 0, ticks_per_slot, &entries)?;

    Ok(entries.last().unwrap().hash)
}

pub fn genesis<'a, I>(ledger_path: &str, keypair: &Keypair, entries: I) -> Result<()>
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
            b.set_id(&keypair.pubkey());
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
    let blockhash = create_new_ledger(&ledger_path, genesis_block).unwrap();
    (ledger_path, blockhash)
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
        create_ticks, make_tiny_test_entries, make_tiny_test_entries_from_hash, Entry, EntrySlice,
    };
    use crate::packet;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use std::cmp::min;
    use std::collections::HashSet;
    use std::iter::once;
    use std::iter::FromIterator;
    use std::time::Duration;

    #[test]
    fn test_write_entries() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let ticks_per_slot = 10;
            let num_slots = 10;
            let num_ticks = ticks_per_slot * num_slots;
            let ledger = Blocktree::open(&ledger_path).unwrap();

            let ticks = create_ticks(num_ticks, Hash::default());
            ledger
                .write_entries(0, 0, 0, ticks_per_slot, ticks.clone())
                .unwrap();

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
                    ticks_per_slot,
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
        ledger.meta_cf.put(0, &meta).unwrap();
        let result = ledger
            .meta_cf
            .get(0)
            .unwrap()
            .expect("Expected meta object to exist");

        assert_eq!(result, meta);

        // Test erasure column family
        let erasure = vec![1u8; 16];
        let erasure_key = (0, 0);
        ledger.erasure_cf.put_bytes(erasure_key, &erasure).unwrap();

        let result = ledger
            .erasure_cf
            .get_bytes(erasure_key)
            .unwrap()
            .expect("Expected erasure object to exist");

        assert_eq!(result, erasure);

        // Test data column family
        let data = vec![2u8; 16];
        let data_key = (0, 0);
        ledger.data_cf.put_bytes(data_key, &data).unwrap();

        let result = ledger
            .data_cf
            .get_bytes(data_key)
            .unwrap()
            .expect("Expected data object to exist");

        assert_eq!(result, data);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_read_blobs_bytes() {
        let shared_blobs = make_tiny_test_entries(10).to_single_entry_shared_blobs();
        let slot = 0;
        packet::index_blobs(&shared_blobs, &Pubkey::new_rand(), 0, slot, 0);

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
            .get(0)
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
            .get(0)
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert_eq!(meta.consumed, num_entries);
        assert_eq!(meta.received, num_entries);
        assert_eq!(meta.parent_slot, 0);
        assert_eq!(meta.last_index, num_entries - 1);
        assert!(meta.next_slots.is_empty());
        assert!(meta.is_connected);

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
                .get(0)
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
            let mut blobs = entries.to_single_entry_blobs();

            for (i, b) in blobs.iter_mut().enumerate() {
                b.set_index(1 << (i * 8));
                b.set_slot(0);
            }

            blocktree
                .write_blobs(&blobs)
                .expect("Expected successful write of blobs");

            let mut db_iterator = blocktree
                .db
                .cursor::<cf::Data>()
                .expect("Expected to be able to open database iterator");

            db_iterator.seek((slot, 1));

            // Iterate through ledger
            for i in 0..num_entries {
                assert!(db_iterator.valid());
                let (_, current_index) = db_iterator.key().expect("Expected a valid key");
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
            let mut blobs = entries.clone().to_single_entry_blobs();
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
            for slot in 0..num_slots {
                let entries = make_tiny_test_entries(slot as usize + 1);
                let last_entry = entries.last().unwrap().clone();
                let mut blobs = entries.clone().to_single_entry_blobs();
                for b in blobs.iter_mut() {
                    b.set_index(index);
                    b.set_slot(slot as u64);
                    index += 1;
                }
                blocktree
                    .write_blobs(&blobs)
                    .expect("Expected successful write of blobs");
                assert_eq!(
                    blocktree.get_slot_entries(slot, index - 1, None).unwrap(),
                    vec![last_entry],
                );
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slot_entries3() {
        // Test inserting/fetching blobs which contain multiple entries per blob
        let blocktree_path = get_tmp_ledger_path("test_get_slot_entries3");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 5 as u64;
            let blobs_per_slot = 5 as u64;
            let entry_serialized_size =
                bincode::serialized_size(&make_tiny_test_entries(1)).unwrap();
            let entries_per_slot =
                (blobs_per_slot * packet::BLOB_DATA_SIZE as u64) / entry_serialized_size;

            // Write entries
            for slot in 0..num_slots {
                let mut index = 0;
                let entries = make_tiny_test_entries(entries_per_slot as usize);
                let mut blobs = entries.clone().to_blobs();
                assert_eq!(blobs.len() as u64, blobs_per_slot);
                for b in blobs.iter_mut() {
                    b.set_index(index);
                    b.set_slot(slot as u64);
                    index += 1;
                }
                blocktree
                    .write_blobs(&blobs)
                    .expect("Expected successful write of blobs");
                assert_eq!(blocktree.get_slot_entries(slot, 0, None).unwrap(), entries,);
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_blobs_consecutive() {
        let blocktree_path = get_tmp_ledger_path("test_insert_data_blobs_consecutive");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            for i in 0..4 {
                let slot = i;
                let parent_slot = if i == 0 { 0 } else { i - 1 };
                // Write entries
                let num_entries = 21 as u64 * (i + 1);
                let (blobs, original_entries) = make_slot_entries(slot, parent_slot, num_entries);

                blocktree
                    .write_blobs(blobs.iter().skip(1).step_by(2))
                    .unwrap();

                assert_eq!(blocktree.get_slot_entries(slot, 0, None).unwrap(), vec![]);

                let meta = blocktree.meta_cf.get(slot).unwrap().unwrap();
                if num_entries % 2 == 0 {
                    assert_eq!(meta.received, num_entries);
                } else {
                    debug!("got here");
                    assert_eq!(meta.received, num_entries - 1);
                }
                assert_eq!(meta.consumed, 0);
                assert_eq!(meta.parent_slot, parent_slot);
                if num_entries % 2 == 0 {
                    assert_eq!(meta.last_index, num_entries - 1);
                } else {
                    assert_eq!(meta.last_index, std::u64::MAX);
                }

                blocktree.write_blobs(blobs.iter().step_by(2)).unwrap();

                assert_eq!(
                    blocktree.get_slot_entries(slot, 0, None).unwrap(),
                    original_entries,
                );

                let meta = blocktree.meta_cf.get(slot).unwrap().unwrap();
                assert_eq!(meta.received, num_entries);
                assert_eq!(meta.consumed, num_entries);
                assert_eq!(meta.parent_slot, parent_slot);
                assert_eq!(meta.last_index, num_entries - 1);
            }
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

            let meta = blocktree.meta_cf.get(0).unwrap().unwrap();
            assert_eq!(meta.consumed, num_unique_entries);
            assert_eq!(meta.received, num_unique_entries);
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_unique_entries - 1);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_genesis_and_entry_iterator() {
        let entries = make_tiny_test_entries_from_hash(&Hash::default(), 10);

        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            genesis(&ledger_path, &Keypair::new(), &entries).unwrap();

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
        let entries = make_tiny_test_entries_from_hash(&Hash::default(), 3);
        let ledger_path = get_tmp_ledger_path("test_genesis_and_entry_iterator");
        {
            // put entries except last 2 into ledger
            genesis(&ledger_path, &Keypair::new(), &entries[..entries.len() - 2]).unwrap();

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
                    16,
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
        for slot in 1..num_slots + 1 {
            let (mut slot_blobs, _) = make_slot_entries(slot, slot - 1, entries_per_slot);
            let missing_blob = slot_blobs.remove(slot as usize - 1);
            blobs.extend(slot_blobs);
            missing_blobs.push(missing_blob);
        }

        // Should be no updates, since no new chains from block 0 were formed
        ledger.insert_data_blobs(blobs.iter()).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // Insert a blob for each slot that doesn't make a consecutive block, we
        // should get no updates
        let blobs: Vec<_> = (1..num_slots + 1)
            .flat_map(|slot| {
                let (mut blob, _) = make_slot_entries(slot, slot - 1, 1);
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
            assert!(!s1.is_connected);
            assert_eq!(s1.parent_slot, 0);
            assert_eq!(s1.last_index, entries_per_slot - 1);

            // 2) Write to the second slot
            blocktree
                .write_blobs(&blobs[2 * entries_per_slot as usize..3 * entries_per_slot as usize])
                .unwrap();
            let s2 = blocktree.meta(2).unwrap().unwrap();
            assert!(s2.next_slots.is_empty());
            // Slot 2 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s2.is_connected);
            assert_eq!(s2.parent_slot, 1);
            assert_eq!(s2.last_index, entries_per_slot - 1);

            // Check the first slot again, it should chain to the second slot,
            // but still isn't part of the trunk
            let s1 = blocktree.meta(1).unwrap().unwrap();
            assert_eq!(s1.next_slots, vec![2]);
            assert!(!s1.is_connected);
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
                assert!(s.is_connected);
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
            for slot in 0..num_slots {
                let parent_slot = {
                    if slot == 0 {
                        0
                    } else {
                        slot - 1
                    }
                };
                let (slot_blobs, _) = make_slot_entries(slot, parent_slot, entries_per_slot);

                if slot % 2 == 1 {
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
                // However, if it's a slot we haven't inserted, aka one of the gaps, then one of the
                // slots we just inserted will chain to that gap, so next_slots for that orphan slot
                // won't be empty, but the parent slot is unknown so should equal std::u64::MAX.
                let s = blocktree.meta(i as u64).unwrap().unwrap();
                if i % 2 == 0 {
                    assert_eq!(s.next_slots, vec![i as u64 + 1]);
                    assert_eq!(s.parent_slot, std::u64::MAX);
                } else {
                    assert!(s.next_slots.is_empty());
                    assert_eq!(s.parent_slot, i - 1);
                }

                if i == 0 {
                    assert!(s.is_connected);
                } else {
                    assert!(!s.is_connected);
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
                assert!(s.is_connected);
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_forward_chaining_is_connected() {
        let blocktree_path = get_tmp_ledger_path("test_forward_chaining_is_connected");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 15;
            let entries_per_slot = 2;
            assert!(entries_per_slot > 1);

            let (blobs, _) = make_many_slot_entries(0, num_slots, entries_per_slot);

            // Write the blobs such that every 3rd slot has a gap in the beginning
            for (slot, slot_ticks) in blobs.chunks(entries_per_slot as usize).enumerate() {
                if slot % 3 == 0 {
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
                    assert!(!s.is_connected);
                } else {
                    assert!(s.is_connected);
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
                            assert!(s.is_connected);
                        } else {
                            assert!(!s.is_connected);
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
            for slot in slots {
                // Get blobs for the slot "slot"
                let slot_blobs = &mut blobs
                    [(slot * entries_per_slot) as usize..((slot + 1) * entries_per_slot) as usize];
                for blob in slot_blobs.iter_mut() {
                    // Get the parent slot of the slot in the tree
                    let slot_parent = {
                        if slot == 0 {
                            0
                        } else {
                            (slot - 1) / branching_factor
                        }
                    };
                    blob.set_parent(slot_parent);
                }

                blocktree.write_blobs(slot_blobs).unwrap();
            }

            // Make sure everything chains correctly
            let last_level =
                (branching_factor.pow(num_tree_levels - 1) - 1) / (branching_factor - 1);
            for slot in 0..num_slots {
                let slot_meta = blocktree.meta(slot).unwrap().unwrap();
                assert_eq!(slot_meta.consumed, entries_per_slot);
                assert_eq!(slot_meta.received, entries_per_slot);
                assert!(slot_meta.is_connected);
                let slot_parent = {
                    if slot == 0 {
                        0
                    } else {
                        (slot - 1) / branching_factor
                    }
                };
                assert_eq!(slot_meta.parent_slot, slot_parent);

                let expected_children: HashSet<_> = {
                    if slot >= last_level {
                        HashSet::new()
                    } else {
                        let first_child_slot = min(num_slots - 1, slot * branching_factor + 1);
                        let last_child_slot = min(num_slots - 1, (slot + 1) * branching_factor);
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

            // No orphan slots should exist
            assert!(blocktree.orphans_cf.is_empty().unwrap())
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

            let mut meta0 = SlotMeta::new(0, 0);
            blocktree.meta_cf.put(0, &meta0).unwrap();

            // Slot exists, chains to nothing
            let expected: HashMap<u64, Vec<u64>> =
                HashMap::from_iter(vec![(0, vec![])].into_iter());
            assert_eq!(blocktree.get_slots_since(&vec![0]).unwrap(), expected);
            meta0.next_slots = vec![1, 2];
            blocktree.meta_cf.put(0, &meta0).unwrap();

            // Slot exists, chains to some other slots
            let expected: HashMap<u64, Vec<u64>> =
                HashMap::from_iter(vec![(0, vec![1, 2])].into_iter());
            assert_eq!(blocktree.get_slots_since(&vec![0]).unwrap(), expected);
            assert_eq!(blocktree.get_slots_since(&vec![0, 1]).unwrap(), expected);

            let mut meta3 = SlotMeta::new(3, 1);
            meta3.next_slots = vec![10, 5];
            blocktree.meta_cf.put(3, &meta3).unwrap();
            let expected: HashMap<u64, Vec<u64>> =
                HashMap::from_iter(vec![(0, vec![1, 2]), (3, vec![10, 5])].into_iter());
            assert_eq!(blocktree.get_slots_since(&vec![0, 1, 3]).unwrap(), expected);
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_orphans() {
        let blocktree_path = get_tmp_ledger_path("test_orphans");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create blobs and entries
            let entries_per_slot = 1;
            let (blobs, _) = make_many_slot_entries(0, 3, entries_per_slot);

            // Write slot 2, which chains to slot 1. We're missing slot 0,
            // so slot 1 is the orphan
            blocktree.write_blobs(once(&blobs[2])).unwrap();
            let meta = blocktree
                .meta(1)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(Blocktree::is_orphan(&meta));
            assert_eq!(blocktree.get_orphans(None), vec![1]);

            // Write slot 1 which chains to slot 0, so now slot 0 is the
            // orphan, and slot 1 is no longer the orphan.
            blocktree.write_blobs(once(&blobs[1])).unwrap();
            let meta = blocktree
                .meta(1)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(!Blocktree::is_orphan(&meta));
            let meta = blocktree
                .meta(0)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(Blocktree::is_orphan(&meta));
            assert_eq!(blocktree.get_orphans(None), vec![0]);

            // Write some slot that also chains to existing slots and orphan,
            // nothing should change
            let blob4 = &make_slot_entries(4, 0, 1).0[0];
            let blob5 = &make_slot_entries(5, 1, 1).0[0];
            blocktree.write_blobs(vec![blob4, blob5]).unwrap();
            assert_eq!(blocktree.get_orphans(None), vec![0]);

            // Write zeroth slot, no more orphans
            blocktree.write_blobs(once(&blobs[0])).unwrap();
            for i in 0..3 {
                let meta = blocktree
                    .meta(i)
                    .expect("Expect database get to succeed")
                    .unwrap();
                assert!(!Blocktree::is_orphan(&meta));
            }
            // Orphans cf is empty
            assert!(blocktree.orphans_cf.is_empty().unwrap())
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
            for slot in 0..num_entries {
                let parent_slot = {
                    if slot == 0 {
                        0
                    } else {
                        slot - 1
                    }
                };

                let (mut blob, entry) = make_slot_entries(slot, parent_slot, 1);
                blob[0].set_index(slot);
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

                let meta = blocktree.meta_cf.get(i).unwrap().unwrap();
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

    #[test]
    fn test_find_missing_data_indexes() {
        let slot = 0;
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Write entries
        let gap = 10;
        assert!(gap > 3);
        let num_entries = 10;
        let mut blobs = make_tiny_test_entries(num_entries).to_single_entry_blobs();
        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64 * gap);
            b.set_slot(slot);
        }
        blocktree.write_blobs(&blobs).unwrap();

        // Index of the first blob is 0
        // Index of the second blob is "gap"
        // Thus, the missing indexes should then be [1, gap - 1] for the input index
        // range of [0, gap)
        let expected: Vec<u64> = (1..gap).collect();
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap, gap as usize),
            expected
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 1, gap, (gap - 1) as usize),
            expected,
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap - 1, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, gap - 2, gap, gap as usize),
            vec![gap - 2, gap - 1],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, gap - 2, gap, 1),
            vec![gap - 2],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap, 1),
            vec![1],
        );

        // Test with end indexes that are greater than the last item in the ledger
        let mut expected: Vec<u64> = (1..gap).collect();
        expected.push(gap + 1);
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap + 2, (gap + 2) as usize),
            expected,
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap + 2, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );

        for i in 0..num_entries as u64 {
            for j in 0..i {
                let expected: Vec<u64> = (j..i)
                    .flat_map(|k| {
                        let begin = k * gap + 1;
                        let end = (k + 1) * gap;
                        (begin..end)
                    })
                    .collect();
                assert_eq!(
                    blocktree.find_missing_data_indexes(
                        slot,
                        j * gap,
                        i * gap,
                        ((i - j) * gap) as usize
                    ),
                    expected,
                );
            }
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_find_missing_data_indexes_sanity() {
        let slot = 0;

        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Early exit conditions
        let empty: Vec<u64> = vec![];
        assert_eq!(blocktree.find_missing_data_indexes(slot, 0, 0, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 5, 5, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 4, 3, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 1, 2, 0), empty);

        let mut blobs = make_tiny_test_entries(2).to_single_entry_blobs();

        const ONE: u64 = 1;
        const OTHER: u64 = 4;

        blobs[0].set_index(ONE);
        blobs[1].set_index(OTHER);

        // Insert one blob at index = first_index
        blocktree.write_blobs(&blobs).unwrap();

        const STARTS: u64 = OTHER * 2;
        const END: u64 = OTHER * 3;
        const MAX: usize = 10;
        // The first blob has index = first_index. Thus, for i < first_index,
        // given the input range of [i, first_index], the missing indexes should be
        // [i, first_index - 1]
        for start in 0..STARTS {
            let result = blocktree.find_missing_data_indexes(
                slot, start, // start
                END,   //end
                MAX,   //max
            );
            let expected: Vec<u64> = (start..END).filter(|i| *i != ONE && *i != OTHER).collect();
            assert_eq!(result, expected);
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_no_missing_blob_indexes() {
        let slot = 0;
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Write entries
        let num_entries = 10;
        let shared_blobs = make_tiny_test_entries(num_entries).to_single_entry_shared_blobs();

        crate::packet::index_blobs(&shared_blobs, &Pubkey::new_rand(), 0, slot, 0);

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();
        blocktree.write_blobs(blobs).unwrap();

        let empty: Vec<u64> = vec![];
        for i in 0..num_entries as u64 {
            for j in 0..i {
                assert_eq!(
                    blocktree.find_missing_data_indexes(slot, j, i, (i - j) as usize),
                    empty
                );
            }
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    mod erasure {
        use super::*;
        use crate::erasure::test::{generate_ledger_model, ErasureSpec, SlotSpec};
        use crate::erasure::{CodingGenerator, NUM_CODING, NUM_DATA};
        use rand::{thread_rng, Rng};
        use std::sync::RwLock;

        impl Into<SharedBlob> for Blob {
            fn into(self) -> SharedBlob {
                Arc::new(RwLock::new(self))
            }
        }

        #[test]
        fn test_erasure_meta_accuracy() {
            let path = get_tmp_ledger_path!();
            let blocktree = Blocktree::open(&path).unwrap();

            // one erasure set + half of the next
            let num_blobs = 24;
            let slot = 0;

            let (blobs, _) = make_slot_entries(slot, 0, num_blobs);
            let shared_blobs: Vec<_> = blobs
                .iter()
                .cloned()
                .map(|blob| Arc::new(RwLock::new(blob)))
                .collect();

            blocktree.write_blobs(&blobs[8..16]).unwrap();

            let erasure_meta_opt = blocktree
                .erasure_meta_cf
                .get((slot, 0))
                .expect("DB get must succeed");

            assert!(erasure_meta_opt.is_some());
            let erasure_meta = erasure_meta_opt.unwrap();

            assert_eq!(erasure_meta.data, 0xFF00);
            assert_eq!(erasure_meta.coding, 0x0);

            blocktree.write_blobs(&blobs[..8]).unwrap();

            let erasure_meta = blocktree
                .erasure_meta_cf
                .get((slot, 0))
                .expect("DB get must succeed")
                .unwrap();

            assert_eq!(erasure_meta.data, 0xFFFF);
            assert_eq!(erasure_meta.coding, 0x0);

            blocktree.write_blobs(&blobs[16..]).unwrap();

            let erasure_meta = blocktree
                .erasure_meta_cf
                .get((slot, 1))
                .expect("DB get must succeed")
                .unwrap();

            assert_eq!(erasure_meta.data, 0x00FF);
            assert_eq!(erasure_meta.coding, 0x0);

            let mut coding_generator = CodingGenerator::new(Arc::clone(&blocktree.session));
            let coding_blobs = coding_generator.next(&shared_blobs[..NUM_DATA]);

            for shared_coding_blob in coding_blobs {
                let blob = shared_coding_blob.read().unwrap();
                let size = blob.size() + BLOB_HEADER_SIZE;
                blocktree
                    .put_coding_blob_bytes(blob.slot(), blob.index(), &blob.data[..size])
                    .unwrap();
            }

            let erasure_meta = blocktree
                .erasure_meta_cf
                .get((slot, 0))
                .expect("DB get must succeed")
                .unwrap();

            assert_eq!(erasure_meta.data, 0xFFFF);
            assert_eq!(erasure_meta.coding, 0x0F);

            let (start_idx, coding_end_idx) =
                (erasure_meta.start_index(), erasure_meta.end_indexes().1);

            for idx in start_idx..coding_end_idx {
                blocktree.delete_coding_blob(slot, idx).unwrap();
            }

            let erasure_meta = blocktree
                .erasure_meta_cf
                .get((slot, 0))
                .expect("DB get must succeed")
                .unwrap();

            assert_eq!(erasure_meta.status(), ErasureMetaStatus::DataFull);
            assert_eq!(erasure_meta.data, 0xFFFF);
            assert_eq!(erasure_meta.coding, 0x0);
        }

        #[test]
        pub fn test_recovery_basic() {
            solana_logger::setup();

            let slot = 0;

            let ledger_path = get_tmp_ledger_path!();

            let blocktree = Blocktree::open(&ledger_path).unwrap();
            let data_blobs = make_slot_entries(slot, 0, 3 * NUM_DATA as u64)
                .0
                .into_iter()
                .map(Blob::into)
                .collect::<Vec<_>>();

            let mut coding_generator = CodingGenerator::new(Arc::clone(&blocktree.session));

            for (set_index, data_blobs) in data_blobs.chunks_exact(NUM_DATA).enumerate() {
                let focused_index = (set_index + 1) * NUM_DATA - 1;
                let coding_blobs = coding_generator.next(&data_blobs);

                assert_eq!(coding_blobs.len(), NUM_CODING);

                let deleted_data = data_blobs[NUM_DATA - 1].clone();
                debug!(
                    "deleted: slot: {}, index: {}",
                    deleted_data.read().unwrap().slot(),
                    deleted_data.read().unwrap().index()
                );

                blocktree
                    .write_shared_blobs(&data_blobs[..NUM_DATA - 1])
                    .unwrap();

                // this should trigger recovery
                for shared_coding_blob in coding_blobs {
                    let blob = shared_coding_blob.read().unwrap();
                    let size = blob.size() + BLOB_HEADER_SIZE;

                    blocktree
                        .put_coding_blob_bytes(slot, blob.index(), &blob.data[..size])
                        .expect("Inserting coding blobs must succeed");
                    (slot, blob.index());
                }

                let erasure_meta = blocktree
                    .erasure_meta_cf
                    .get((slot, set_index as u64))
                    .expect("Erasure Meta should be present")
                    .unwrap();

                assert_eq!(erasure_meta.data, 0xFFFF);
                assert_eq!(erasure_meta.coding, 0x0F);

                let retrieved_data = blocktree
                    .data_cf
                    .get_bytes((slot, focused_index as u64))
                    .unwrap();

                assert!(retrieved_data.is_some());

                let data_blob = Blob::new(&retrieved_data.unwrap());

                assert_eq!(&data_blob, &*deleted_data.read().unwrap());
            }

            drop(blocktree);

            Blocktree::destroy(&ledger_path).expect("Expect successful Blocktree destruction");
        }

        #[test]
        fn test_recovery_multi_slot_multi_thread() {
            use rand::rngs::SmallRng;
            use rand::SeedableRng;
            use std::thread;

            let slots = vec![0, 3, 5, 50, 100];
            let max_erasure_sets = 16;
            solana_logger::setup();

            let path = get_tmp_ledger_path!();
            let mut rng = thread_rng();

            // Specification should generate a ledger where each slot has an random number of
            // erasure sets. Odd erasure sets will have all data blobs and no coding blobs, and even ones
            // will have between 1 data blob missing and 1 coding blob
            let specs = slots
                .iter()
                .map(|&slot| {
                    let num_erasure_sets = rng.gen_range(0, max_erasure_sets);

                    let set_specs = (0..num_erasure_sets)
                        .map(|set_index| {
                            let (num_data, num_coding) = if set_index % 2 == 0 {
                                (NUM_DATA - rng.gen_range(1, 5), NUM_CODING)
                            } else {
                                (NUM_DATA - 1, NUM_CODING - 1)
                            };
                            ErasureSpec {
                                set_index,
                                num_data,
                                num_coding,
                            }
                        })
                        .collect();

                    SlotSpec { slot, set_specs }
                })
                .collect::<Vec<_>>();

            let model = generate_ledger_model(&specs);
            let blocktree = Arc::new(Blocktree::open(&path).unwrap());

            // Write to each slot in a different thread simultaneously.
            // These writes should trigger the recovery. Every erasure set should have all of its
            // data blobs
            let mut handles = vec![];

            for slot_model in model.clone() {
                let blocktree = Arc::clone(&blocktree);
                let slot = slot_model.slot;
                let mut rng = SmallRng::from_rng(&mut rng).unwrap();
                let handle = thread::spawn(move || {
                    for erasure_set in slot_model.chunks {
                        // for even sets, write data blobs first, then write coding blobs, which
                        // should trigger recovery since all coding blobs will be inserted and
                        // between 1-4 data blobs are missing
                        if rng.gen() {
                            blocktree
                                .write_shared_blobs(erasure_set.data)
                                .expect("Writing data blobs must succeed");
                            debug!(
                                "multislot: wrote data: slot: {}, erasure_set: {}",
                                slot, erasure_set.set_index
                            );

                            for shared_coding_blob in erasure_set.coding {
                                let blob = shared_coding_blob.read().unwrap();
                                let size = blob.size() + BLOB_HEADER_SIZE;
                                blocktree
                                    .put_coding_blob_bytes(slot, blob.index(), &blob.data[..size])
                                    .expect("Writing coding blobs must succeed");
                            }
                            debug!(
                                "multislot: wrote coding: slot: {}, erasure_set: {}",
                                slot, erasure_set.set_index
                            );
                        } else {
                            // for odd sets, write coding blobs first, then write the data blobs.
                            // writing the data blobs should trigger recovery, since 3/4 coding and
                            // 15/16 data blobs will be present
                            for shared_coding_blob in erasure_set.coding {
                                let blob = shared_coding_blob.read().unwrap();
                                let size = blob.size() + BLOB_HEADER_SIZE;
                                blocktree
                                    .put_coding_blob_bytes(slot, blob.index(), &blob.data[..size])
                                    .expect("Writing coding blobs must succeed");
                            }
                            debug!(
                                "multislot: wrote coding: slot: {}, erasure_set: {}",
                                slot, erasure_set.set_index
                            );

                            blocktree
                                .write_shared_blobs(erasure_set.data)
                                .expect("Writing data blobs must succeed");
                            debug!(
                                "multislot: wrote data: slot: {}, erasure_set: {}",
                                slot, erasure_set.set_index
                            );
                        }
                    }
                });

                handles.push(handle);
            }

            handles
                .into_iter()
                .for_each(|handle| handle.join().unwrap());

            for slot_model in model {
                let slot = slot_model.slot;

                for erasure_set_model in slot_model.chunks {
                    let set_index = erasure_set_model.set_index as u64;

                    let erasure_meta = blocktree
                        .erasure_meta_cf
                        .get((slot, set_index))
                        .expect("DB get must succeed")
                        .expect("ErasureMeta must be present for each erasure set");

                    debug!(
                        "multislot: got erasure_meta: slot: {}, set_index: {}, erasure_meta: {:?}",
                        slot, set_index, erasure_meta
                    );

                    // all possibility for recovery should be exhausted
                    assert_eq!(erasure_meta.status(), ErasureMetaStatus::DataFull);
                    // Should have all data
                    assert_eq!(erasure_meta.data, 0xFFFF);
                    if set_index % 2 == 0 {
                        // Even sets have all coding
                        assert_eq!(erasure_meta.coding, 0x0F);
                    }
                }
            }

            drop(blocktree);
            Blocktree::destroy(&path).expect("Blocktree destruction must succeed");
        }
    }

    pub fn entries_to_blobs(
        entries: &Vec<Entry>,
        slot: u64,
        parent_slot: u64,
        is_full_slot: bool,
    ) -> Vec<Blob> {
        let mut blobs = entries.clone().to_single_entry_blobs();
        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64);
            b.set_slot(slot);
            b.set_parent(parent_slot);
        }
        if is_full_slot {
            blobs.last_mut().unwrap().set_is_last_in_slot();
        }
        blobs
    }

    pub fn make_slot_entries(
        slot: u64,
        parent_slot: u64,
        num_entries: u64,
    ) -> (Vec<Blob>, Vec<Entry>) {
        let entries = make_tiny_test_entries(num_entries as usize);
        let blobs = entries_to_blobs(&entries, slot, parent_slot, true);
        (blobs, entries)
    }

    pub fn make_many_slot_entries(
        start_slot: u64,
        num_slots: u64,
        entries_per_slot: u64,
    ) -> (Vec<Blob>, Vec<Entry>) {
        let mut blobs = vec![];
        let mut entries = vec![];
        for slot in start_slot..start_slot + num_slots {
            let parent_slot = if slot == 0 { 0 } else { slot - 1 };

            let (slot_blobs, slot_entries) = make_slot_entries(slot, parent_slot, entries_per_slot);
            blobs.extend(slot_blobs);
            entries.extend(slot_entries);
        }

        (blobs, entries)
    }
}
