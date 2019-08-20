//! The `block_tree` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.
use crate::broadcast_stage::broadcast_utils::entries_to_shreds;
use crate::entry::Entry;
use crate::erasure::{ErasureConfig, Session};
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::{Error, Result};
use crate::shred::{Shred, Shredder};

#[cfg(feature = "kvstore")]
use solana_kvstore as kvstore;

use bincode::deserialize;

use std::collections::HashMap;

#[cfg(not(feature = "kvstore"))]
use rocksdb;

use solana_metrics::{datapoint_error, datapoint_info};

use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};

use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::cmp;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};

pub use self::meta::*;
pub use self::rooted_slot_iterator::*;
use solana_sdk::timing::Slot;

mod db;
mod meta;
mod rooted_slot_iterator;

macro_rules! db_imports {
    { $mod:ident, $db:ident, $db_path:expr } => {
        mod $mod;

        use $mod::$db;
        use db::columns as cf;

        pub use db::columns;

        pub type Database = db::Database<$db>;
        pub type Cursor<C> = db::Cursor<$db, C>;
        pub type LedgerColumn<C> = db::LedgerColumn<$db, C>;
        pub type WriteBatch = db::WriteBatch<$db>;
        type BatchProcessor = db::BatchProcessor<$db>;

        pub trait Column: db::Column<$db> {}
        impl<C: db::Column<$db>> Column for C {}

        pub const BLOCKTREE_DIRECTORY: &str = $db_path;
    };
}

#[cfg(not(feature = "kvstore"))]
db_imports! {rocks, Rocks, "rocksdb"}
#[cfg(feature = "kvstore")]
db_imports! {kvs, Kvs, "kvstore"}

pub const MAX_COMPLETED_SLOTS_IN_CHANNEL: usize = 100_000;

pub type CompletedSlotsReceiver = Receiver<Vec<u64>>;

#[derive(Debug)]
pub enum BlocktreeError {
    BlobForIndexExists,
    InvalidBlobData(Box<bincode::ErrorKind>),
    RocksDb(rocksdb::Error),
    #[cfg(feature = "kvstore")]
    KvsDb(kvstore::Error),
    SlotNotRooted,
}

// ledger window
pub struct Blocktree {
    db: Arc<Database>,
    meta_cf: LedgerColumn<cf::SlotMeta>,
    data_cf: LedgerColumn<cf::Data>,
    dead_slots_cf: LedgerColumn<cf::DeadSlots>,
    erasure_cf: LedgerColumn<cf::Coding>,
    erasure_meta_cf: LedgerColumn<cf::ErasureMeta>,
    orphans_cf: LedgerColumn<cf::Orphans>,
    index_cf: LedgerColumn<cf::Index>,
    data_shred_cf: LedgerColumn<cf::ShredData>,
    _code_shred_cf: LedgerColumn<cf::ShredCode>,
    batch_processor: Arc<RwLock<BatchProcessor>>,
    pub new_blobs_signals: Vec<SyncSender<bool>>,
    pub completed_slots_senders: Vec<SyncSender<Vec<u64>>>,
}

// Column family for metadata about a leader slot
pub const META_CF: &str = "meta";
// Column family for the data in a leader slot
pub const DATA_CF: &str = "data";
// Column family for slots that have been marked as dead
pub const DEAD_SLOTS_CF: &str = "dead_slots";
// Column family for erasure data
pub const ERASURE_CF: &str = "erasure";
pub const ERASURE_META_CF: &str = "erasure_meta";
// Column family for orphans data
pub const ORPHANS_CF: &str = "orphans";
// Column family for root data
pub const ROOT_CF: &str = "root";
/// Column family for indexes
pub const INDEX_CF: &str = "index";
/// Column family for Data Shreds
pub const DATA_SHRED_CF: &str = "data_shred";
/// Column family for Code Shreds
pub const CODE_SHRED_CF: &str = "code_shred";

impl Blocktree {
    /// Opens a Ledger in directory, provides "infinite" window of blobs
    pub fn open(ledger_path: &Path) -> Result<Blocktree> {
        fs::create_dir_all(&ledger_path)?;
        let blocktree_path = ledger_path.join(BLOCKTREE_DIRECTORY);

        // Open the database
        let db = Database::open(&blocktree_path)?;

        let batch_processor = unsafe { Arc::new(RwLock::new(db.batch_processor())) };

        // Create the metadata column family
        let meta_cf = db.column();

        // Create the data column family
        let data_cf = db.column();

        // Create the dead slots column family
        let dead_slots_cf = db.column();

        // Create the erasure column family
        let erasure_cf = db.column();

        let erasure_meta_cf = db.column();

        // Create the orphans column family. An "orphan" is defined as
        // the head of a detached chain of slots, i.e. a slot with no
        // known parent
        let orphans_cf = db.column();
        let index_cf = db.column();

        let data_shred_cf = db.column();
        let code_shred_cf = db.column();

        let db = Arc::new(db);

        Ok(Blocktree {
            db,
            meta_cf,
            data_cf,
            dead_slots_cf,
            erasure_cf,
            erasure_meta_cf,
            orphans_cf,
            index_cf,
            data_shred_cf,
            _code_shred_cf: code_shred_cf,
            new_blobs_signals: vec![],
            batch_processor,
            completed_slots_senders: vec![],
        })
    }

    pub fn open_with_signal(
        ledger_path: &Path,
    ) -> Result<(Self, Receiver<bool>, CompletedSlotsReceiver)> {
        let mut blocktree = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        let (completed_slots_sender, completed_slots_receiver) =
            sync_channel(MAX_COMPLETED_SLOTS_IN_CHANNEL);
        blocktree.new_blobs_signals = vec![signal_sender];
        blocktree.completed_slots_senders = vec![completed_slots_sender];

        Ok((blocktree, signal_receiver, completed_slots_receiver))
    }

    pub fn destroy(ledger_path: &Path) -> Result<()> {
        // Database::destroy() fails if the path doesn't exist
        fs::create_dir_all(ledger_path)?;
        let blocktree_path = ledger_path.join(BLOCKTREE_DIRECTORY);
        Database::destroy(&blocktree_path)
    }

    pub fn meta(&self, slot: u64) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(slot)
    }

    pub fn is_full(&self, slot: u64) -> bool {
        if let Ok(meta) = self.meta_cf.get(slot) {
            if let Some(meta) = meta {
                return meta.is_full();
            }
        }
        false
    }

    /// Silently deletes all blocktree column families starting at the given slot until the `to` slot
    /// Dangerous; Use with care:
    /// Does not check for integrity and does not update slot metas that refer to deleted slots
    /// Modifies multiple column families simultaneously
    pub fn purge_slots(&self, mut from_slot: Slot, to_slot: Option<Slot>) {
        // split the purge request into batches of 1000 slots
        const PURGE_BATCH_SIZE: u64 = 1000;
        let mut batch_end = to_slot
            .unwrap_or(from_slot + PURGE_BATCH_SIZE)
            .min(from_slot + PURGE_BATCH_SIZE);
        while from_slot < batch_end {
            if let Ok(end) = self.run_purge_batch(from_slot, batch_end) {
                // no more slots to iter or reached the upper bound
                if end {
                    break;
                } else {
                    // update the next batch bounds
                    from_slot = batch_end;
                    batch_end = to_slot
                        .unwrap_or(batch_end + PURGE_BATCH_SIZE)
                        .min(batch_end + PURGE_BATCH_SIZE);
                }
            }
        }
    }

    // Returns whether or not all iterators have reached their end
    fn run_purge_batch(&self, from_slot: Slot, batch_end: Slot) -> Result<bool> {
        let mut end = true;
        let from_slot = Some(from_slot);
        let batch_end = Some(batch_end);
        unsafe {
            let mut batch_processor = self.db.batch_processor();
            let mut write_batch = batch_processor
                .batch()
                .expect("Database Error: Failed to get write batch");
            end &= match self
                .meta_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .data_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .erasure_meta_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .erasure_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .orphans_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .index_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            end &= match self
                .dead_slots_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
            {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            let roots_cf = self.db.column::<cf::Root>();
            end &= match roots_cf.delete_slot(&mut write_batch, from_slot, batch_end) {
                Ok(finished) => finished,
                Err(e) => {
                    error!(
                        "Error: {:?} while deleting meta_cf for slot {:?}",
                        e, from_slot
                    );
                    false
                }
            };
            if let Err(e) = batch_processor.write(write_batch) {
                error!(
                    "Error: {:?} while submitting write batch for slot {:?} retrying...",
                    e, from_slot
                );
                Err(e)?;
            }
            Ok(end)
        }
    }

    pub fn erasure_meta(&self, slot: u64, set_index: u64) -> Result<Option<ErasureMeta>> {
        self.erasure_meta_cf.get((slot, set_index))
    }

    pub fn orphan(&self, slot: u64) -> Result<Option<bool>> {
        self.orphans_cf.get(slot)
    }

    pub fn rooted_slot_iterator(&self, slot: u64) -> Result<RootedSlotIterator> {
        RootedSlotIterator::new(slot, self)
    }

    pub fn slot_meta_iterator(&self, slot: u64) -> Result<impl Iterator<Item = (u64, SlotMeta)>> {
        let meta_iter = self.db.iter::<cf::SlotMeta>(Some(slot))?;
        Ok(meta_iter.map(|(slot, slot_meta_bytes)| {
            (
                slot,
                deserialize(&slot_meta_bytes)
                    .unwrap_or_else(|_| panic!("Could not deserialize SlotMeta for slot {}", slot)),
            )
        }))
    }

    pub fn slot_data_iterator(
        &self,
        slot: u64,
    ) -> Result<impl Iterator<Item = ((u64, u64), Box<[u8]>)>> {
        let slot_iterator = self.db.iter::<cf::Data>(Some((slot, 0)))?;
        Ok(slot_iterator.take_while(move |((blob_slot, _), _)| *blob_slot == slot))
    }

    pub fn insert_shreds(&self, shreds: &[Shred]) -> Result<()> {
        let db = &*self.db;
        let mut batch_processor = self.batch_processor.write().unwrap();
        let mut write_batch = batch_processor.batch()?;

        let mut just_inserted_data_indexes = HashMap::new();
        let mut slot_meta_working_set = HashMap::new();
        let mut index_working_set = HashMap::new();

        shreds.iter().for_each(|shred| {
            let slot = shred.slot();

            let _ = index_working_set.entry(slot).or_insert_with(|| {
                self.index_cf
                    .get(slot)
                    .unwrap()
                    .unwrap_or_else(|| Index::new(slot))
            });
        });

        // Possibly do erasure recovery here

        let dummy_data = vec![];

        for shred in shreds {
            let slot = shred.slot();
            let index = u64::from(shred.index());

            let inserted = Blocktree::insert_data_shred(
                db,
                &just_inserted_data_indexes,
                &mut slot_meta_working_set,
                &mut index_working_set,
                shred,
                &mut write_batch,
            )?;

            if inserted {
                just_inserted_data_indexes.insert((slot, index), &dummy_data);
            }
        }

        // Handle chaining for the working set
        handle_chaining(&db, &mut write_batch, &slot_meta_working_set)?;

        let (should_signal, newly_completed_slots) = prepare_signals(
            &slot_meta_working_set,
            &self.completed_slots_senders,
            &mut write_batch,
        )?;

        for (&slot, index) in index_working_set.iter() {
            write_batch.put::<cf::Index>(slot, index)?;
        }

        batch_processor.write(write_batch)?;

        if should_signal {
            for signal in &self.new_blobs_signals {
                let _ = signal.try_send(true);
            }
        }

        send_signals(
            &self.new_blobs_signals,
            &self.completed_slots_senders,
            should_signal,
            newly_completed_slots,
        )?;

        Ok(())
    }

    fn insert_data_shred(
        db: &Database,
        prev_inserted_data_indexes: &HashMap<(u64, u64), &[u8]>,
        mut slot_meta_working_set: &mut HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
        index_working_set: &mut HashMap<u64, Index>,
        shred: &Shred,
        write_batch: &mut WriteBatch,
    ) -> Result<bool> {
        let slot = shred.slot();
        let index = u64::from(shred.index());
        let parent = if let Shred::FirstInSlot(s) = shred {
            debug!("got first in slot");
            s.header.parent
        } else {
            std::u64::MAX
        };

        let last_in_slot = if let Shred::LastInSlot(_) = shred {
            debug!("got last in slot");
            true
        } else {
            false
        };

        let entry = get_slot_meta_entry(db, &mut slot_meta_working_set, slot, parent);

        let slot_meta = &mut entry.0.borrow_mut();
        if is_orphan(slot_meta) {
            slot_meta.parent_slot = parent;
        }

        let data_cf = db.column::<cf::ShredData>();

        let check_data_cf = |slot, index| {
            data_cf
                .get_bytes((slot, index))
                .map(|opt| opt.is_some())
                .unwrap_or(false)
        };

        if should_insert(
            slot_meta,
            &prev_inserted_data_indexes,
            index as u64,
            slot,
            last_in_slot,
            check_data_cf,
        ) {
            let new_consumed = compute_consume_index(
                prev_inserted_data_indexes,
                slot_meta,
                index,
                slot,
                check_data_cf,
            );

            let serialized_shred = bincode::serialize(shred).unwrap();
            write_batch.put_bytes::<cf::ShredData>((slot, index), &serialized_shred)?;

            update_slot_meta(last_in_slot, slot_meta, index, new_consumed);
            index_working_set
                .get_mut(&slot)
                .expect("Index must be present for all data blobs")
                .data_mut()
                .set_present(index, true);
            trace!("inserted shred into slot {:?} and index {:?}", slot, index);
            Ok(true)
        } else {
            debug!("didn't insert shred");
            Ok(false)
        }
    }

    pub fn get_data_shred(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.data_shred_cf.get_bytes((slot, index))
    }

    /// Use this function to write data blobs to blocktree
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

    pub fn write_entries_using_shreds<I>(
        &self,
        start_slot: u64,
        num_ticks_in_start_slot: u64,
        start_index: u64,
        ticks_per_slot: u64,
        parent: Option<u64>,
        is_full_slot: bool,
        entries: I,
    ) -> Result<usize>
    where
        I: IntoIterator,
        I::Item: Borrow<Entry>,
    {
        assert!(num_ticks_in_start_slot < ticks_per_slot);
        let mut remaining_ticks_in_slot = ticks_per_slot - num_ticks_in_start_slot;

        let mut current_slot = start_slot;
        let mut parent_slot = parent.map_or(
            if current_slot == 0 {
                current_slot
            } else {
                current_slot - 1
            },
            |v| v,
        );
        let mut shredder = Shredder::new(
            current_slot,
            Some(parent_slot),
            0.0,
            &Arc::new(Keypair::new()),
            start_index as u32,
        )
        .expect("Failed to create entry shredder");
        let mut all_shreds = vec![];
        // Find all the entries for start_slot
        for entry in entries {
            if remaining_ticks_in_slot == 0 {
                current_slot += 1;
                parent_slot = current_slot - 1;
                remaining_ticks_in_slot = ticks_per_slot;
                shredder.finalize_slot();
                let shreds: Vec<Shred> = shredder
                    .shreds
                    .iter()
                    .map(|s| bincode::deserialize(s).unwrap())
                    .collect();
                all_shreds.extend(shreds);
                shredder = Shredder::new(
                    current_slot,
                    Some(parent_slot),
                    0.0,
                    &Arc::new(Keypair::new()),
                    0,
                )
                .expect("Failed to create entry shredder");
            }

            if entry.borrow().is_tick() {
                remaining_ticks_in_slot -= 1;
            }

            entries_to_shreds(
                vec![vec![entry.borrow().clone()]],
                ticks_per_slot - remaining_ticks_in_slot,
                ticks_per_slot,
                &mut shredder,
            );
        }

        if is_full_slot && remaining_ticks_in_slot != 0 {
            shredder.finalize_slot();
        }
        let shreds: Vec<Shred> = shredder
            .shreds
            .iter()
            .map(|s| bincode::deserialize(s).unwrap())
            .collect();
        all_shreds.extend(shreds);

        let num_shreds = all_shreds.len();
        self.insert_shreds(&all_shreds)?;
        Ok(num_shreds)
    }

    pub fn insert_data_blobs<I>(&self, new_blobs: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let db = &*self.db;
        let mut batch_processor = self.batch_processor.write().unwrap();
        let mut write_batch = batch_processor.batch()?;

        let new_blobs: Vec<_> = new_blobs.into_iter().collect();

        let mut prev_inserted_blob_datas = HashMap::new();
        let mut prev_inserted_coding = HashMap::new();

        // A map from slot to a 2-tuple of metadata: (working copy, backup copy),
        // so we can detect changes to the slot metadata later
        let mut slot_meta_working_set = HashMap::new();
        let mut erasure_meta_working_set = HashMap::new();
        let mut index_working_set = HashMap::new();
        let mut erasure_config_opt = None;

        for blob in new_blobs.iter() {
            let blob = blob.borrow();
            assert!(!blob.is_coding());

            match erasure_config_opt {
                Some(config) => {
                    if config != blob.erasure_config() {
                        // ToDo: This is a potential slashing condition
                        error!("Multiple erasure config for the same slot.");
                    }
                }
                None => erasure_config_opt = Some(blob.erasure_config()),
            }

            let blob_slot = blob.slot();

            let _ = index_working_set.entry(blob_slot).or_insert_with(|| {
                self.index_cf
                    .get(blob_slot)
                    .unwrap()
                    .unwrap_or_else(|| Index::new(blob_slot))
            });

            let set_index =
                ErasureMeta::set_index_for(blob.index(), erasure_config_opt.unwrap().num_data());
            if let Some(erasure_meta) = self.erasure_meta_cf.get((blob_slot, set_index))? {
                erasure_meta_working_set.insert((blob_slot, set_index), erasure_meta);
            }
        }

        let recovered_data_opt = handle_recovery(
            &self.db,
            &erasure_meta_working_set,
            &mut index_working_set,
            &prev_inserted_blob_datas,
            &mut prev_inserted_coding,
            &mut write_batch,
            &erasure_config_opt.unwrap_or_default(),
        )?;

        if let Some(recovered_data) = recovered_data_opt {
            insert_data_blob_batch(
                recovered_data
                    .iter()
                    .chain(new_blobs.iter().map(Borrow::borrow)),
                &self.db,
                &mut slot_meta_working_set,
                &mut index_working_set,
                &mut prev_inserted_blob_datas,
                &mut write_batch,
            )?;
        } else {
            insert_data_blob_batch(
                new_blobs.iter().map(Borrow::borrow),
                &db,
                &mut slot_meta_working_set,
                &mut index_working_set,
                &mut prev_inserted_blob_datas,
                &mut write_batch,
            )?;
        }

        // Handle chaining for the working set
        handle_chaining(&db, &mut write_batch, &slot_meta_working_set)?;

        let (should_signal, newly_completed_slots) = prepare_signals(
            &slot_meta_working_set,
            &self.completed_slots_senders,
            &mut write_batch,
        )?;

        for ((slot, set_index), erasure_meta) in erasure_meta_working_set {
            write_batch.put::<cf::ErasureMeta>((slot, set_index), &erasure_meta)?;
        }

        for (&slot, index) in index_working_set.iter() {
            write_batch.put::<cf::Index>(slot, index)?;
        }

        batch_processor.write(write_batch)?;

        if should_signal {
            for signal in &self.new_blobs_signals {
                let _ = signal.try_send(true);
            }
        }

        send_signals(
            &self.new_blobs_signals,
            &self.completed_slots_senders,
            should_signal,
            newly_completed_slots,
        )?;

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

    pub fn get_index(&self, slot: u64) -> Result<Option<Index>> {
        self.index_cf.get(slot)
    }

    pub fn get_coding_blob_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.erasure_cf.get_bytes((slot, index))
    }

    pub fn delete_coding_blob(&self, slot: u64, blob_index: u64) -> Result<()> {
        let mut batch_processor = self.batch_processor.write().unwrap();

        let mut index = self.index_cf.get(slot)?.unwrap_or_else(|| Index::new(slot));

        index.coding_mut().set_present(blob_index, false);

        let mut batch = batch_processor.batch()?;

        batch.delete::<cf::Coding>((slot, blob_index))?;
        batch.put::<cf::Index>(slot, &index)?;

        batch_processor.write(batch)?;
        Ok(())
    }

    pub fn get_data_shred_bytes(&self, slot: u64, index: u64) -> Result<Option<Vec<u8>>> {
        self.get_data_shred(slot, index)
    }

    /// Manually update the meta for a slot.
    /// Can interfere with automatic meta update and potentially break chaining.
    /// Dangerous. Use with care.
    pub fn put_meta_bytes(&self, slot: u64, bytes: &[u8]) -> Result<()> {
        self.meta_cf.put_bytes(slot, bytes)
    }

    /// For benchmarks, testing, and setup.
    /// Does no metadata tracking. Use with care.
    pub fn put_data_blob_bytes(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.data_cf.put_bytes((slot, index), bytes)
    }

    /// For benchmarks, testing, and setup.
    /// Does no metadata tracking. Use with care.
    pub fn put_coding_blob_bytes_raw(&self, slot: u64, index: u64, bytes: &[u8]) -> Result<()> {
        self.erasure_cf.put_bytes((slot, index), bytes)
    }

    /// this function will insert coding blobs and also automatically track erasure-related
    /// metadata. If recovery is available it will be done
    pub fn put_coding_blob(&self, blob: &Blob) -> Result<()> {
        self.put_many_coding_blobs(vec![blob])
    }

    /// this function will insert coding blobs and also automatically track erasure-related
    /// metadata. If recovery is available it will be done
    pub fn put_many_coding_blobs<I>(&self, blobs: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<Blob>,
    {
        let mut batch_processor = self.batch_processor.write().unwrap();
        let mut writebatch = batch_processor.batch()?;

        let mut erasure_metas = HashMap::new();
        let mut slot_meta_working_set = HashMap::new();
        let mut index_working_set = HashMap::new();

        let mut prev_inserted_coding = HashMap::new();
        let mut prev_inserted_blob_datas = HashMap::new();

        let mut erasure_config_opt = None;

        for blob_item in blobs {
            let blob = blob_item.borrow();
            assert!(blob.is_coding());

            match erasure_config_opt {
                Some(config) => {
                    if config != blob.erasure_config() {
                        // ToDo: This is a potential slashing condition
                        error!("Multiple erasure config for the same slot.");
                    }
                }
                None => erasure_config_opt = Some(blob.erasure_config()),
            }

            let (blob_slot, blob_index, blob_size) =
                (blob.slot(), blob.index(), blob.size() as usize);
            let set_index = blob_index / blob.erasure_config().num_coding() as u64;

            writebatch.put_bytes::<cf::Coding>(
                (blob_slot, blob_index),
                &blob.data[..BLOB_HEADER_SIZE + blob_size],
            )?;

            let index = index_working_set.entry(blob_slot).or_insert_with(|| {
                self.index_cf
                    .get(blob_slot)
                    .unwrap()
                    .unwrap_or_else(|| Index::new(blob_slot))
            });

            let erasure_meta = erasure_metas
                .entry((blob_slot, set_index))
                .or_insert_with(|| {
                    self.erasure_meta_cf
                        .get((blob_slot, set_index))
                        .expect("Expect database get to succeed")
                        .unwrap_or_else(|| {
                            ErasureMeta::new(set_index, &erasure_config_opt.unwrap())
                        })
                });

            // size should be the same for all coding blobs, else there's a bug
            erasure_meta.set_size(blob_size);
            index.coding_mut().set_present(blob_index, true);

            // `or_insert_with` used to prevent stack overflow
            prev_inserted_coding
                .entry((blob_slot, blob_index))
                .or_insert_with(|| blob.clone());
        }

        let recovered_data_opt = handle_recovery(
            &self.db,
            &erasure_metas,
            &mut index_working_set,
            &prev_inserted_blob_datas,
            &mut prev_inserted_coding,
            &mut writebatch,
            &erasure_config_opt.unwrap_or_default(),
        )?;

        if let Some(recovered_data) = recovered_data_opt {
            insert_data_blob_batch(
                recovered_data.iter(),
                &self.db,
                &mut slot_meta_working_set,
                &mut index_working_set,
                &mut prev_inserted_blob_datas,
                &mut writebatch,
            )?;

            // Handle chaining for the working set
            handle_chaining(&self.db, &mut writebatch, &slot_meta_working_set)?;
        }

        let (should_signal, newly_completed_slots) = prepare_signals(
            &slot_meta_working_set,
            &self.completed_slots_senders,
            &mut writebatch,
        )?;

        for ((slot, set_index), erasure_meta) in erasure_metas {
            writebatch.put::<cf::ErasureMeta>((slot, set_index), &erasure_meta)?;
        }

        for (&slot, index) in index_working_set.iter() {
            writebatch.put::<cf::Index>(slot, index)?;
        }

        batch_processor.write(writebatch)?;

        send_signals(
            &self.new_blobs_signals,
            &self.completed_slots_senders,
            should_signal,
            newly_completed_slots,
        )?;

        Ok(())
    }

    pub fn put_shared_coding_blobs<I>(&self, shared_blobs: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<SharedBlob>,
    {
        let blobs: Vec<_> = shared_blobs
            .into_iter()
            .map(move |s| s.borrow().clone())
            .collect();

        let locks: Vec<_> = blobs.iter().map(move |b| b.read().unwrap()).collect();

        let blob_refs = locks.iter().map(|s| &**s);

        self.put_many_coding_blobs(blob_refs)
    }

    pub fn put_many_coding_blob_bytes_raw(&self, coding_blobs: &[SharedBlob]) -> Result<()> {
        for shared_coding_blob in coding_blobs {
            let blob = shared_coding_blob.read().unwrap();
            assert!(blob.is_coding());
            let size = blob.size() + BLOB_HEADER_SIZE;
            self.put_coding_blob_bytes_raw(blob.slot(), blob.index(), &blob.data[..size])?
        }

        Ok(())
    }

    pub fn get_data_shred_as_blob(&self, slot: u64, blob_index: u64) -> Result<Option<Blob>> {
        let bytes = self.get_data_shred(slot, blob_index)?;
        Ok(bytes.map(|bytes| Blob::new(&bytes)))
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
        if let Ok(mut db_iterator) = self.db.cursor::<cf::Data>() {
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
        _max_entries: Option<u64>,
    ) -> Result<Vec<Entry>> {
        self.get_slot_entries_with_shred_count(slot, blob_start_index)
            .map(|x| x.0)
    }

    pub fn read_ledger_blobs(&self) -> impl Iterator<Item = Blob> + '_ {
        let iter = self.db.iter::<cf::Data>(None).unwrap();
        iter.map(|(_, blob_data)| Blob::new(&blob_data))
    }

    pub fn get_slot_entries_with_blob_count(
        &self,
        slot: u64,
        blob_start_index: u64,
        max_entries: Option<u64>,
    ) -> Result<(Vec<Entry>, usize)> {
        // Find the next consecutive block of blobs.
        let consecutive_blobs = get_slot_consecutive_blobs(
            slot,
            &self.db,
            &HashMap::new(),
            blob_start_index,
            max_entries,
        )?;
        let num = consecutive_blobs.len();
        let blobs =
            deserialize_blobs(&consecutive_blobs).map_err(BlocktreeError::InvalidBlobData)?;
        Ok((blobs, num))
    }

    pub fn get_slot_entries_with_shred_count(
        &self,
        slot: u64,
        start_index: u64,
    ) -> Result<(Vec<Entry>, usize)> {
        // Find the next consecutive block of blobs.
        let serialized_shreds = get_slot_consecutive_shreds(slot, &self.db, start_index)?;
        trace!(
            "Found {:?} shreds for slot {:?}",
            serialized_shreds.len(),
            slot
        );
        let mut shreds: Vec<Shred> = serialized_shreds
            .iter()
            .map(|serialzied_shred| {
                let shred: Shred =
                    bincode::deserialize(serialzied_shred).expect("Failed to deserialize shred");
                shred
            })
            .collect();

        let mut all_entries = vec![];
        let mut num = 0;
        loop {
            let mut look_for_last_shred = true;

            let mut shred_chunk = vec![];
            while look_for_last_shred && !shreds.is_empty() {
                let shred = shreds.remove(0);
                if let Shred::LastInFECSet(_) = shred {
                    look_for_last_shred = false;
                } else if let Shred::LastInSlot(_) = shred {
                    look_for_last_shred = false;
                }
                shred_chunk.push(shred);
            }

            debug!(
                "{:?} shreds in last FEC set. Looking for last shred {:?}",
                shred_chunk.len(),
                look_for_last_shred
            );

            // Break if we didn't find the last shred (as more data is required)
            if look_for_last_shred {
                break;
            }

            if let Ok(deshred) = Shredder::deshred(&shred_chunk) {
                let entries: Vec<Entry> = bincode::deserialize(&deshred.payload)?;
                trace!("Found entries: {:#?}", entries);
                all_entries.extend(entries);
                num += shred_chunk.len();
            } else {
                debug!("Failed in deshredding shred payloads");
                break;
            }
        }

        trace!("Found {:?} entries", all_entries.len());

        Ok((all_entries, num))
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
            .filter_map(|(height, meta)| {
                meta.map(|meta| {
                    let valid_next_slots: Vec<u64> = meta
                        .next_slots
                        .iter()
                        .cloned()
                        .filter(|s| !self.is_dead(*s))
                        .collect();
                    (*height, valid_next_slots)
                })
            })
            .collect();

        Ok(result)
    }

    pub fn deserialize_blob_data(data: &[u8]) -> bincode::Result<Vec<Entry>> {
        let entries = deserialize(data)?;
        Ok(entries)
    }

    pub fn is_root(&self, slot: u64) -> bool {
        if let Ok(Some(true)) = self.db.get::<cf::Root>(slot) {
            true
        } else {
            false
        }
    }

    pub fn set_roots(&self, rooted_slots: &[u64]) -> Result<()> {
        unsafe {
            let mut batch_processor = self.db.batch_processor();
            let mut write_batch = batch_processor.batch()?;
            for slot in rooted_slots {
                write_batch.put::<cf::Root>(*slot, &true)?;
            }

            batch_processor.write(write_batch)?;
        }
        Ok(())
    }

    pub fn is_dead(&self, slot: u64) -> bool {
        if let Some(true) = self
            .db
            .get::<cf::DeadSlots>(slot)
            .expect("fetch from DeadSlots column family failed")
        {
            true
        } else {
            false
        }
    }

    pub fn set_dead_slot(&self, slot: u64) -> Result<()> {
        self.dead_slots_cf.put(slot, &true)
    }

    pub fn get_orphans(&self, max: Option<usize>) -> Vec<u64> {
        let mut results = vec![];

        let mut iter = self.db.cursor::<cf::Orphans>().unwrap();
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

    // Handle special case of writing genesis blobs. For instance, the first two entries
    // don't count as ticks, even if they're empty entries
    fn write_genesis_blobs(&self, blobs: &[Blob]) -> Result<()> {
        // TODO: change bootstrap height to number of slots
        let mut bootstrap_meta = SlotMeta::new(0, 1);
        let last = blobs.last().unwrap();

        let mut batch_processor = self.batch_processor.write().unwrap();

        bootstrap_meta.consumed = last.index() + 1;
        bootstrap_meta.received = last.index() + 1;
        bootstrap_meta.is_connected = true;

        let mut batch = batch_processor.batch()?;
        batch.put::<cf::SlotMeta>(0, &bootstrap_meta)?;
        for blob in blobs {
            let serialized_blob_datas = &blob.data[..BLOB_HEADER_SIZE + blob.size()];
            batch.put_bytes::<cf::Data>((blob.slot(), blob.index()), serialized_blob_datas)?;
        }
        batch_processor.write(batch)?;
        Ok(())
    }

    /// Prune blocktree such that slots higher than `target_slot` are deleted and all references to
    /// higher slots are removed
    pub fn prune(&self, target_slot: u64) {
        let mut meta = self
            .meta(target_slot)
            .expect("couldn't read slot meta")
            .expect("no meta for target slot");
        meta.next_slots.clear();
        self.put_meta_bytes(
            target_slot,
            &bincode::serialize(&meta).expect("couldn't get meta bytes"),
        )
        .expect("unable to update meta for target slot");

        self.purge_slots(target_slot + 1, None);

        // fixup anything that refers to non-root slots and delete the rest
        for (slot, mut meta) in self
            .slot_meta_iterator(0)
            .expect("unable to iterate over meta")
        {
            if slot > target_slot {
                break;
            }
            meta.next_slots.retain(|slot| *slot <= target_slot);
            self.put_meta_bytes(
                slot,
                &bincode::serialize(&meta).expect("couldn't update meta"),
            )
            .expect("couldn't update meta");
        }
    }
}

fn insert_data_blob_batch<'a, I>(
    new_blobs: I,
    db: &Database,
    slot_meta_working_set: &mut HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    index_working_set: &mut HashMap<u64, Index>,
    prev_inserted_blob_datas: &mut HashMap<(u64, u64), &'a [u8]>,
    write_batch: &mut WriteBatch,
) -> Result<()>
where
    I: IntoIterator<Item = &'a Blob>,
{
    for blob in new_blobs.into_iter() {
        let inserted = check_insert_data_blob(
            blob,
            db,
            slot_meta_working_set,
            prev_inserted_blob_datas,
            write_batch,
        );

        if inserted {
            index_working_set
                .get_mut(&blob.slot())
                .expect("Index must be present for all data blobs")
                .data_mut()
                .set_present(blob.index(), true);
        }
    }

    Ok(())
}

/// Insert a blob into ledger, updating the slot_meta if necessary
fn insert_data_blob<'a>(
    blob_to_insert: &'a Blob,
    db: &Database,
    prev_inserted_blob_datas: &mut HashMap<(u64, u64), &'a [u8]>,
    slot_meta: &mut SlotMeta,
    write_batch: &mut WriteBatch,
) -> Result<()> {
    let blob_index = blob_to_insert.index();
    let blob_slot = blob_to_insert.slot();
    let blob_size = blob_to_insert.size();
    let data_cf = db.column::<cf::Data>();

    let check_data_cf = |slot, index| {
        data_cf
            .get_bytes((slot, index))
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    };

    let new_consumed = compute_consume_index(
        prev_inserted_blob_datas,
        slot_meta,
        blob_index,
        blob_slot,
        check_data_cf,
    );

    let serialized_blob_data = &blob_to_insert.data[..BLOB_HEADER_SIZE + blob_size];

    // Commit step: commit all changes to the mutable structures at once, or none at all.
    // We don't want only some of these changes going through.
    write_batch.put_bytes::<cf::Data>((blob_slot, blob_index), serialized_blob_data)?;
    prev_inserted_blob_datas.insert((blob_slot, blob_index), serialized_blob_data);
    update_slot_meta(
        blob_to_insert.is_last_in_slot(),
        slot_meta,
        blob_index,
        new_consumed,
    );
    Ok(())
}

fn update_slot_meta(
    is_last_in_slot: bool,
    slot_meta: &mut SlotMeta,
    index: u64,
    new_consumed: u64,
) {
    // Index is zero-indexed, while the "received" height starts from 1,
    // so received = index + 1 for the same blob.
    slot_meta.received = cmp::max(index + 1, slot_meta.received);
    slot_meta.consumed = new_consumed;
    slot_meta.last_index = {
        // If the last index in the slot hasn't been set before, then
        // set it to this blob index
        if slot_meta.last_index == std::u64::MAX {
            if is_last_in_slot {
                index
            } else {
                std::u64::MAX
            }
        } else {
            slot_meta.last_index
        }
    };
}

fn compute_consume_index<F>(
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    slot_meta: &mut SlotMeta,
    index: u64,
    slot: u64,
    db_check: F,
) -> u64
where
    F: Fn(u64, u64) -> bool,
{
    if slot_meta.consumed == index {
        let mut current_index = index + 1;

        while prev_inserted_blob_datas
            .get(&(slot, current_index))
            .is_some()
            || db_check(slot, current_index)
        {
            current_index += 1;
        }
        current_index
    } else {
        slot_meta.consumed
    }
}

/// Checks to see if the data blob passes integrity checks for insertion. Proceeds with
/// insertion if it does.
fn check_insert_data_blob<'a>(
    blob: &'a Blob,
    db: &Database,
    slot_meta_working_set: &mut HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    prev_inserted_blob_datas: &mut HashMap<(u64, u64), &'a [u8]>,
    write_batch: &mut WriteBatch,
) -> bool {
    let entry = get_slot_meta_entry(db, slot_meta_working_set, blob.slot(), blob.parent());

    let slot_meta = &mut entry.0.borrow_mut();
    if is_orphan(slot_meta) {
        slot_meta.parent_slot = blob.parent();
    }

    // This slot is full, skip the bogus blob
    // Check if this blob should be inserted
    if !should_insert_blob(&slot_meta, db, &prev_inserted_blob_datas, blob) {
        false
    } else {
        let _ = insert_data_blob(blob, db, prev_inserted_blob_datas, slot_meta, write_batch);
        true
    }
}

fn get_slot_meta_entry<'a>(
    db: &Database,
    slot_meta_working_set: &'a mut HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    slot: u64,
    parent_slot: u64,
) -> &'a mut (Rc<RefCell<SlotMeta>>, Option<SlotMeta>) {
    let meta_cf = db.column::<cf::SlotMeta>();

    // Check if we've already inserted the slot metadata for this blob's slot
    slot_meta_working_set.entry(slot).or_insert_with(|| {
        // Store a 2-tuple of the metadata (working copy, backup copy)
        if let Some(mut meta) = meta_cf.get(slot).expect("Expect database get to succeed") {
            let backup = Some(meta.clone());
            // If parent_slot == std::u64::MAX, then this is one of the orphans inserted
            // during the chaining process, see the function find_slot_meta_in_cached_state()
            // for details. Slots that are orphans are missing a parent_slot, so we should
            // fill in the parent now that we know it.
            if is_orphan(&meta) {
                meta.parent_slot = parent_slot;
            }

            (Rc::new(RefCell::new(meta)), backup)
        } else {
            (
                Rc::new(RefCell::new(SlotMeta::new(slot, parent_slot))),
                None,
            )
        }
    })
}

fn should_insert_blob(
    slot: &SlotMeta,
    db: &Database,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    blob: &Blob,
) -> bool {
    let blob_index = blob.index();
    let blob_slot = blob.slot();
    let last_in_slot = blob.is_last_in_slot();
    let data_cf = db.column::<cf::Data>();

    let check_data_cf = |slot, index| {
        data_cf
            .get_bytes((slot, index))
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    };

    should_insert(
        slot,
        prev_inserted_blob_datas,
        blob_index,
        blob_slot,
        last_in_slot,
        check_data_cf,
    )
}

fn should_insert<F>(
    slot_meta: &SlotMeta,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    index: u64,
    slot: u64,
    last_in_slot: bool,
    db_check: F,
) -> bool
where
    F: Fn(u64, u64) -> bool,
{
    // Check that the index doesn't already exist
    if index < slot_meta.consumed
        || prev_inserted_blob_datas.contains_key(&(slot, index))
        || db_check(slot, index)
    {
        return false;
    }
    // Check that we do not receive index >= than the last_index
    // for the slot
    let last_index = slot_meta.last_index;
    if index >= last_index {
        datapoint_error!(
            "blocktree_error",
            (
                "error",
                format!("Received index {} >= slot.last_index {}", index, last_index),
                String
            )
        );
        return false;
    }
    // Check that we do not receive a blob with "last_index" true, but index
    // less than our current received
    if last_in_slot && index < slot_meta.received {
        datapoint_error!(
            "blocktree_error",
            (
                "error",
                format!(
                    "Received index {} < slot.received {}",
                    index, slot_meta.received
                ),
                String
            )
        );
        return false;
    }
    true
}

fn send_signals(
    new_blobs_signals: &[SyncSender<bool>],
    completed_slots_senders: &[SyncSender<Vec<u64>>],
    should_signal: bool,
    newly_completed_slots: Vec<u64>,
) -> Result<()> {
    if should_signal {
        for signal in new_blobs_signals {
            let _ = signal.try_send(true);
        }
    }

    if !completed_slots_senders.is_empty() && !newly_completed_slots.is_empty() {
        let mut slots: Vec<_> = (0..completed_slots_senders.len() - 1)
            .map(|_| newly_completed_slots.clone())
            .collect();

        slots.push(newly_completed_slots);

        for (signal, slots) in completed_slots_senders.iter().zip(slots.into_iter()) {
            let res = signal.try_send(slots);
            if let Err(TrySendError::Full(_)) = res {
                solana_metrics::submit(
                    solana_metrics::influxdb::Point::new("blocktree_error")
                        .add_field(
                            "error",
                            solana_metrics::influxdb::Value::String(
                                "Unable to send newly completed slot because channel is full"
                                    .to_string(),
                            ),
                        )
                        .to_owned(),
                    log::Level::Error,
                );
            }
        }
    }

    Ok(())
}

fn prepare_signals(
    slot_meta_working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    completed_slots_senders: &[SyncSender<Vec<u64>>],
    write_batch: &mut WriteBatch,
) -> Result<(bool, Vec<u64>)> {
    let mut should_signal = false;
    let mut newly_completed_slots = vec![];

    // Check if any metadata was changed, if so, insert the new version of the
    // metadata into the write batch
    for (slot, (meta, meta_backup)) in slot_meta_working_set.iter() {
        let meta: &SlotMeta = &RefCell::borrow(&*meta);
        if !completed_slots_senders.is_empty() && is_newly_completed_slot(meta, meta_backup) {
            newly_completed_slots.push(*slot);
        }
        // Check if the working copy of the metadata has changed
        if Some(meta) != meta_backup.as_ref() {
            should_signal = should_signal || slot_has_updates(meta, &meta_backup);
            write_batch.put::<cf::SlotMeta>(*slot, &meta)?;
        }
    }

    Ok((should_signal, newly_completed_slots))
}

// 1) Find the slot metadata in the cache of dirty slot metadata we've previously touched,
// else:
// 2) Search the database for that slot metadata. If still no luck, then:
// 3) Create a dummy orphan slot in the database
fn find_slot_meta_else_create<'a>(
    db: &Database,
    working_set: &'a HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    chained_slots: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    slot_index: u64,
) -> Result<Rc<RefCell<SlotMeta>>> {
    let result = find_slot_meta_in_cached_state(working_set, chained_slots, slot_index)?;
    if let Some(slot) = result {
        Ok(slot)
    } else {
        find_slot_meta_in_db_else_create(db, slot_index, chained_slots)
    }
}

// Search the database for that slot metadata. If still no luck, then
// create a dummy orphan slot in the database
fn find_slot_meta_in_db_else_create<'a>(
    db: &Database,
    slot: u64,
    insert_map: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
) -> Result<Rc<RefCell<SlotMeta>>> {
    if let Some(slot_meta) = db.column::<cf::SlotMeta>().get(slot)? {
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

/// Returns the next consumed index and the number of ticks in the new consumed
/// range
fn get_slot_consecutive_blobs<'a>(
    slot: u64,
    db: &Database,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &'a [u8]>,
    mut current_index: u64,
    max_blobs: Option<u64>,
) -> Result<Vec<Cow<'a, [u8]>>> {
    let mut blobs: Vec<Cow<[u8]>> = vec![];
    let data_cf = db.column::<cf::Data>();

    loop {
        if Some(blobs.len() as u64) == max_blobs {
            break;
        }
        // Try to find the next blob we're looking for in the prev_inserted_blob_datas
        if let Some(prev_blob_data) = prev_inserted_blob_datas.get(&(slot, current_index)) {
            blobs.push(Cow::Borrowed(*prev_blob_data));
        } else if let Some(blob_data) = data_cf.get_bytes((slot, current_index))? {
            // Try to find the next blob we're looking for in the database
            blobs.push(Cow::Owned(blob_data));
        } else {
            break;
        }

        current_index += 1;
    }

    Ok(blobs)
}

fn get_slot_consecutive_shreds<'a>(
    slot: u64,
    db: &Database,
    mut current_index: u64,
) -> Result<Vec<Cow<'a, [u8]>>> {
    let mut serialized_shreds: Vec<Cow<[u8]>> = vec![];
    let data_cf = db.column::<cf::ShredData>();

    while let Some(serialized_shred) = data_cf.get_bytes((slot, current_index))? {
        serialized_shreds.push(Cow::Owned(serialized_shred));
        current_index += 1;
    }

    Ok(serialized_shreds)
}

// Chaining based on latest discussion here: https://github.com/solana-labs/solana/pull/2253
fn handle_chaining(
    db: &Database,
    write_batch: &mut WriteBatch,
    working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
) -> Result<()> {
    let mut new_chained_slots = HashMap::new();
    let working_set_slots: Vec<_> = working_set.iter().map(|s| *s.0).collect();
    for slot in working_set_slots {
        handle_chaining_for_slot(db, write_batch, working_set, &mut new_chained_slots, slot)?;
    }

    // Write all the newly changed slots in new_chained_slots to the write_batch
    for (slot, meta) in new_chained_slots.iter() {
        let meta: &SlotMeta = &RefCell::borrow(&*meta);
        write_batch.put::<cf::SlotMeta>(*slot, meta)?;
    }
    Ok(())
}

fn handle_chaining_for_slot(
    db: &Database,
    write_batch: &mut WriteBatch,
    working_set: &HashMap<u64, (Rc<RefCell<SlotMeta>>, Option<SlotMeta>)>,
    new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    slot: u64,
) -> Result<()> {
    let (meta, meta_backup) = working_set
        .get(&slot)
        .expect("Slot must exist in the working_set hashmap");

    {
        let mut meta_mut = meta.borrow_mut();
        let was_orphan_slot = meta_backup.is_some() && is_orphan(meta_backup.as_ref().unwrap());

        // If:
        // 1) This is a new slot
        // 2) slot != 0
        // then try to chain this slot to a previous slot
        if slot != 0 {
            let prev_slot = meta_mut.parent_slot;

            // Check if the slot represented by meta_mut is either a new slot or a orphan.
            // In both cases we need to run the chaining logic b/c the parent on the slot was
            // previously unknown.
            if meta_backup.is_none() || was_orphan_slot {
                let prev_slot_meta =
                    find_slot_meta_else_create(db, working_set, new_chained_slots, prev_slot)?;

                // This is a newly inserted slot/orphan so run the chaining logic to link it to a
                // newly discovered parent
                chain_new_slot_to_prev_slot(&mut prev_slot_meta.borrow_mut(), slot, &mut meta_mut);

                // If the parent of `slot` is a newly inserted orphan, insert it into the orphans
                // column family
                if is_orphan(&RefCell::borrow(&*prev_slot_meta)) {
                    write_batch.put::<cf::Orphans>(prev_slot, &true)?;
                }
            }
        }

        // At this point this slot has received a parent, so it's no longer an orphan
        if was_orphan_slot {
            write_batch.delete::<cf::Orphans>(slot)?;
        }
    }

    // If this is a newly inserted slot, then we know the children of this slot were not previously
    // connected to the trunk of the ledger. Thus if slot.is_connected is now true, we need to
    // update all child slots with `is_connected` = true because these children are also now newly
    // connected to to trunk of the the ledger
    let should_propagate_is_connected =
        is_newly_completed_slot(&RefCell::borrow(&*meta), meta_backup)
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

        traverse_children_mut(
            db,
            slot,
            &meta,
            working_set,
            new_chained_slots,
            slot_function,
        )?;
    }

    Ok(())
}

fn traverse_children_mut<F>(
    db: &Database,
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
                let next_slot = find_slot_meta_else_create(
                    db,
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
            || slot_meta.consumed != backup_slot_meta.as_ref().unwrap().consumed)
}

fn handle_recovery(
    db: &Database,
    erasure_metas: &HashMap<(u64, u64), ErasureMeta>,
    index_working_set: &mut HashMap<u64, Index>,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    prev_inserted_coding: &mut HashMap<(u64, u64), Blob>,
    writebatch: &mut WriteBatch,
    erasure_config: &ErasureConfig,
) -> Result<Option<Vec<Blob>>> {
    use solana_sdk::signature::Signable;

    let (mut recovered_data, mut recovered_coding) = (vec![], vec![]);

    for (&(slot, _), erasure_meta) in erasure_metas.iter() {
        let index = index_working_set.get_mut(&slot).expect("Index");

        if let Some((mut data, coding)) = try_erasure_recover(
            db,
            &erasure_meta,
            index,
            slot,
            &prev_inserted_blob_datas,
            &prev_inserted_coding,
            erasure_config,
        )? {
            for blob in data.iter() {
                debug!(
                    "[handle_recovery] recovered blob at ({}, {})",
                    blob.slot(),
                    blob.index()
                );

                let blob_index = blob.index();
                let blob_slot = blob.slot();

                assert_eq!(blob_slot, slot);

                assert!(
                    blob_index >= erasure_meta.start_index()
                        && blob_index < erasure_meta.end_indexes().0
                );
            }

            recovered_data.append(&mut data);

            for coding_blob in coding {
                if !index.coding().is_present(coding_blob.index()) {
                    recovered_coding.push(coding_blob);
                }
            }
        }
    }

    if !recovered_coding.is_empty() {
        debug!(
            "[handle_recovery] recovered {} coding blobs",
            recovered_coding.len()
        );

        for coding_blob in recovered_coding {
            let (blob_slot, blob_index) = (coding_blob.slot(), coding_blob.index());
            let index = index_working_set.get_mut(&blob_slot).expect("Index");

            index.coding_mut().set_present(coding_blob.index(), true);

            writebatch.put_bytes::<cf::Coding>(
                (blob_slot, blob_index),
                &coding_blob.data[..coding_blob.data_size() as usize],
            )?;

            prev_inserted_coding.insert((blob_slot, blob_index), coding_blob);
        }
    }

    if !recovered_data.is_empty() {
        let mut new_data = vec![];

        for blob in recovered_data {
            let index = index_working_set
                .get_mut(&blob.slot())
                .expect("Index must have been present if blob was recovered");

            let (blob_slot, blob_index) = (blob.slot(), blob.index());

            if !index.data().is_present(blob_index) {
                if blob.verify() {
                    trace!(
                        "[handle_recovery] successful verification at slot = {}, index={}",
                        blob_slot,
                        blob_index
                    );

                    new_data.push(blob);
                } else {
                    warn!(
                        "[handle_recovery] failed verification at slot={}, index={}, discarding",
                        blob.slot(),
                        blob.index()
                    );
                }
            }
        }

        debug!("[handle_recovery] recovered {} data blobs", new_data.len());

        Ok(Some(new_data))
    } else {
        Ok(None)
    }
}

/// Attempts recovery using erasure coding
fn try_erasure_recover(
    db: &Database,
    erasure_meta: &ErasureMeta,
    index: &Index,
    slot: u64,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    prev_inserted_coding: &HashMap<(u64, u64), Blob>,
    erasure_config: &ErasureConfig,
) -> Result<Option<(Vec<Blob>, Vec<Blob>)>> {
    let set_index = erasure_meta.set_index;
    let start_index = erasure_meta.start_index();
    let (data_end_index, coding_end_idx) = erasure_meta.end_indexes();

    let submit_metrics = |attempted: bool, status: String| {
        datapoint_info!(
            "blocktree-erasure",
            ("slot", slot as i64, i64),
            ("start_index", start_index as i64, i64),
            ("end_index", data_end_index as i64, i64),
            ("recovery_attempted", attempted, bool),
            ("recovery_status", status, String),
        );
    };

    let blobs = match erasure_meta.status(index) {
        ErasureMetaStatus::CanRecover => {
            let session = Session::new_from_config(erasure_config).unwrap();
            let erasure_result = recover(
                db,
                &session,
                slot,
                erasure_meta,
                index,
                prev_inserted_blob_datas,
                prev_inserted_coding,
                erasure_config,
            );

            match erasure_result {
                Ok((data, coding)) => {
                    let recovered = data.len() + coding.len();

                    assert_eq!(
                        erasure_config.num_data() + erasure_config.num_coding(),
                        recovered
                            + index.data().present_in_bounds(start_index..data_end_index)
                            + index
                                .coding()
                                .present_in_bounds(start_index..coding_end_idx),
                        "Recovery should always complete a set"
                    );

                    submit_metrics(true, "complete".into());

                    debug!(
                        "[try_erasure] slot: {}, set_index: {}, recovered {} blobs",
                        slot, set_index, recovered
                    );

                    Some((data, coding))
                }
                Err(Error::ErasureError(e)) => {
                    submit_metrics(true, format!("error: {}", e));

                    error!(
                        "[try_erasure] slot: {}, set_index: {}, recovery failed: cause: {}",
                        slot, erasure_meta.set_index, e
                    );

                    None
                }

                Err(e) => return Err(e),
            }
        }
        ErasureMetaStatus::StillNeed(needed) => {
            submit_metrics(false, format!("still need: {}", needed));

            debug!(
                "[try_erasure] slot: {}, set_index: {}, still need {} blobs",
                slot, set_index, needed
            );

            None
        }
        ErasureMetaStatus::DataFull => {
            submit_metrics(false, "complete".into());

            trace!(
                "[try_erasure] slot: {}, set_index: {}, set full",
                slot,
                set_index,
            );

            None
        }
    };

    Ok(blobs)
}

fn recover(
    db: &Database,
    session: &Session,
    slot: u64,
    erasure_meta: &ErasureMeta,
    index: &Index,
    prev_inserted_blob_datas: &HashMap<(u64, u64), &[u8]>,
    prev_inserted_coding: &HashMap<(u64, u64), Blob>,
    erasure_config: &ErasureConfig,
) -> Result<(Vec<Blob>, Vec<Blob>)> {
    let start_idx = erasure_meta.start_index();
    let size = erasure_meta.size();
    let data_cf = db.column::<cf::Data>();
    let erasure_cf = db.column::<cf::Coding>();

    debug!(
        "[recover] Attempting recovery: slot = {}, start_idx = {}, size = {}, erasure_meta = {:?}",
        slot, start_idx, size, erasure_meta
    );

    let (data_end_idx, coding_end_idx) = erasure_meta.end_indexes();

    let erasure_set_size = erasure_config.num_data() + erasure_config.num_coding();
    let present = &mut vec![true; erasure_set_size];
    let mut blobs = Vec::with_capacity(erasure_set_size);

    for i in start_idx..data_end_idx {
        if index.data().is_present(i) {
            trace!("[recover] present data blob at {}", i);

            let mut blob_bytes = match prev_inserted_blob_datas.get(&(slot, i)) {
                Some(bytes) => bytes.to_vec(),
                None => data_cf
                    .get_bytes((slot, i))?
                    .expect("erasure_meta must have no false positives"),
            };

            // If data is too short, extend it with zeroes
            blob_bytes.resize(size, 0u8);

            blobs.push(blob_bytes);
        } else {
            trace!("[recover] absent data blob at {}", i);

            let set_relative_idx = (i - start_idx) as usize;
            blobs.push(vec![0u8; size]);
            present[set_relative_idx] = false;
        }
    }

    for i in start_idx..coding_end_idx {
        if index.coding().is_present(i) {
            trace!("[recover] present coding blob at {}", i);
            let blob = match prev_inserted_coding.get(&(slot, i)) {
                Some(blob) => (*blob).clone(),
                _ => {
                    let bytes = erasure_cf
                        .get_bytes((slot, i))?
                        .expect("ErasureMeta must have no false positives");

                    Blob::new(&bytes)
                }
            };

            blobs.push(blob.data[BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + size].to_vec());
        } else {
            trace!("[recover] absent coding blob at {}", i);
            let set_relative_idx = (i - start_idx) as usize + erasure_config.num_data();
            blobs.push(vec![0; size]);
            present[set_relative_idx] = false;
        }
    }

    let (recovered_data, recovered_coding) =
        session.reconstruct_blobs(&mut blobs, present, size, start_idx, slot)?;

    debug!(
        "[recover] reconstruction OK slot: {}, indexes: [{},{})",
        slot, start_idx, data_end_idx
    );

    Ok((recovered_data, recovered_coding))
}

fn deserialize_blobs<I>(blob_datas: &[I]) -> bincode::Result<Vec<Entry>>
where
    I: Borrow<[u8]>,
{
    let blob_results = blob_datas.iter().map(|blob_data| {
        let serialized_entries_data = &blob_data.borrow()[BLOB_HEADER_SIZE..];
        Blocktree::deserialize_blob_data(serialized_entries_data)
    });

    let mut entries = vec![];

    // We want to early exit in this loop to prevent needless work if any blob is corrupted.
    // However, there's no way to early exit from a flat_map, so we're flattening manually
    // instead of calling map().flatten() to avoid allocating a vector for the map results above,
    // and then allocating another vector for flattening the results
    for r in blob_results {
        let blob_entries = r?;
        entries.extend(blob_entries);
    }

    Ok(entries)
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

// Creates a new ledger with slot 0 full of ticks (and only ticks).
//
// Returns the blockhash that can be used to append entries with.
pub fn create_new_ledger(ledger_path: &Path, genesis_block: &GenesisBlock) -> Result<Hash> {
    let ticks_per_slot = genesis_block.ticks_per_slot;
    Blocktree::destroy(ledger_path)?;
    genesis_block.write(&ledger_path)?;

    // Fill slot 0 with ticks that link back to the genesis_block to bootstrap the ledger.
    let blocktree = Blocktree::open(ledger_path)?;
    let entries = crate::entry::create_ticks(ticks_per_slot, genesis_block.hash());

    let mut shredder = Shredder::new(0, Some(0), 0.0, &Arc::new(Keypair::new()), 0)
        .expect("Failed to create entry shredder");
    let last_hash = entries.last().unwrap().hash;
    entries_to_shreds(vec![entries], ticks_per_slot, ticks_per_slot, &mut shredder);
    let shreds: Vec<Shred> = shredder
        .shreds
        .iter()
        .map(|s| bincode::deserialize(s).unwrap())
        .collect();

    blocktree.insert_shreds(&shreds)?;

    Ok(last_hash)
}

pub fn genesis<'a, I>(ledger_path: &Path, keypair: &Keypair, entries: I) -> Result<()>
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

pub fn get_tmp_ledger_path(name: &str) -> PathBuf {
    use std::env;
    let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
    let keypair = Keypair::new();

    let path = [
        out_dir,
        "ledger".to_string(),
        format!("{}-{}", name, keypair.pubkey()),
    ]
    .iter()
    .collect();

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
pub fn create_new_tmp_ledger(name: &str, genesis_block: &GenesisBlock) -> (PathBuf, Hash) {
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

pub fn tmp_copy_blocktree(from: &Path, name: &str) -> PathBuf {
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
    use crate::entry::{create_ticks, make_tiny_test_entries, Entry, EntrySlice};
    use crate::erasure::{CodingGenerator, ErasureConfig};
    use crate::packet;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use rand::Rng;
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
            let ledger = Blocktree::open(&ledger_path).unwrap();
            let mut ticks = vec![];
            //let mut shreds_per_slot = 0 as u64;
            let mut shreds_per_slot = vec![];

            for i in 0..num_slots {
                let mut new_ticks = create_ticks(ticks_per_slot, Hash::default());
                let num_shreds = ledger
                    .write_entries_using_shreds(
                        i,
                        0,
                        0,
                        ticks_per_slot,
                        Some(i.saturating_sub(1)),
                        true,
                        new_ticks.clone(),
                    )
                    .unwrap() as u64;
                shreds_per_slot.push(num_shreds);
                ticks.append(&mut new_ticks);
            }

            for i in 0..num_slots {
                let meta = ledger.meta(i).unwrap().unwrap();
                let num_shreds = shreds_per_slot[i as usize];
                assert_eq!(meta.consumed, num_shreds);
                assert_eq!(meta.received, num_shreds);
                assert_eq!(meta.last_index, num_shreds - 1);
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

            /*
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
            */
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
    fn test_insert_data_shreds_basic() {
        let num_entries = 5;
        assert!(num_entries > 1);

        let (mut shreds, entries) = make_slot_entries_using_shreds(0, 0, num_entries);
        let num_shreds = shreds.len() as u64;

        let ledger_path = get_tmp_ledger_path("test_insert_data_shreds_basic");
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert last blob, we're missing the other blobs, so no consecutive
        // blobs starting from slot 0, index 0 should exist.
        let last_shred = shreds.pop().unwrap();
        ledger.insert_shreds(&[last_shred]).unwrap();
        assert!(ledger.get_slot_entries(0, 0, None).unwrap().is_empty());

        let meta = ledger
            .meta(0)
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == num_shreds);

        // Insert the other blobs, check for consecutive returned entries
        ledger.insert_shreds(&shreds).unwrap();
        let result = ledger.get_slot_entries(0, 0, None).unwrap();

        assert_eq!(result, entries);

        let meta = ledger
            .meta(0)
            .unwrap()
            .expect("Expected new metadata object to exist");
        assert_eq!(meta.consumed, num_shreds);
        assert_eq!(meta.received, num_shreds);
        assert_eq!(meta.parent_slot, 0);
        assert_eq!(meta.last_index, num_shreds - 1);
        assert!(meta.next_slots.is_empty());
        assert!(meta.is_connected);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_shreds_reverse() {
        let num_entries = 10;
        let (mut shreds, entries) = make_slot_entries_using_shreds(0, 0, num_entries);
        let num_shreds = shreds.len() as u64;

        let ledger_path = get_tmp_ledger_path("test_insert_data_shreds_reverse");
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert blobs in reverse, check for consecutive returned blobs
        for i in (0..num_shreds).rev() {
            let shred = shreds.pop().unwrap();
            ledger.insert_shreds(&[shred]).unwrap();
            let result = ledger.get_slot_entries(0, 0, None).unwrap();

            let meta = ledger
                .meta(0)
                .unwrap()
                .expect("Expected metadata object to exist");
            assert_eq!(meta.last_index, num_shreds - 1);
            if i != 0 {
                assert_eq!(result.len(), 0);
                assert!(meta.consumed == 0 && meta.received == num_shreds as u64);
            } else {
                assert_eq!(meta.parent_slot, 0);
                assert_eq!(result, entries);
                assert!(meta.consumed == num_shreds as u64 && meta.received == num_shreds as u64);
            }
        }

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_slots() {
        test_insert_data_shreds_slots("test_insert_data_blobs_slots_single", false);
        test_insert_data_shreds_slots("test_insert_data_blobs_slots_bulk", true);
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
    #[ignore]
    pub fn test_get_slot_entries1() {
        let blocktree_path = get_tmp_ledger_path("test_get_slot_entries1");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let entries = make_tiny_test_entries(100);
            let mut shreds = entries_to_test_shreds(entries.clone(), 1, 0, false);
            for (i, b) in shreds.iter_mut().enumerate() {
                if i < 4 {
                    b.set_index(i as u32);
                } else {
                    b.set_index(8 + i as u32);
                }
            }
            blocktree
                .insert_shreds(&shreds)
                .expect("Expected successful write of blobs");

            assert_eq!(
                blocktree.get_slot_entries(1, 0, None).unwrap()[2..4],
                entries[2..4],
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    #[ignore]
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
    #[ignore]
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
    pub fn test_insert_data_shreds_consecutive() {
        let blocktree_path = get_tmp_ledger_path("test_insert_data_blobs_consecutive");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            for i in 0..4 {
                let slot = i;
                let parent_slot = if i == 0 { 0 } else { i - 1 };
                // Write entries
                let num_entries = 21 as u64 * (i + 1);
                let (mut shreds, original_entries) =
                    make_slot_entries_using_shreds(slot, parent_slot, num_entries);

                let num_shreds = shreds.len() as u64;
                let mut odd_shreds = vec![];
                for i in (0..num_shreds).rev() {
                    if i % 2 != 0 {
                        odd_shreds.insert(0, shreds.remove(i as usize));
                    }
                }
                blocktree.insert_shreds(&odd_shreds).unwrap();

                assert_eq!(blocktree.get_slot_entries(slot, 0, None).unwrap(), vec![]);

                let meta = blocktree.meta(slot).unwrap().unwrap();
                if num_shreds % 2 == 0 {
                    assert_eq!(meta.received, num_shreds);
                } else {
                    debug!("got here");
                    assert_eq!(meta.received, num_shreds - 1);
                }
                assert_eq!(meta.consumed, 0);
                if num_shreds % 2 == 0 {
                    assert_eq!(meta.last_index, num_shreds - 1);
                } else {
                    assert_eq!(meta.last_index, std::u64::MAX);
                }

                blocktree.insert_shreds(&shreds).unwrap();

                assert_eq!(
                    blocktree.get_slot_entries(slot, 0, None).unwrap(),
                    original_entries,
                );

                let meta = blocktree.meta(slot).unwrap().unwrap();
                assert_eq!(meta.received, num_shreds);
                assert_eq!(meta.consumed, num_shreds);
                assert_eq!(meta.parent_slot, parent_slot);
                assert_eq!(meta.last_index, num_shreds - 1);
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_shreds_duplicate() {
        // Create RocksDb ledger
        let blocktree_path = get_tmp_ledger_path("test_insert_data_blobs_duplicate");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Make duplicate entries and blobs
            let num_unique_entries = 10;
            let (mut original_shreds, original_entries) =
                make_slot_entries_using_shreds(0, 0, num_unique_entries);

            // Discard first shred
            original_shreds.remove(0);

            blocktree.insert_shreds(&original_shreds).unwrap();

            assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), vec![]);

            let duplicate_shreds = entries_to_test_shreds(original_entries.clone(), 0, 0, true);
            blocktree.insert_shreds(&duplicate_shreds).unwrap();

            assert_eq!(
                blocktree.get_slot_entries(0, 0, None).unwrap(),
                original_entries
            );

            let num_shreds = duplicate_shreds.len() as u64;
            let meta = blocktree.meta(0).unwrap().unwrap();
            assert_eq!(meta.consumed, num_shreds);
            assert_eq!(meta.received, num_shreds);
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_shreds - 1);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_new_blobs_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_new_blobs_signal");
        let (ledger, recvr, _) = Blocktree::open_with_signal(&ledger_path).unwrap();
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
    pub fn test_completed_blobs_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_completed_blobs_signal");
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;

        // Create blobs for slot 0
        let (blobs, _) = make_slot_entries(0, 0, entries_per_slot);

        // Insert all but the first blob in the slot, should not be considered complete
        ledger
            .insert_data_blobs(&blobs[1..entries_per_slot as usize])
            .unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first blob, slot should now be considered complete
        ledger.insert_data_blobs(once(&blobs[0])).unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![0]);
    }

    #[test]
    pub fn test_completed_blobs_signal_orphans() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_completed_blobs_signal_orphans");
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;
        let slots = vec![2, 5, 10];
        let all_blobs = make_chaining_slot_entries(&slots[..], entries_per_slot);

        // Get the blobs for slot 5 chaining to slot 2
        let (ref orphan_blobs, _) = all_blobs[1];

        // Get the blobs for slot 10, chaining to slot 5
        let (ref orphan_child, _) = all_blobs[2];

        // Insert all but the first blob in the slot, should not be considered complete
        ledger
            .insert_data_blobs(&orphan_child[1..entries_per_slot as usize])
            .unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first blob, slot should now be considered complete
        ledger.insert_data_blobs(once(&orphan_child[0])).unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![slots[2]]);

        // Insert the blobs for the orphan_slot
        ledger
            .insert_data_blobs(&orphan_blobs[1..entries_per_slot as usize])
            .unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first blob, slot should now be considered complete
        ledger.insert_data_blobs(once(&orphan_blobs[0])).unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![slots[1]]);
    }

    #[test]
    pub fn test_completed_blobs_signal_many() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path("test_completed_blobs_signal_many");
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;
        let mut slots = vec![2, 5, 10];
        let all_blobs = make_chaining_slot_entries(&slots[..], entries_per_slot);
        let disconnected_slot = 4;

        let (ref blobs0, _) = all_blobs[0];
        let (ref blobs1, _) = all_blobs[1];
        let (ref blobs2, _) = all_blobs[2];
        let (ref blobs3, _) = make_slot_entries(disconnected_slot, 1, entries_per_slot);

        let mut all_blobs: Vec<_> = vec![blobs0, blobs1, blobs2, blobs3]
            .into_iter()
            .flatten()
            .collect();

        all_blobs.shuffle(&mut thread_rng());
        ledger.insert_data_blobs(all_blobs).unwrap();
        let mut result = recvr.try_recv().unwrap();
        result.sort();
        slots.push(disconnected_slot);
        slots.sort();
        assert_eq!(result, slots);
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
        let blocktree_path = get_tmp_ledger_path("test_chaining_tree");
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_tree_levels = 6;
            assert!(num_tree_levels > 1);
            let branching_factor: u64 = 4;
            // Number of slots that will be in the tree
            let num_slots = (branching_factor.pow(num_tree_levels) - 1) / (branching_factor - 1);
            let erasure_config = ErasureConfig::default();
            let entries_per_slot = erasure_config.num_data() as u64;
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

                let shared_blobs: Vec<_> = slot_blobs
                    .iter()
                    .cloned()
                    .map(|blob| Arc::new(RwLock::new(blob)))
                    .collect();
                let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);
                let coding_blobs = coding_generator.next(&shared_blobs);
                assert_eq!(coding_blobs.len(), erasure_config.num_coding());

                let mut rng = thread_rng();

                // Randomly pick whether to insert erasure or coding blobs first
                if rng.gen_bool(0.5) {
                    blocktree.write_blobs(slot_blobs).unwrap();
                    blocktree.put_shared_coding_blobs(&coding_blobs).unwrap();
                } else {
                    blocktree.put_shared_coding_blobs(&coding_blobs).unwrap();
                    blocktree.write_blobs(slot_blobs).unwrap();
                }
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
            assert!(is_orphan(&meta));
            assert_eq!(blocktree.get_orphans(None), vec![1]);

            // Write slot 1 which chains to slot 0, so now slot 0 is the
            // orphan, and slot 1 is no longer the orphan.
            blocktree.write_blobs(once(&blobs[1])).unwrap();
            let meta = blocktree
                .meta(1)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(!is_orphan(&meta));
            let meta = blocktree
                .meta(0)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(is_orphan(&meta));
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
                assert!(!is_orphan(&meta));
            }
            // Orphans cf is empty
            assert!(blocktree.orphans_cf.is_empty().unwrap())
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    fn test_insert_data_shreds_slots(name: &str, should_bulk_write: bool) {
        let blocktree_path = get_tmp_ledger_path(name);
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create blobs and entries
            let num_entries = 20 as u64;
            let mut entries = vec![];
            let mut shreds = vec![];
            let mut num_shreds_per_slot = 0;
            for slot in 0..num_entries {
                let parent_slot = {
                    if slot == 0 {
                        0
                    } else {
                        slot - 1
                    }
                };

                let (mut shred, entry) = make_slot_entries_using_shreds(slot, parent_slot, 1);
                num_shreds_per_slot = shred.len() as u64;
                shred
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, shred)| shred.set_index(slot as u32 + i as u32));
                shreds.extend(shred);
                entries.extend(entry);
            }

            let num_shreds = shreds.len();
            // Write blobs to the database
            if should_bulk_write {
                blocktree.insert_shreds(&shreds).unwrap();
            } else {
                for _ in 0..num_shreds {
                    let shred = shreds.remove(0);
                    blocktree.insert_shreds(&vec![shred]).unwrap();
                }
            }

            for i in 0..num_entries - 1 {
                assert_eq!(
                    blocktree.get_slot_entries(i, i, None).unwrap()[0],
                    entries[i as usize]
                );

                let meta = blocktree.meta(i).unwrap().unwrap();
                assert_eq!(meta.received, i + num_shreds_per_slot);
                assert_eq!(meta.last_index, i + num_shreds_per_slot - 1);
                if i != 0 {
                    assert_eq!(meta.parent_slot, i - 1);
                    assert_eq!(meta.consumed, 0);
                } else {
                    assert_eq!(meta.parent_slot, 0);
                    assert_eq!(meta.consumed, num_shreds_per_slot);
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

    #[test]
    pub fn test_should_insert_blob() {
        let (mut blobs, _) = make_slot_entries(0, 0, 20);
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Insert the first 5 blobs, we don't have a "is_last" blob yet
        blocktree.insert_data_blobs(&blobs[0..5]).unwrap();

        // Trying to insert a blob less than consumed should fail
        let slot_meta = blocktree.meta(0).unwrap().unwrap();
        assert_eq!(slot_meta.consumed, 5);
        assert!(!should_insert_blob(
            &slot_meta,
            &blocktree.db,
            &HashMap::new(),
            &blobs[4].clone()
        ));

        // Trying to insert the same blob again should fail
        blocktree.insert_data_blobs(&blobs[7..8]).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();
        assert!(!should_insert_blob(
            &slot_meta,
            &blocktree.db,
            &HashMap::new(),
            &blobs[7].clone()
        ));

        // Trying to insert another "is_last" blob with index < the received index
        // should fail
        blocktree.insert_data_blobs(&blobs[8..9]).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();
        assert_eq!(slot_meta.received, 9);
        blobs[8].set_is_last_in_slot();
        assert!(!should_insert_blob(
            &slot_meta,
            &blocktree.db,
            &HashMap::new(),
            &blobs[8].clone()
        ));

        // Insert the 10th blob, which is marked as "is_last"
        blobs[9].set_is_last_in_slot();
        blocktree.insert_data_blobs(&blobs[9..10]).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();

        // Trying to insert a blob with index > the "is_last" blob should fail
        assert!(!should_insert_blob(
            &slot_meta,
            &blocktree.db,
            &HashMap::new(),
            &blobs[10].clone()
        ));

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_multiple_is_last() {
        let (mut blobs, _) = make_slot_entries(0, 0, 20);
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Inserting multiple blobs with the is_last flag set should only insert
        // the first blob with the "is_last" flag, and drop the rest
        for i in 6..20 {
            blobs[i].set_is_last_in_slot();
        }

        blocktree.insert_data_blobs(&blobs[..]).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();

        assert_eq!(slot_meta.consumed, 7);
        assert_eq!(slot_meta.received, 7);
        assert_eq!(slot_meta.last_index, 6);
        assert!(slot_meta.is_full());

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_slot_data_iterator() {
        // Construct the blobs
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let blobs_per_slot = 10;
        let slots = vec![2, 4, 8, 12];
        let all_blobs = make_chaining_slot_entries(&slots, blobs_per_slot);
        let slot_8_blobs = all_blobs[2].0.clone();
        for (slot_blobs, _) in all_blobs {
            blocktree.insert_data_blobs(&slot_blobs[..]).unwrap();
        }

        // Slot doesnt exist, iterator should be empty
        let blob_iter = blocktree.slot_data_iterator(5).unwrap();
        let result: Vec<_> = blob_iter.collect();
        assert_eq!(result, vec![]);

        // Test that the iterator for slot 8 contains what was inserted earlier
        let blob_iter = blocktree.slot_data_iterator(8).unwrap();
        let result: Vec<_> = blob_iter.map(|(_, bytes)| Blob::new(&bytes)).collect();
        assert_eq!(result.len() as u64, blobs_per_slot);
        assert_eq!(result, slot_8_blobs);

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_set_roots() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let chained_slots = vec![0, 2, 4, 7, 12, 15];

        blocktree.set_roots(&chained_slots).unwrap();

        for i in chained_slots {
            assert!(blocktree.is_root(i));
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_prune() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let (blobs, _) = make_many_slot_entries(0, 50, 6);
        blocktree.write_blobs(blobs).unwrap();
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(_, meta)| assert_eq!(meta.last_index, 5));

        blocktree.prune(5);

        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, meta)| {
                assert!(slot <= 5);
                assert_eq!(meta.last_index, 5)
            });

        let data_iter = blocktree.data_cf.iter(Some((0, 0))).unwrap();
        for ((slot, _), _) in data_iter {
            if slot > 5 {
                assert!(false);
            }
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_purge_slots() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let (blobs, _) = make_many_slot_entries(0, 50, 5);
        blocktree.write_blobs(blobs).unwrap();

        blocktree.purge_slots(0, Some(5));

        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| {
                assert!(slot > 5);
            });

        blocktree.purge_slots(0, None);

        blocktree.slot_meta_iterator(0).unwrap().for_each(|(_, _)| {
            assert!(false);
        });

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_purge_huge() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let (blobs, _) = make_many_slot_entries(0, 5000, 10);
        blocktree.write_blobs(blobs).unwrap();

        blocktree.purge_slots(0, Some(4999));

        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, _)| {
                assert_eq!(slot, 5000);
            });

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[should_panic]
    #[test]
    fn test_prune_out_of_bounds() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // slot 5 does not exist, prune should panic
        blocktree.prune(5);

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_iter_bounds() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // slot 5 does not exist, iter should be ok and should be a noop
        blocktree
            .slot_meta_iterator(5)
            .unwrap()
            .for_each(|_| assert!(false));

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    mod erasure {
        use super::*;
        use crate::blocktree::meta::ErasureMetaStatus;
        use crate::erasure::test::{generate_ledger_model, ErasureSpec, SlotSpec};
        use crate::erasure::CodingGenerator;
        use rand::{thread_rng, Rng};
        use solana_sdk::signature::Signable;
        use std::sync::RwLock;

        impl Into<SharedBlob> for Blob {
            fn into(self) -> SharedBlob {
                Arc::new(RwLock::new(self))
            }
        }

        #[test]
        fn test_erasure_meta_accuracy() {
            use ErasureMetaStatus::DataFull;

            solana_logger::setup();

            let path = get_tmp_ledger_path!();
            let blocktree = Blocktree::open(&path).unwrap();

            let erasure_config = ErasureConfig::default();
            // two erasure sets
            let num_blobs = erasure_config.num_data() as u64 * 2;
            let slot = 0;

            let (mut blobs, _) = make_slot_entries(slot, 0, num_blobs);
            let keypair = Keypair::new();
            blobs.iter_mut().for_each(|blob| {
                blob.set_id(&keypair.pubkey());
                blob.sign(&keypair);
            });
            let shared_blobs: Vec<_> = blobs
                .iter()
                .cloned()
                .map(|blob| Arc::new(RwLock::new(blob)))
                .collect();

            blocktree.write_blobs(&blobs[..2]).unwrap();

            let erasure_meta_opt = blocktree
                .erasure_meta(slot, 0)
                .expect("DB get must succeed");

            assert!(erasure_meta_opt.is_none());

            blocktree
                .write_blobs(&blobs[2..erasure_config.num_data()])
                .unwrap();

            // insert all coding blobs in first set
            let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);
            let coding_blobs = coding_generator.next(&shared_blobs[..erasure_config.num_data()]);

            blocktree
                .put_shared_coding_blobs(coding_blobs.iter())
                .unwrap();

            let erasure_meta = blocktree
                .erasure_meta(slot, 0)
                .expect("DB get must succeed")
                .unwrap();
            let index = blocktree.get_index(slot).unwrap().unwrap();

            assert_eq!(erasure_meta.status(&index), DataFull);

            // insert blob in the 2nd set so that recovery should be possible given all coding blobs
            let set2 = &blobs[erasure_config.num_data()..];
            blocktree.write_blobs(&set2[..1]).unwrap();

            // insert all coding blobs in 2nd set. Should trigger recovery
            let coding_blobs = coding_generator.next(&shared_blobs[erasure_config.num_data()..]);

            blocktree
                .put_shared_coding_blobs(coding_blobs.iter())
                .unwrap();

            let erasure_meta = blocktree
                .erasure_meta(slot, 1)
                .expect("DB get must succeed")
                .unwrap();
            let index = blocktree.get_index(slot).unwrap().unwrap();

            assert_eq!(erasure_meta.status(&index), DataFull);

            // remove coding blobs, erasure meta should still report being full
            let (start_idx, coding_end_idx) =
                (erasure_meta.start_index(), erasure_meta.end_indexes().1);

            for idx in start_idx..coding_end_idx {
                blocktree.delete_coding_blob(slot, idx).unwrap();
            }

            let erasure_meta = blocktree
                .erasure_meta(slot, 1)
                .expect("DB get must succeed")
                .unwrap();
            let index = blocktree.get_index(slot).unwrap().unwrap();

            assert_eq!(erasure_meta.status(&index), DataFull);
        }

        #[test]
        pub fn test_recovery_basic() {
            solana_logger::setup();

            let slot = 0;

            let ledger_path = get_tmp_ledger_path!();
            let erasure_config = ErasureConfig::default();

            let blocktree = Blocktree::open(&ledger_path).unwrap();
            let num_sets = 3;
            let data_blobs =
                make_slot_entries(slot, 0, num_sets * erasure_config.num_data() as u64)
                    .0
                    .into_iter()
                    .map(Blob::into)
                    .collect::<Vec<_>>();
            let keypair = Keypair::new();
            data_blobs.iter().for_each(|blob: &Arc<RwLock<Blob>>| {
                let mut b = blob.write().unwrap();
                b.set_id(&keypair.pubkey());
                b.sign(&keypair);
            });

            let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);

            for (set_index, data_blobs) in data_blobs
                .chunks_exact(erasure_config.num_data())
                .enumerate()
            {
                let coding_blobs = coding_generator.next(&data_blobs);

                assert_eq!(coding_blobs.len(), erasure_config.num_coding());

                let deleted_data = data_blobs[0].clone();

                blocktree
                    .write_shared_blobs(data_blobs.iter().skip(1))
                    .unwrap();

                // This should trigger recovery of the missing data blob
                blocktree
                    .put_shared_coding_blobs(coding_blobs.iter())
                    .unwrap();

                // Verify the slot meta
                let slot_meta = blocktree.meta(slot).unwrap().unwrap();
                assert_eq!(
                    slot_meta.consumed,
                    (erasure_config.num_data() * (set_index + 1)) as u64
                );
                assert_eq!(
                    slot_meta.received,
                    (erasure_config.num_data() * (set_index + 1)) as u64
                );
                assert_eq!(slot_meta.parent_slot, 0);
                assert!(slot_meta.next_slots.is_empty());
                assert_eq!(slot_meta.is_connected, true);
                if set_index as u64 == num_sets - 1 {
                    assert_eq!(
                        slot_meta.last_index,
                        (erasure_config.num_data() * (set_index + 1) - 1) as u64
                    );
                }

                let erasure_meta = blocktree
                    .erasure_meta_cf
                    .get((slot, set_index as u64))
                    .expect("Erasure Meta should be present")
                    .unwrap();
                let index = blocktree.get_index(slot).unwrap().unwrap();
                let status = erasure_meta.status(&index);

                assert_eq!(status, ErasureMetaStatus::DataFull);

                let retrieved_data = blocktree
                    .data_cf
                    .get_bytes((slot, erasure_meta.start_index()))
                    .unwrap();

                assert!(retrieved_data.is_some());

                let data_blob = Blob::new(&retrieved_data.unwrap());

                assert_eq!(&data_blob, &*deleted_data.read().unwrap());
                //assert_eq!(
                //&retrieved_data.unwrap()[..],
                //deleted_data.read().unwrap().data()
                //);
            }

            drop(blocktree);

            Blocktree::destroy(&ledger_path).expect("Expect successful Blocktree destruction");
        }

        #[test]
        fn test_recovery_is_accurate() {
            const SLOT: u64 = 0;
            const SET_INDEX: u64 = 0;

            solana_logger::setup();

            let ledger_path = get_tmp_ledger_path!();
            let erasure_config = ErasureConfig::default();
            let blocktree = Blocktree::open(&ledger_path).unwrap();
            let data_blobs = make_slot_entries(SLOT, 0, erasure_config.num_data() as u64)
                .0
                .into_iter()
                .map(Blob::into)
                .collect::<Vec<_>>();

            let session = Arc::new(Session::new_from_config(&erasure_config).unwrap());

            let mut coding_generator = CodingGenerator::new(Arc::clone(&session));

            let shared_coding_blobs = coding_generator.next(&data_blobs);
            assert_eq!(shared_coding_blobs.len(), erasure_config.num_coding());

            let mut prev_coding = HashMap::new();
            let prev_data = HashMap::new();
            let mut index = Index::new(SLOT);
            let mut erasure_meta = ErasureMeta::new(SET_INDEX, &erasure_config);
            erasure_meta.size = shared_coding_blobs[0].read().unwrap().size();

            for shared_blob in shared_coding_blobs.iter() {
                let blob = shared_blob.read().unwrap();

                prev_coding.insert((blob.slot(), blob.index()), blob.clone());
            }

            index.coding_mut().set_many_present(
                (0..erasure_config.num_coding() as u64).zip(std::iter::repeat(true)),
            );

            let (recovered_data, recovered_coding) = recover(
                &blocktree.db,
                &session,
                SLOT,
                &erasure_meta,
                &index,
                &prev_data,
                &prev_coding,
                &erasure_config,
            )
            .expect("Successful recovery");

            for (original, recovered) in data_blobs.iter().zip(recovered_data.iter()) {
                let original = original.read().unwrap();

                assert_eq!(original.slot(), recovered.slot());
                assert_eq!(original.index(), recovered.index());

                assert_eq!(original.data(), recovered.data());
                assert_eq!(&*original, recovered);
            }

            for (original, recovered) in shared_coding_blobs.iter().zip(recovered_coding.iter()) {
                let original = original.read().unwrap();

                assert_eq!(original.slot(), recovered.slot());
                assert_eq!(original.index(), recovered.index());

                assert_eq!(original.data(), recovered.data());
                assert_eq!(&*original, recovered);
            }
        }

        pub fn try_recovery_using_erasure_config(
            erasure_config: &ErasureConfig,
            num_drop_data: usize,
            slot: u64,
            blocktree: &Blocktree,
        ) -> ErasureMetaStatus {
            let entries = make_tiny_test_entries(erasure_config.num_data());
            let mut blobs = entries_to_blobs_using_config(&entries, slot, 0, true, &erasure_config);

            let keypair = Keypair::new();
            blobs.iter_mut().for_each(|blob| {
                blob.set_id(&keypair.pubkey());
                blob.sign(&keypair);
            });

            let shared_blobs: Vec<_> = blobs
                .iter()
                .cloned()
                .map(|blob| Arc::new(RwLock::new(blob)))
                .collect();

            blocktree
                .write_blobs(&blobs[..(erasure_config.num_data() - num_drop_data)])
                .unwrap();

            let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);
            let coding_blobs = coding_generator.next(&shared_blobs[..erasure_config.num_data()]);

            blocktree
                .put_shared_coding_blobs(coding_blobs.iter())
                .unwrap();

            let erasure_meta = blocktree
                .erasure_meta(slot, 0)
                .expect("DB get must succeed")
                .unwrap();
            let index = blocktree.get_index(slot).unwrap().unwrap();

            erasure_meta.status(&index)
        }

        #[test]
        fn test_recovery_different_configs() {
            use ErasureMetaStatus::DataFull;
            solana_logger::setup();

            let ledger_path = get_tmp_ledger_path!();
            let blocktree = Blocktree::open(&ledger_path).unwrap();

            assert_eq!(
                try_recovery_using_erasure_config(&ErasureConfig::default(), 4, 0, &blocktree),
                DataFull
            );

            assert_eq!(
                try_recovery_using_erasure_config(&ErasureConfig::new(12, 8), 8, 1, &blocktree),
                DataFull
            );

            assert_eq!(
                try_recovery_using_erasure_config(&ErasureConfig::new(16, 4), 4, 2, &blocktree),
                DataFull
            );
        }

        #[test]
        fn test_recovery_fails_safely() {
            const SLOT: u64 = 0;
            const SET_INDEX: u64 = 0;

            solana_logger::setup();
            let ledger_path = get_tmp_ledger_path!();
            let erasure_config = ErasureConfig::default();
            let blocktree = Blocktree::open(&ledger_path).unwrap();
            let data_blobs = make_slot_entries(SLOT, 0, erasure_config.num_data() as u64)
                .0
                .into_iter()
                .map(Blob::into)
                .collect::<Vec<_>>();

            let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);

            let shared_coding_blobs = coding_generator.next(&data_blobs);
            assert_eq!(shared_coding_blobs.len(), erasure_config.num_coding());

            // Insert coding blobs except 1 and no data. Not enough to do recovery
            blocktree
                .put_shared_coding_blobs(shared_coding_blobs.iter().skip(1))
                .unwrap();

            // try recovery even though there aren't enough blobs
            let erasure_meta = blocktree
                .erasure_meta_cf
                .get((SLOT, SET_INDEX))
                .unwrap()
                .unwrap();

            let index = blocktree.index_cf.get(SLOT).unwrap().unwrap();

            assert_eq!(erasure_meta.status(&index), ErasureMetaStatus::StillNeed(1));

            let prev_inserted_blob_datas = HashMap::new();
            let prev_inserted_coding = HashMap::new();

            let attempt_result = try_erasure_recover(
                &blocktree.db,
                &erasure_meta,
                &index,
                SLOT,
                &prev_inserted_blob_datas,
                &prev_inserted_coding,
                &erasure_config,
            );

            assert!(attempt_result.is_ok());
            let recovered_blobs_opt = attempt_result.unwrap();

            assert!(recovered_blobs_opt.is_none());
        }

        #[test]
        #[ignore]
        fn test_deserialize_corrupted_blob() {
            let path = get_tmp_ledger_path!();
            let blocktree = Blocktree::open(&path).unwrap();
            let (mut blobs, _) = make_slot_entries(0, 0, 1);
            {
                let blob0 = &mut blobs[0];
                // corrupt the size
                blob0.set_size(BLOB_HEADER_SIZE);
            }
            blocktree.insert_data_blobs(&blobs).unwrap();
            assert_matches!(
                blocktree.get_slot_entries(0, 0, None),
                Err(Error::BlocktreeError(BlocktreeError::InvalidBlobData(_)))
            );
        }

        #[test]
        fn test_recovery_multi_slot_multi_thread() {
            use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
            use std::thread;

            const N_THREADS: usize = 3;

            let slots = vec![0, 3, 5, 50, 100];
            let max_erasure_sets = 16;
            solana_logger::setup();

            let erasure_config = ErasureConfig::default();
            let path = get_tmp_ledger_path!();
            let blocktree = Arc::new(Blocktree::open(&path).unwrap());
            let mut rng = thread_rng();

            // Specification should generate a ledger where each slot has an random number of
            // erasure sets. Odd erasure sets will have all coding blobs and between 1-4 data blobs
            // missing, and even ones will have between 1-2 data blobs missing and 1-2 coding blobs
            // missing
            let specs = slots
                .iter()
                .map(|&slot| {
                    let num_erasure_sets = rng.gen_range(0, max_erasure_sets);

                    let set_specs = (0..num_erasure_sets)
                        .map(|set_index| {
                            let (num_data, num_coding) = if set_index % 2 == 0 {
                                (
                                    erasure_config.num_data() - rng.gen_range(1, 3),
                                    erasure_config.num_coding() - rng.gen_range(1, 3),
                                )
                            } else {
                                (
                                    erasure_config.num_data() - rng.gen_range(1, 5),
                                    erasure_config.num_coding(),
                                )
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

            let model = generate_ledger_model(specs);

            // Write to each slot in a different thread simultaneously.
            // These writes should trigger the recovery. Every erasure set should have all of its
            // data blobs and coding_blobs at the end
            let mut handles = vec![];

            // Each thread will attempt to write to each slot in order. Within a slot, each thread
            // will try to write each erasure set in a random order. Within each erasure set, there
            // is a 50/50 chance of attempting to write the coding blobs first or the data blobs
            // first.
            // The goal is to be as contentious as possible and cover a wide range of situations
            for thread_id in 0..N_THREADS {
                let blocktree = Arc::clone(&blocktree);

                let model = model.clone();
                let handle = thread::Builder::new().stack_size(32* 1024 * 1024).spawn(move || {
                    let mut rng = SmallRng::from_rng(&mut thread_rng()).unwrap();

                    for slot_model in model {
                        let slot = slot_model.slot;
                        let num_erasure_sets = slot_model.chunks.len();
                        let unordered_sets = slot_model
                            .chunks
                            .choose_multiple(&mut rng, num_erasure_sets);

                        for erasure_set in unordered_sets {
                            let mut attempt = 0;
                            loop {
                                if rng.gen() {
                                    blocktree
                                        .write_shared_blobs(&erasure_set.data)
                                        .expect("Writing data blobs must succeed");
                                    trace!(
                                        "multislot: wrote data: slot: {}, erasure_set: {}",
                                        slot,
                                        erasure_set.set_index
                                    );

                                    blocktree
                                        .put_shared_coding_blobs(erasure_set.coding.iter())
                                        .unwrap();

                                    trace!(
                                        "multislot: wrote coding: slot: {}, erasure_set: {}",
                                        slot,
                                        erasure_set.set_index
                                    );
                                } else {
                                    // write coding blobs first, then write the data blobs.
                                    blocktree
                                        .put_shared_coding_blobs(erasure_set.coding.iter())
                                        .unwrap();

                                    trace!(
                                        "multislot: wrote coding: slot: {}, erasure_set: {}",
                                        slot,
                                        erasure_set.set_index
                                    );

                                    blocktree
                                        .write_shared_blobs(&erasure_set.data)
                                        .expect("Writing data blobs must succeed");
                                    trace!(
                                        "multislot: wrote data: slot: {}, erasure_set: {}",
                                        slot,
                                        erasure_set.set_index
                                    );
                                }

                                // due to racing, some blobs might not be inserted. don't stop
                                // trying until *some* thread succeeds in writing everything and
                                // triggering recovery.
                                let erasure_meta = blocktree
                                    .erasure_meta_cf
                                    .get((slot, erasure_set.set_index))
                                    .unwrap()
                                    .unwrap();

                                let index = blocktree.index_cf.get(slot).unwrap().unwrap();

                                let status = erasure_meta.status(&index);
                                attempt += 1;

                                debug!(
                                    "[multi_slot] thread_id: {}, attempt: {}, slot: {}, set_index: {}, status: {:?}",
                                    thread_id, attempt, slot, erasure_set.set_index, status
                                );

                                match status {
                                    ErasureMetaStatus::DataFull => break,
                                    ErasureMetaStatus::CanRecover => {
                                        debug!("[test_multi_slot] can recover");

                                        if !rng.gen::<bool>() {
                                            continue;
                                        } else {
                                            break;
                                        }
                                    }
                                    ErasureMetaStatus::StillNeed(_) => {
                                        if attempt > N_THREADS + thread_id {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }).unwrap();

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

                    let index = blocktree
                        .index_cf
                        .get(slot)
                        .expect("DB read")
                        .expect("Erasure meta for each set");

                    debug!(
                        "multislot: got erasure_meta: slot: {}, set_index: {}, erasure_meta: {:?}",
                        slot, set_index, erasure_meta
                    );

                    let start_index = erasure_meta.start_index();
                    let (data_end_idx, _) = erasure_meta.end_indexes();

                    // all possibility for recovery should be exhausted
                    assert_eq!(erasure_meta.status(&index), ErasureMetaStatus::DataFull);
                    // Should have all data
                    assert_eq!(
                        index.data().present_in_bounds(start_index..data_end_idx),
                        erasure_config.num_data()
                    );
                }
            }

            drop(blocktree);
            Blocktree::destroy(&path).expect("Blocktree destruction must succeed");
        }
    }

    pub fn entries_to_blobs_using_config(
        entries: &Vec<Entry>,
        slot: u64,
        parent_slot: u64,
        is_full_slot: bool,
        config: &ErasureConfig,
    ) -> Vec<Blob> {
        let mut blobs = entries.clone().to_single_entry_blobs();
        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64);
            b.set_slot(slot);
            b.set_parent(parent_slot);
            b.set_erasure_config(config);
        }
        if is_full_slot {
            blobs.last_mut().unwrap().set_is_last_in_slot();
        }
        blobs
    }

    pub fn entries_to_blobs(
        entries: &Vec<Entry>,
        slot: u64,
        parent_slot: u64,
        is_full_slot: bool,
    ) -> Vec<Blob> {
        entries_to_blobs_using_config(
            entries,
            slot,
            parent_slot,
            is_full_slot,
            &ErasureConfig::default(),
        )
    }

    pub fn entries_to_test_shreds(
        entries: Vec<Entry>,
        slot: u64,
        parent_slot: u64,
        is_full_slot: bool,
    ) -> Vec<Shred> {
        let mut shredder = Shredder::new(
            slot,
            Some(parent_slot),
            0.0,
            &Arc::new(Keypair::new()),
            0 as u32,
        )
        .expect("Failed to create entry shredder");

        let last_tick = 0;
        let bank_max_tick = if is_full_slot {
            last_tick
        } else {
            last_tick + 1
        };

        entries_to_shreds(vec![entries], last_tick, bank_max_tick, &mut shredder);

        let shreds: Vec<Shred> = shredder
            .shreds
            .iter()
            .map(|s| bincode::deserialize(s).unwrap())
            .collect();

        shreds
    }

    pub fn make_slot_entries_using_shreds(
        slot: u64,
        parent_slot: u64,
        num_entries: u64,
    ) -> (Vec<Shred>, Vec<Entry>) {
        let entries = make_tiny_test_entries(num_entries as usize);
        let shreds = entries_to_test_shreds(entries.clone(), slot, parent_slot, true);
        (shreds, entries)
    }

    pub fn make_many_slot_entries_using_shreds(
        start_slot: u64,
        num_slots: u64,
        entries_per_slot: u64,
    ) -> (Vec<Shred>, Vec<Entry>) {
        let mut shreds = vec![];
        let mut entries = vec![];
        for slot in start_slot..start_slot + num_slots {
            let parent_slot = if slot == 0 { 0 } else { slot - 1 };

            let (slot_blobs, slot_entries) =
                make_slot_entries_using_shreds(slot, parent_slot, entries_per_slot);
            shreds.extend(slot_blobs);
            entries.extend(slot_entries);
        }

        (shreds, entries)
    }

    // Create blobs for slots that have a parent-child relationship defined by the input `chain`
    pub fn make_chaining_slot_entries_using_shreds(
        chain: &[u64],
        entries_per_slot: u64,
    ) -> Vec<(Vec<Shred>, Vec<Entry>)> {
        let mut slots_shreds_and_entries = vec![];
        for (i, slot) in chain.iter().enumerate() {
            let parent_slot = {
                if *slot == 0 {
                    0
                } else if i == 0 {
                    std::u64::MAX
                } else {
                    chain[i - 1]
                }
            };

            let result = make_slot_entries_using_shreds(*slot, parent_slot, entries_per_slot);
            slots_shreds_and_entries.push(result);
        }

        slots_shreds_and_entries
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

    // Create blobs for slots that have a parent-child relationship defined by the input `chain`
    pub fn make_chaining_slot_entries(
        chain: &[u64],
        entries_per_slot: u64,
    ) -> Vec<(Vec<Blob>, Vec<Entry>)> {
        let mut slots_blobs_and_entries = vec![];
        for (i, slot) in chain.iter().enumerate() {
            let parent_slot = {
                if *slot == 0 {
                    0
                } else if i == 0 {
                    std::u64::MAX
                } else {
                    chain[i - 1]
                }
            };

            let result = make_slot_entries(*slot, parent_slot, entries_per_slot);
            slots_blobs_and_entries.push(result);
        }

        slots_blobs_and_entries
    }
}
