//! The `blocktree` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.
pub use crate::{blocktree_db::BlocktreeError, blocktree_meta::SlotMeta};
use crate::{
    blocktree_db::{
        columns as cf, Column, Database, IteratorDirection, IteratorMode, LedgerColumn, Result,
        WriteBatch,
    },
    blocktree_meta::*,
    entry::{create_ticks, Entry},
    erasure::ErasureConfig,
    leader_schedule_cache::LeaderScheduleCache,
    shred::{Shred, Shredder},
};
use bincode::deserialize;
use chrono::{offset::TimeZone, Duration as ChronoDuration, Utc};
use log::*;
use rayon::{
    iter::{IntoParallelRefIterator, ParallelIterator},
    ThreadPool,
};
use rocksdb::DBRawIterator;
use solana_client::rpc_request::{RpcConfirmedBlock, RpcTransactionStatus};
use solana_measure::measure::Measure;
use solana_metrics::{datapoint_debug, datapoint_error};
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::{
    clock::{Slot, UnixTimestamp, DEFAULT_TICKS_PER_SECOND, MS_PER_TICK},
    genesis_config::GenesisConfig,
    hash::Hash,
    signature::{Keypair, KeypairUtil, Signature},
    timing::{duration_as_ms, timestamp},
    transaction::Transaction,
};
use std::{
    cell::RefCell,
    cmp,
    collections::HashMap,
    convert::TryFrom,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

pub const BLOCKTREE_DIRECTORY: &str = "rocksdb";

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .build()
                    .unwrap()));

pub const MAX_COMPLETED_SLOTS_IN_CHANNEL: usize = 100_000;
pub const MAX_TURBINE_PROPAGATION_IN_MS: u64 = 100;
pub const MAX_TURBINE_DELAY_IN_TICKS: u64 = MAX_TURBINE_PROPAGATION_IN_MS / MS_PER_TICK;

pub type CompletedSlotsReceiver = Receiver<Vec<u64>>;

// ledger window
pub struct Blocktree {
    db: Arc<Database>,
    meta_cf: LedgerColumn<cf::SlotMeta>,
    dead_slots_cf: LedgerColumn<cf::DeadSlots>,
    erasure_meta_cf: LedgerColumn<cf::ErasureMeta>,
    orphans_cf: LedgerColumn<cf::Orphans>,
    index_cf: LedgerColumn<cf::Index>,
    data_shred_cf: LedgerColumn<cf::ShredData>,
    code_shred_cf: LedgerColumn<cf::ShredCode>,
    transaction_status_cf: LedgerColumn<cf::TransactionStatus>,
    last_root: Arc<RwLock<Slot>>,
    insert_shreds_lock: Arc<Mutex<()>>,
    pub new_shreds_signals: Vec<SyncSender<bool>>,
    pub completed_slots_senders: Vec<SyncSender<Vec<Slot>>>,
}

pub struct IndexMetaWorkingSetEntry {
    index: Index,
    // true only if at least one shred for this Index was inserted since the time this
    // struct was created
    did_insert_occur: bool,
}

pub struct SlotMetaWorkingSetEntry {
    new_slot_meta: Rc<RefCell<SlotMeta>>,
    old_slot_meta: Option<SlotMeta>,
    // True only if at least one shred for this SlotMeta was inserted since the time this
    // struct was created.
    did_insert_occur: bool,
}

pub struct BlocktreeInsertionMetrics {
    pub num_shreds: usize,
    pub insert_lock_elapsed: u64,
    pub insert_shreds_elapsed: u64,
    pub shred_recovery_elapsed: u64,
    pub chaining_elapsed: u64,
    pub commit_working_sets_elapsed: u64,
    pub write_batch_elapsed: u64,
    pub total_elapsed: u64,
    pub num_inserted: u64,
    pub num_recovered: usize,
    pub index_meta_time: u64,
}

impl SlotMetaWorkingSetEntry {
    fn new(new_slot_meta: Rc<RefCell<SlotMeta>>, old_slot_meta: Option<SlotMeta>) -> Self {
        Self {
            new_slot_meta,
            old_slot_meta,
            did_insert_occur: false,
        }
    }
}

impl BlocktreeInsertionMetrics {
    pub fn report_metrics(&self, metric_name: &'static str) {
        datapoint_debug!(
            metric_name,
            ("num_shreds", self.num_shreds as i64, i64),
            ("total_elapsed", self.total_elapsed as i64, i64),
            ("insert_lock_elapsed", self.insert_lock_elapsed as i64, i64),
            (
                "insert_shreds_elapsed",
                self.insert_shreds_elapsed as i64,
                i64
            ),
            (
                "shred_recovery_elapsed",
                self.shred_recovery_elapsed as i64,
                i64
            ),
            ("chaining_elapsed", self.chaining_elapsed as i64, i64),
            (
                "commit_working_sets_elapsed",
                self.commit_working_sets_elapsed as i64,
                i64
            ),
            ("write_batch_elapsed", self.write_batch_elapsed as i64, i64),
            ("num_inserted", self.num_inserted as i64, i64),
            ("num_recovered", self.num_recovered as i64, i64),
        );
    }
}

impl Blocktree {
    /// Opens a Ledger in directory, provides "infinite" window of shreds
    pub fn open(ledger_path: &Path) -> Result<Blocktree> {
        fs::create_dir_all(&ledger_path)?;
        let blocktree_path = ledger_path.join(BLOCKTREE_DIRECTORY);

        adjust_ulimit_nofile();

        // Open the database
        let mut measure = Measure::start("open");
        let db = Database::open(&blocktree_path)?;

        // Create the metadata column family
        let meta_cf = db.column();

        // Create the dead slots column family
        let dead_slots_cf = db.column();

        let erasure_meta_cf = db.column();

        // Create the orphans column family. An "orphan" is defined as
        // the head of a detached chain of slots, i.e. a slot with no
        // known parent
        let orphans_cf = db.column();
        let index_cf = db.column();

        let data_shred_cf = db.column();
        let code_shred_cf = db.column();
        let transaction_status_cf = db.column();

        let db = Arc::new(db);

        // Get max root or 0 if it doesn't exist
        let max_root = db
            .iter::<cf::Root>(IteratorMode::End)?
            .next()
            .map(|(slot, _)| slot)
            .unwrap_or(0);
        let last_root = Arc::new(RwLock::new(max_root));

        measure.stop();
        info!("{:?} {}", blocktree_path, measure);
        Ok(Blocktree {
            db,
            meta_cf,
            dead_slots_cf,
            erasure_meta_cf,
            orphans_cf,
            index_cf,
            data_shred_cf,
            code_shred_cf,
            transaction_status_cf,
            new_shreds_signals: vec![],
            completed_slots_senders: vec![],
            insert_shreds_lock: Arc::new(Mutex::new(())),
            last_root,
        })
    }

    pub fn open_with_signal(
        ledger_path: &Path,
    ) -> Result<(Self, Receiver<bool>, CompletedSlotsReceiver)> {
        let mut blocktree = Self::open(ledger_path)?;
        let (signal_sender, signal_receiver) = sync_channel(1);
        let (completed_slots_sender, completed_slots_receiver) =
            sync_channel(MAX_COMPLETED_SLOTS_IN_CHANNEL);
        blocktree.new_shreds_signals = vec![signal_sender];
        blocktree.completed_slots_senders = vec![completed_slots_sender];

        Ok((blocktree, signal_receiver, completed_slots_receiver))
    }

    pub fn destroy(ledger_path: &Path) -> Result<()> {
        // Database::destroy() fails if the path doesn't exist
        fs::create_dir_all(ledger_path)?;
        let blocktree_path = ledger_path.join(BLOCKTREE_DIRECTORY);
        Database::destroy(&blocktree_path)
    }

    pub fn meta(&self, slot: Slot) -> Result<Option<SlotMeta>> {
        self.meta_cf.get(slot)
    }

    pub fn is_full(&self, slot: Slot) -> bool {
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
        let from_slot = Some(from_slot);
        let batch_end = Some(batch_end);

        let mut write_batch = self
            .db
            .batch()
            .expect("Database Error: Failed to get write batch");
        let end = self
            .meta_cf
            .delete_slot(&mut write_batch, from_slot, batch_end)
            .unwrap_or(false)
            && self
                .erasure_meta_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .data_shred_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .code_shred_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .transaction_status_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .orphans_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .index_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .dead_slots_cf
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false)
            && self
                .db
                .column::<cf::Root>()
                .delete_slot(&mut write_batch, from_slot, batch_end)
                .unwrap_or(false);

        if let Err(e) = self.db.write(write_batch) {
            error!(
                "Error: {:?} while submitting write batch for slot {:?} retrying...",
                e, from_slot
            );
            return Err(e);
        }
        Ok(end)
    }

    pub fn erasure_meta(&self, slot: Slot, set_index: u64) -> Result<Option<ErasureMeta>> {
        self.erasure_meta_cf.get((slot, set_index))
    }

    pub fn orphan(&self, slot: Slot) -> Result<Option<bool>> {
        self.orphans_cf.get(slot)
    }

    pub fn slot_meta_iterator<'a>(
        &'a self,
        slot: Slot,
    ) -> Result<impl Iterator<Item = (u64, SlotMeta)> + 'a> {
        let meta_iter = self
            .db
            .iter::<cf::SlotMeta>(IteratorMode::From(slot, IteratorDirection::Forward))?;
        Ok(meta_iter.map(|(slot, slot_meta_bytes)| {
            (
                slot,
                deserialize(&slot_meta_bytes)
                    .unwrap_or_else(|_| panic!("Could not deserialize SlotMeta for slot {}", slot)),
            )
        }))
    }

    pub fn slot_data_iterator<'a>(
        &'a self,
        slot: Slot,
    ) -> Result<impl Iterator<Item = ((u64, u64), Box<[u8]>)> + 'a> {
        let slot_iterator = self
            .db
            .iter::<cf::ShredData>(IteratorMode::From((slot, 0), IteratorDirection::Forward))?;
        Ok(slot_iterator.take_while(move |((shred_slot, _), _)| *shred_slot == slot))
    }

    pub fn slot_coding_iterator<'a>(
        &'a self,
        slot: Slot,
    ) -> Result<impl Iterator<Item = ((u64, u64), Box<[u8]>)> + 'a> {
        let slot_iterator = self
            .db
            .iter::<cf::ShredCode>(IteratorMode::From((slot, 0), IteratorDirection::Forward))?;
        Ok(slot_iterator.take_while(move |((shred_slot, _), _)| *shred_slot == slot))
    }

    fn try_shred_recovery(
        db: &Database,
        erasure_metas: &HashMap<(u64, u64), ErasureMeta>,
        index_working_set: &mut HashMap<u64, IndexMetaWorkingSetEntry>,
        prev_inserted_datas: &mut HashMap<(u64, u64), Shred>,
        prev_inserted_codes: &mut HashMap<(u64, u64), Shred>,
    ) -> Vec<Shred> {
        let data_cf = db.column::<cf::ShredData>();
        let code_cf = db.column::<cf::ShredCode>();
        let mut recovered_data_shreds = vec![];
        // Recovery rules:
        // 1. Only try recovery around indexes for which new data or coding shreds are received
        // 2. For new data shreds, check if an erasure set exists. If not, don't try recovery
        // 3. Before trying recovery, check if enough number of shreds have been received
        // 3a. Enough number of shreds = (#data + #coding shreds) > erasure.num_data
        for (&(slot, set_index), erasure_meta) in erasure_metas.iter() {
            let submit_metrics = |attempted: bool, status: String, recovered: usize| {
                datapoint_debug!(
                    "blocktree-erasure",
                    ("slot", slot as i64, i64),
                    ("start_index", set_index as i64, i64),
                    ("end_index", erasure_meta.end_indexes().0 as i64, i64),
                    ("recovery_attempted", attempted, bool),
                    ("recovery_status", status, String),
                    ("recovered", recovered as i64, i64),
                );
            };

            let index_meta_entry = index_working_set.get_mut(&slot).expect("Index");
            let index = &mut index_meta_entry.index;
            match erasure_meta.status(&index) {
                ErasureMetaStatus::CanRecover => {
                    // Find shreds for this erasure set and try recovery
                    let slot = index.slot;
                    let mut available_shreds = vec![];
                    (set_index..set_index + erasure_meta.config.num_data() as u64).for_each(|i| {
                        if index.data().is_present(i) {
                            if let Some(shred) =
                                prev_inserted_datas.remove(&(slot, i)).or_else(|| {
                                    let some_data = data_cf
                                        .get_bytes((slot, i))
                                        .expect("Database failure, could not fetch data shred");
                                    if let Some(data) = some_data {
                                        Shred::new_from_serialized_shred(data).ok()
                                    } else {
                                        warn!("Data shred deleted while reading for recovery");
                                        None
                                    }
                                })
                            {
                                available_shreds.push(shred);
                            }
                        }
                    });
                    (set_index..set_index + erasure_meta.config.num_coding() as u64).for_each(
                        |i| {
                            if let Some(shred) = prev_inserted_codes
                                .remove(&(slot, i))
                                .map(|s| {
                                    // Remove from the index so it doesn't get committed. We know
                                    // this is safe to do because everything in
                                    // `prev_inserted_codes` does not yet exist in blocktree
                                    // (guaranteed by `check_cache_coding_shred`)
                                    index.coding_mut().set_present(i, false);
                                    s
                                })
                                .or_else(|| {
                                    if index.coding().is_present(i) {
                                        let some_code = code_cf
                                            .get_bytes((slot, i))
                                            .expect("Database failure, could not fetch code shred");
                                        if let Some(code) = some_code {
                                            Shred::new_from_serialized_shred(code).ok()
                                        } else {
                                            warn!("Code shred deleted while reading for recovery");
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                            {
                                available_shreds.push(shred);
                            }
                        },
                    );
                    if let Ok(mut result) = Shredder::try_recovery(
                        available_shreds,
                        erasure_meta.config.num_data(),
                        erasure_meta.config.num_coding(),
                        set_index as usize,
                        slot,
                    ) {
                        submit_metrics(true, "complete".into(), result.len());
                        recovered_data_shreds.append(&mut result);
                    } else {
                        submit_metrics(true, "incomplete".into(), 0);
                    }
                }
                ErasureMetaStatus::DataFull => {
                    (set_index..set_index + erasure_meta.config.num_coding() as u64).for_each(
                        |i| {
                            // Remove saved coding shreds. We don't need these for future recovery.
                            if prev_inserted_codes.remove(&(slot, i)).is_some() {
                                // Remove from the index so it doesn't get committed. We know
                                // this is safe to do because everything in
                                // `prev_inserted_codes` does not yet exist in blocktree
                                // (guaranteed by `check_cache_coding_shred`)
                                index.coding_mut().set_present(i, false);
                            }
                        },
                    );
                    submit_metrics(false, "complete".into(), 0);
                }
                ErasureMetaStatus::StillNeed(needed) => {
                    submit_metrics(false, format!("still need: {}", needed), 0);
                }
            };
        }
        recovered_data_shreds
    }

    pub fn insert_shreds(
        &self,
        shreds: Vec<Shred>,
        leader_schedule: Option<&Arc<LeaderScheduleCache>>,
        is_trusted: bool,
    ) -> Result<BlocktreeInsertionMetrics> {
        let mut total_start = Measure::start("Total elapsed");
        let mut start = Measure::start("Blocktree lock");
        let _lock = self.insert_shreds_lock.lock().unwrap();
        start.stop();
        let insert_lock_elapsed = start.as_us();

        let db = &*self.db;
        let mut write_batch = db.batch()?;

        let mut just_inserted_coding_shreds = HashMap::new();
        let mut just_inserted_data_shreds = HashMap::new();
        let mut erasure_metas = HashMap::new();
        let mut slot_meta_working_set = HashMap::new();
        let mut index_working_set = HashMap::new();

        let num_shreds = shreds.len();
        let mut start = Measure::start("Shred insertion");
        let mut num_inserted = 0;
        let mut index_meta_time = 0;
        shreds.into_iter().for_each(|shred| {
            if shred.is_data() {
                if self.check_insert_data_shred(
                    shred,
                    &mut index_working_set,
                    &mut slot_meta_working_set,
                    &mut write_batch,
                    &mut just_inserted_data_shreds,
                    &mut index_meta_time,
                    is_trusted,
                ) {
                    num_inserted += 1;
                }
            } else if shred.is_code() {
                self.check_cache_coding_shred(
                    shred,
                    &mut erasure_metas,
                    &mut index_working_set,
                    &mut just_inserted_coding_shreds,
                    &mut index_meta_time,
                    is_trusted,
                );
            } else {
                panic!("There should be no other case");
            }
        });
        start.stop();

        let insert_shreds_elapsed = start.as_us();
        let mut start = Measure::start("Shred recovery");
        let mut num_recovered = 0;
        if let Some(leader_schedule_cache) = leader_schedule {
            let recovered_data = Self::try_shred_recovery(
                &db,
                &erasure_metas,
                &mut index_working_set,
                &mut just_inserted_data_shreds,
                &mut just_inserted_coding_shreds,
            );

            num_recovered = recovered_data.len();
            recovered_data.into_iter().for_each(|shred| {
                if let Some(leader) = leader_schedule_cache.slot_leader_at(shred.slot(), None) {
                    if shred.verify(&leader) {
                        self.check_insert_data_shred(
                            shred,
                            &mut index_working_set,
                            &mut slot_meta_working_set,
                            &mut write_batch,
                            &mut just_inserted_data_shreds,
                            &mut index_meta_time,
                            is_trusted,
                        );
                    }
                }
            });
        }
        start.stop();
        let shred_recovery_elapsed = start.as_us();

        just_inserted_coding_shreds
            .into_iter()
            .for_each(|((_, _), shred)| {
                self.check_insert_coding_shred(
                    shred,
                    &mut index_working_set,
                    &mut write_batch,
                    &mut index_meta_time,
                );
                num_inserted += 1;
            });

        let mut start = Measure::start("Shred recovery");
        // Handle chaining for the members of the slot_meta_working_set that were inserted into,
        // drop the others
        handle_chaining(&self.db, &mut write_batch, &mut slot_meta_working_set)?;
        start.stop();
        let chaining_elapsed = start.as_us();

        let mut start = Measure::start("Commit Working Sets");
        let (should_signal, newly_completed_slots) = commit_slot_meta_working_set(
            &slot_meta_working_set,
            &self.completed_slots_senders,
            &mut write_batch,
        )?;

        for ((slot, set_index), erasure_meta) in erasure_metas {
            write_batch.put::<cf::ErasureMeta>((slot, set_index), &erasure_meta)?;
        }

        for (&slot, index_working_set_entry) in index_working_set.iter() {
            if index_working_set_entry.did_insert_occur {
                write_batch.put::<cf::Index>(slot, &index_working_set_entry.index)?;
            }
        }
        start.stop();
        let commit_working_sets_elapsed = start.as_us();

        let mut start = Measure::start("Write Batch");
        self.db.write(write_batch)?;
        start.stop();
        let write_batch_elapsed = start.as_us();

        send_signals(
            &self.new_shreds_signals,
            &self.completed_slots_senders,
            should_signal,
            newly_completed_slots,
        )?;

        total_start.stop();

        Ok(BlocktreeInsertionMetrics {
            num_shreds,
            total_elapsed: total_start.as_us(),
            insert_lock_elapsed,
            insert_shreds_elapsed,
            shred_recovery_elapsed,
            chaining_elapsed,
            commit_working_sets_elapsed,
            write_batch_elapsed,
            num_inserted,
            num_recovered,
            index_meta_time,
        })
    }

    fn check_insert_coding_shred(
        &self,
        shred: Shred,
        index_working_set: &mut HashMap<u64, IndexMetaWorkingSetEntry>,
        write_batch: &mut WriteBatch,
        index_meta_time: &mut u64,
    ) -> bool {
        let slot = shred.slot();

        let index_meta_working_set_entry =
            get_index_meta_entry(&self.db, slot, index_working_set, index_meta_time);

        let index_meta = &mut index_meta_working_set_entry.index;
        // This gives the index of first coding shred in this FEC block
        // So, all coding shreds in a given FEC block will have the same set index
        self.insert_coding_shred(index_meta, &shred, write_batch)
            .map(|_| {
                index_meta_working_set_entry.did_insert_occur = true;
            })
            .is_ok()
    }

    fn check_cache_coding_shred(
        &self,
        shred: Shred,
        erasure_metas: &mut HashMap<(u64, u64), ErasureMeta>,
        index_working_set: &mut HashMap<u64, IndexMetaWorkingSetEntry>,
        just_received_coding_shreds: &mut HashMap<(u64, u64), Shred>,
        index_meta_time: &mut u64,
        is_trusted: bool,
    ) -> bool {
        let slot = shred.slot();
        let shred_index = u64::from(shred.index());

        let index_meta_working_set_entry =
            get_index_meta_entry(&self.db, slot, index_working_set, index_meta_time);

        let index_meta = &mut index_meta_working_set_entry.index;
        // This gives the index of first coding shred in this FEC block
        // So, all coding shreds in a given FEC block will have the same set index
        if is_trusted
            || Blocktree::should_insert_coding_shred(&shred, index_meta.coding(), &self.last_root)
        {
            let set_index = shred_index - u64::from(shred.coding_header.position);
            let erasure_config = ErasureConfig::new(
                shred.coding_header.num_data_shreds as usize,
                shred.coding_header.num_coding_shreds as usize,
            );

            let erasure_meta = erasure_metas.entry((slot, set_index)).or_insert_with(|| {
                self.erasure_meta_cf
                    .get((slot, set_index))
                    .expect("Expect database get to succeed")
                    .unwrap_or_else(|| ErasureMeta::new(set_index, &erasure_config))
            });

            if erasure_config != erasure_meta.config {
                // ToDo: This is a potential slashing condition
                warn!("Received multiple erasure configs for the same erasure set!!!");
                warn!(
                    "Stored config: {:#?}, new config: {:#?}",
                    erasure_meta.config, erasure_config
                );
            }

            // Should be safe to modify index_meta here. Two cases
            // 1) Recovery happens: Then all inserted erasure metas are removed
            // from just_received_coding_shreds, and nothing wll be committed by
            // `check_insert_coding_shred`, so the coding index meta will not be
            // committed
            index_meta.coding_mut().set_present(shred_index, true);

            just_received_coding_shreds
                .entry((slot, shred_index))
                .or_insert_with(|| shred);

            true
        } else {
            false
        }
    }

    fn check_insert_data_shred(
        &self,
        shred: Shred,
        index_working_set: &mut HashMap<u64, IndexMetaWorkingSetEntry>,
        slot_meta_working_set: &mut HashMap<u64, SlotMetaWorkingSetEntry>,
        write_batch: &mut WriteBatch,
        just_inserted_data_shreds: &mut HashMap<(u64, u64), Shred>,
        index_meta_time: &mut u64,
        is_trusted: bool,
    ) -> bool {
        let slot = shred.slot();
        let shred_index = u64::from(shred.index());

        let index_meta_working_set_entry =
            get_index_meta_entry(&self.db, slot, index_working_set, index_meta_time);

        let index_meta = &mut index_meta_working_set_entry.index;
        let slot_meta_entry =
            get_slot_meta_entry(&self.db, slot_meta_working_set, slot, shred.parent());

        let slot_meta = &mut slot_meta_entry.new_slot_meta.borrow_mut();

        if is_trusted
            || Blocktree::should_insert_data_shred(
                &shred,
                slot_meta,
                index_meta.data(),
                &self.last_root,
            )
        {
            if let Ok(()) =
                self.insert_data_shred(slot_meta, index_meta.data_mut(), &shred, write_batch)
            {
                just_inserted_data_shreds.insert((slot, shred_index), shred);
                index_meta_working_set_entry.did_insert_occur = true;
                slot_meta_entry.did_insert_occur = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    fn should_insert_coding_shred(
        shred: &Shred,
        coding_index: &CodingIndex,
        last_root: &RwLock<u64>,
    ) -> bool {
        let slot = shred.slot();
        let shred_index = shred.index();

        if shred.is_data() || shred_index < u32::from(shred.coding_header.position) {
            return false;
        }

        let set_index = shred_index - u32::from(shred.coding_header.position);
        !(shred.coding_header.num_coding_shreds == 0
            || shred.coding_header.position >= shred.coding_header.num_coding_shreds
            || std::u32::MAX - set_index < u32::from(shred.coding_header.num_coding_shreds) - 1
            || coding_index.is_present(u64::from(shred_index))
            || slot <= *last_root.read().unwrap())
    }

    fn insert_coding_shred(
        &self,
        index_meta: &mut Index,
        shred: &Shred,
        write_batch: &mut WriteBatch,
    ) -> Result<()> {
        let slot = shred.slot();
        let shred_index = u64::from(shred.index());

        // Assert guaranteed by integrity checks on the shred that happen before
        // `insert_coding_shred` is called
        assert!(shred.is_code() && shred_index >= u64::from(shred.coding_header.position));

        // Commit step: commit all changes to the mutable structures at once, or none at all.
        // We don't want only a subset of these changes going through.
        write_batch.put_bytes::<cf::ShredCode>((slot, shred_index), &shred.payload)?;
        index_meta.coding_mut().set_present(shred_index, true);

        Ok(())
    }

    fn should_insert_data_shred(
        shred: &Shred,
        slot_meta: &SlotMeta,
        data_index: &DataIndex,
        last_root: &RwLock<u64>,
    ) -> bool {
        let shred_index = u64::from(shred.index());
        let slot = shred.slot();
        let last_in_slot = if shred.last_in_slot() {
            debug!("got last in slot");
            true
        } else {
            false
        };

        // Check that the data shred doesn't already exist in blocktree
        if shred_index < slot_meta.consumed || data_index.is_present(shred_index) {
            return false;
        }

        // Check that we do not receive shred_index >= than the last_index
        // for the slot
        let last_index = slot_meta.last_index;
        if shred_index >= last_index {
            datapoint_error!(
                "blocktree_error",
                (
                    "error",
                    format!(
                        "Slot {}: received index {} >= slot.last_index {}",
                        slot, shred_index, last_index
                    ),
                    String
                )
            );
            return false;
        }
        // Check that we do not receive a shred with "last_index" true, but shred_index
        // less than our current received
        if last_in_slot && shred_index < slot_meta.received {
            datapoint_error!(
                "blocktree_error",
                (
                    "error",
                    format!(
                        "Slot {}: received shred_index {} < slot.received {}",
                        slot, shred_index, slot_meta.received
                    ),
                    String
                )
            );
            return false;
        }

        let last_root = *last_root.read().unwrap();
        verify_shred_slots(slot, slot_meta.parent_slot, last_root)
    }

    fn insert_data_shred(
        &self,
        slot_meta: &mut SlotMeta,
        data_index: &mut DataIndex,
        shred: &Shred,
        write_batch: &mut WriteBatch,
    ) -> Result<()> {
        let slot = shred.slot();
        let index = u64::from(shred.index());

        let last_in_slot = if shred.last_in_slot() {
            debug!("got last in slot");
            true
        } else {
            false
        };

        let last_in_data = if shred.data_complete() {
            debug!("got last in data");
            true
        } else {
            false
        };

        // Parent for slot meta should have been set by this point
        assert!(!is_orphan(slot_meta));

        let new_consumed = if slot_meta.consumed == index {
            let mut current_index = index + 1;

            while data_index.is_present(current_index) {
                current_index += 1;
            }
            current_index
        } else {
            slot_meta.consumed
        };

        // Commit step: commit all changes to the mutable structures at once, or none at all.
        // We don't want only a subset of these changes going through.
        write_batch.put_bytes::<cf::ShredData>((slot, index), &shred.payload)?;
        update_slot_meta(
            last_in_slot,
            last_in_data,
            slot_meta,
            index as u32,
            new_consumed,
            shred.reference_tick(),
        );
        data_index.set_present(index, true);
        trace!("inserted shred into slot {:?} and index {:?}", slot, index);
        Ok(())
    }

    pub fn get_data_shred(&self, slot: Slot, index: u64) -> Result<Option<Vec<u8>>> {
        self.data_shred_cf.get_bytes((slot, index))
    }

    pub fn get_data_shreds(
        &self,
        slot: Slot,
        from_index: u64,
        to_index: u64,
        buffer: &mut [u8],
    ) -> Result<(u64, usize)> {
        let meta_cf = self.db.column::<cf::SlotMeta>();
        let mut buffer_offset = 0;
        let mut last_index = 0;
        if let Some(meta) = meta_cf.get(slot)? {
            if !meta.is_full() {
                warn!("The slot is not yet full. Will not return any shreds");
                return Ok((last_index, buffer_offset));
            }
            let to_index = cmp::min(to_index, meta.consumed);
            for index in from_index..to_index {
                if let Some(shred_data) = self.get_data_shred(slot, index)? {
                    let shred_len = shred_data.len();
                    if buffer.len().saturating_sub(buffer_offset) >= shred_len {
                        buffer[buffer_offset..buffer_offset + shred_len]
                            .copy_from_slice(&shred_data[..shred_len]);
                        buffer_offset += shred_len;
                        last_index = index;
                        // All shreds are of the same length.
                        // Let's check if we have scope to accomodate another shred
                        // If not, let's break right away, as it'll save on 1 DB read
                        if buffer.len().saturating_sub(buffer_offset) < shred_len {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        Ok((last_index, buffer_offset))
    }

    pub fn get_coding_shred(&self, slot: Slot, index: u64) -> Result<Option<Vec<u8>>> {
        self.code_shred_cf.get_bytes((slot, index))
    }

    // Only used by tests
    #[allow(clippy::too_many_arguments)]
    pub fn write_entries(
        &self,
        start_slot: Slot,
        num_ticks_in_start_slot: u64,
        start_index: u32,
        ticks_per_slot: u64,
        parent: Option<u64>,
        is_full_slot: bool,
        keypair: &Arc<Keypair>,
        entries: Vec<Entry>,
        version: u16,
    ) -> Result<usize> {
        let mut parent_slot = parent.map_or(start_slot.saturating_sub(1), |v| v);
        let num_slots = (start_slot - parent_slot).max(1); // Note: slot 0 has parent slot 0
        assert!(num_ticks_in_start_slot < num_slots * ticks_per_slot);
        let mut remaining_ticks_in_slot = num_slots * ticks_per_slot - num_ticks_in_start_slot;

        let mut current_slot = start_slot;
        let mut shredder =
            Shredder::new(current_slot, parent_slot, 0.0, keypair.clone(), 0, version)
                .expect("Failed to create entry shredder");
        let mut all_shreds = vec![];
        let mut slot_entries = vec![];
        // Find all the entries for start_slot
        for entry in entries.into_iter() {
            if remaining_ticks_in_slot == 0 {
                current_slot += 1;
                parent_slot = current_slot - 1;
                remaining_ticks_in_slot = ticks_per_slot;
                let mut current_entries = vec![];
                std::mem::swap(&mut slot_entries, &mut current_entries);
                let start_index = {
                    if all_shreds.is_empty() {
                        start_index
                    } else {
                        0
                    }
                };
                let (mut data_shreds, mut coding_shreds, _) =
                    shredder.entries_to_shreds(&current_entries, true, start_index);
                all_shreds.append(&mut data_shreds);
                all_shreds.append(&mut coding_shreds);
                shredder = Shredder::new(
                    current_slot,
                    parent_slot,
                    0.0,
                    keypair.clone(),
                    (ticks_per_slot - remaining_ticks_in_slot) as u8,
                    version,
                )
                .expect("Failed to create entry shredder");
            }

            if entry.is_tick() {
                remaining_ticks_in_slot -= 1;
            }
            slot_entries.push(entry);
        }

        if !slot_entries.is_empty() {
            let (mut data_shreds, mut coding_shreds, _) =
                shredder.entries_to_shreds(&slot_entries, is_full_slot, 0);
            all_shreds.append(&mut data_shreds);
            all_shreds.append(&mut coding_shreds);
        }

        let num_shreds = all_shreds.len();
        self.insert_shreds(all_shreds, None, false)?;
        Ok(num_shreds)
    }

    pub fn get_index(&self, slot: Slot) -> Result<Option<Index>> {
        self.index_cf.get(slot)
    }

    /// Manually update the meta for a slot.
    /// Can interfere with automatic meta update and potentially break chaining.
    /// Dangerous. Use with care.
    pub fn put_meta_bytes(&self, slot: Slot, bytes: &[u8]) -> Result<()> {
        self.meta_cf.put_bytes(slot, bytes)
    }

    // Given a start and end entry index, find all the missing
    // indexes in the ledger in the range [start_index, end_index)
    // for the slot with the specified slot
    fn find_missing_indexes<C>(
        db_iterator: &mut DBRawIterator,
        slot: Slot,
        first_timestamp: u64,
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
        let ticks_since_first_insert =
            DEFAULT_TICKS_PER_SECOND * (timestamp() - first_timestamp) / 1000;

        // Seek to the first shred with index >= start_index
        db_iterator.seek(&C::key((slot, start_index)));

        // The index of the first missing shred in the slot
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
            let (current_slot, index) = C::index(&db_iterator.key().expect("Expect a valid key"));

            let current_index = {
                if current_slot > slot {
                    end_index
                } else {
                    index
                }
            };

            let upper_index = cmp::min(current_index, end_index);
            // the tick that will be used to figure out the timeout for this hole
            let reference_tick = u64::from(Shred::reference_tick_from_data(
                &db_iterator.value().expect("couldn't read value"),
            ));

            if ticks_since_first_insert < reference_tick + MAX_TURBINE_DELAY_IN_TICKS {
                // The higher index holes have not timed out yet
                break 'outer;
            }
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
        slot: Slot,
        first_timestamp: u64,
        start_index: u64,
        end_index: u64,
        max_missing: usize,
    ) -> Vec<u64> {
        if let Ok(mut db_iterator) = self
            .db
            .raw_iterator_cf(self.db.cf_handle::<cf::ShredData>())
        {
            Self::find_missing_indexes::<cf::ShredData>(
                &mut db_iterator,
                slot,
                first_timestamp,
                start_index,
                end_index,
                max_missing,
            )
        } else {
            vec![]
        }
    }

    // The `get_block_time` method is not fully implemented (depends on validator timestamp
    // transactions). It currently returns Some(`slot` * DEFAULT_MS_PER_SLOT) offset from 0 for all
    // transactions, and None for any values that would overflow any step.
    pub fn get_block_time(&self, slot: Slot, slot_duration: Duration) -> Option<UnixTimestamp> {
        let ms_per_slot = duration_as_ms(&slot_duration);
        let (offset_millis, overflow) = slot.overflowing_mul(ms_per_slot);
        if !overflow {
            i64::try_from(offset_millis)
                .ok()
                .and_then(|millis| {
                    let median_datetime = Utc.timestamp(0, 0);
                    median_datetime.checked_add_signed(ChronoDuration::milliseconds(millis))
                })
                .map(|dt| dt.timestamp())
        } else {
            None
        }
    }

    pub fn get_confirmed_block(&self, slot: Slot) -> Result<RpcConfirmedBlock> {
        if self.is_root(slot) {
            let slot_meta_cf = self.db.column::<cf::SlotMeta>();
            let slot_meta = slot_meta_cf
                .get(slot)?
                .expect("Rooted slot must exist in SlotMeta");

            let slot_entries = self.get_slot_entries(slot, 0, None)?;
            if !slot_entries.is_empty() {
                let slot_transaction_iterator = slot_entries
                    .iter()
                    .cloned()
                    .flat_map(|entry| entry.transactions);
                let parent_slot_entries = self.get_slot_entries(slot_meta.parent_slot, 0, None)?;
                let previous_blockhash = if !parent_slot_entries.is_empty() {
                    get_last_hash(parent_slot_entries.iter()).unwrap()
                } else {
                    Hash::default()
                };

                let block = RpcConfirmedBlock {
                    previous_blockhash,
                    blockhash: get_last_hash(slot_entries.iter())
                        .unwrap_or_else(|| panic!("Rooted slot {:?} must have blockhash", slot)),
                    parent_slot: slot_meta.parent_slot,
                    transactions: self
                        .map_transactions_to_statuses(slot, slot_transaction_iterator),
                };
                return Ok(block);
            }
        }
        Err(BlocktreeError::SlotNotRooted)
    }

    fn map_transactions_to_statuses<'a>(
        &self,
        slot: Slot,
        iterator: impl Iterator<Item = Transaction> + 'a,
    ) -> Vec<(Transaction, Option<RpcTransactionStatus>)> {
        iterator
            .map(|transaction| {
                let signature = transaction.signatures[0];
                (
                    transaction,
                    self.transaction_status_cf
                        .get((slot, signature))
                        .expect("Expect database get to succeed"),
                )
            })
            .collect()
    }

    pub fn write_transaction_status(
        &self,
        index: (Slot, Signature),
        status: &RpcTransactionStatus,
    ) -> Result<()> {
        self.transaction_status_cf.put(index, status)
    }

    /// Returns the entry vector for the slot starting with `shred_start_index`
    pub fn get_slot_entries(
        &self,
        slot: Slot,
        shred_start_index: u64,
        _max_entries: Option<u64>,
    ) -> Result<Vec<Entry>> {
        self.get_slot_entries_with_shred_info(slot, shred_start_index)
            .map(|x| x.0)
    }

    /// Returns the entry vector for the slot starting with `shred_start_index`, the number of
    /// shreds that comprise the entry vector, and whether the slot is full (consumed all shreds).
    pub fn get_slot_entries_with_shred_info(
        &self,
        slot: Slot,
        start_index: u64,
    ) -> Result<(Vec<Entry>, usize, bool)> {
        let slot_meta_cf = self.db.column::<cf::SlotMeta>();
        let slot_meta = slot_meta_cf.get(slot)?;
        if slot_meta.is_none() {
            return Ok((vec![], 0, false));
        }

        let slot_meta = slot_meta.unwrap();
        // Find all the ranges for the completed data blocks
        let completed_ranges = Self::get_completed_data_ranges(
            start_index as u32,
            &slot_meta.completed_data_indexes[..],
            slot_meta.consumed as u32,
        );
        if completed_ranges.is_empty() {
            return Ok((vec![], 0, false));
        }
        let num_shreds = completed_ranges
            .last()
            .map(|(_, end_index)| u64::from(*end_index) - start_index + 1)
            .unwrap_or(0) as usize;

        let entries: Result<Vec<Vec<Entry>>> = PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                completed_ranges
                    .par_iter()
                    .map(|(start_index, end_index)| {
                        self.get_entries_in_data_block(slot, *start_index, *end_index)
                    })
                    .collect()
            })
        });

        let entries: Vec<Entry> = entries?.into_iter().flatten().collect();
        Ok((entries, num_shreds, slot_meta.is_full()))
    }

    // Get the range of indexes [start_index, end_index] of every completed data block
    fn get_completed_data_ranges(
        mut start_index: u32,
        completed_data_end_indexes: &[u32],
        consumed: u32,
    ) -> Vec<(u32, u32)> {
        let mut completed_data_ranges = vec![];
        let floor = completed_data_end_indexes
            .iter()
            .position(|i| *i >= start_index)
            .unwrap_or_else(|| completed_data_end_indexes.len());

        for i in &completed_data_end_indexes[floor as usize..] {
            // `consumed` is the next missing shred index, but shred `i` existing in
            // completed_data_end_indexes implies it's not missing
            assert!(*i != consumed);

            if *i < consumed {
                completed_data_ranges.push((start_index, *i));
                start_index = *i + 1;
            }
        }

        completed_data_ranges
    }

    fn get_entries_in_data_block(
        &self,
        slot: Slot,
        start_index: u32,
        end_index: u32,
    ) -> Result<Vec<Entry>> {
        let data_shred_cf = self.db.column::<cf::ShredData>();

        // Short circuit on first error
        let data_shreds: Result<Vec<Shred>> = (start_index..=end_index)
            .map(|i| {
                data_shred_cf
                    .get_bytes((slot, u64::from(i)))
                    .and_then(|serialized_shred| {
                        Shred::new_from_serialized_shred(
                            serialized_shred
                                .expect("Shred must exist if shred index was included in a range"),
                        )
                        .map_err(|err| {
                            BlocktreeError::InvalidShredData(Box::new(bincode::ErrorKind::Custom(
                                format!(
                                    "Could not reconstruct shred from shred payload: {:?}",
                                    err
                                ),
                            )))
                        })
                    })
            })
            .collect();

        let data_shreds = data_shreds?;
        assert!(data_shreds.last().unwrap().data_complete());

        let deshred_payload = Shredder::deshred(&data_shreds).map_err(|_| {
            BlocktreeError::InvalidShredData(Box::new(bincode::ErrorKind::Custom(
                "Could not reconstruct data block from constituent shreds".to_string(),
            )))
        })?;

        debug!("{:?} shreds in last FEC set", data_shreds.len(),);
        bincode::deserialize::<Vec<Entry>>(&deshred_payload).map_err(|_| {
            BlocktreeError::InvalidShredData(Box::new(bincode::ErrorKind::Custom(
                "could not reconstruct entries".to_string(),
            )))
        })
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

    pub fn is_root(&self, slot: Slot) -> bool {
        if let Ok(Some(true)) = self.db.get::<cf::Root>(slot) {
            true
        } else {
            false
        }
    }

    pub fn set_roots(&self, rooted_slots: &[u64]) -> Result<()> {
        let mut write_batch = self.db.batch()?;
        for slot in rooted_slots {
            write_batch.put::<cf::Root>(*slot, &true)?;
        }

        self.db.write(write_batch)?;

        let mut last_root = self.last_root.write().unwrap();
        if *last_root == std::u64::MAX {
            *last_root = 0;
        }
        *last_root = cmp::max(*rooted_slots.iter().max().unwrap(), *last_root);
        Ok(())
    }

    pub fn is_dead(&self, slot: Slot) -> bool {
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

    pub fn set_dead_slot(&self, slot: Slot) -> Result<()> {
        self.dead_slots_cf.put(slot, &true)
    }

    pub fn get_orphans(&self, max: Option<usize>) -> Vec<u64> {
        let mut results = vec![];

        let mut iter = self
            .db
            .raw_iterator_cf(self.db.cf_handle::<cf::Orphans>())
            .unwrap();
        iter.seek_to_first();
        while iter.valid() {
            if let Some(max) = max {
                if results.len() > max {
                    break;
                }
            }
            results.push(<cf::Orphans as Column>::index(&iter.key().unwrap()));
            iter.next();
        }
        results
    }

    /// Prune blocktree such that slots higher than `target_slot` are deleted and all references to
    /// higher slots are removed
    pub fn prune(&self, target_slot: Slot) {
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

    pub fn last_root(&self) -> Slot {
        *self.last_root.read().unwrap()
    }

    // find the first available slot in blocktree that has some data in it
    pub fn lowest_slot(&self) -> Slot {
        for (slot, meta) in self
            .slot_meta_iterator(0)
            .expect("unable to iterate over meta")
        {
            if slot > 0 && meta.received > 0 {
                return slot;
            }
        }
        // This means blocktree is empty, should never get here aside from right at boot.
        self.last_root()
    }

    pub fn storage_size(&self) -> Result<u64> {
        self.db.storage_size()
    }
}

fn update_slot_meta(
    is_last_in_slot: bool,
    is_last_in_data: bool,
    slot_meta: &mut SlotMeta,
    index: u32,
    new_consumed: u64,
    reference_tick: u8,
) {
    let maybe_first_insert = slot_meta.received == 0;
    // Index is zero-indexed, while the "received" height starts from 1,
    // so received = index + 1 for the same shred.
    slot_meta.received = cmp::max((u64::from(index) + 1) as u64, slot_meta.received);
    if maybe_first_insert && slot_meta.received > 0 {
        // predict the timestamp of what would have been the first shred in this slot
        let slot_time_elapsed = u64::from(reference_tick) * 1000 / DEFAULT_TICKS_PER_SECOND;
        slot_meta.first_shred_timestamp = timestamp() - slot_time_elapsed;
    }
    slot_meta.consumed = new_consumed;
    slot_meta.last_index = {
        // If the last index in the slot hasn't been set before, then
        // set it to this shred index
        if slot_meta.last_index == std::u64::MAX {
            if is_last_in_slot {
                u64::from(index)
            } else {
                std::u64::MAX
            }
        } else {
            slot_meta.last_index
        }
    };

    if is_last_in_slot || is_last_in_data {
        let position = slot_meta
            .completed_data_indexes
            .iter()
            .position(|completed_data_index| *completed_data_index > index)
            .unwrap_or_else(|| slot_meta.completed_data_indexes.len());

        slot_meta.completed_data_indexes.insert(position, index);
    }
}

fn get_index_meta_entry<'a>(
    db: &Database,
    slot: Slot,
    index_working_set: &'a mut HashMap<u64, IndexMetaWorkingSetEntry>,
    index_meta_time: &mut u64,
) -> &'a mut IndexMetaWorkingSetEntry {
    let index_cf = db.column::<cf::Index>();
    let mut total_start = Measure::start("Total elapsed");
    let res = index_working_set.entry(slot).or_insert_with(|| {
        let newly_inserted_meta = index_cf
            .get(slot)
            .unwrap()
            .unwrap_or_else(|| Index::new(slot));
        IndexMetaWorkingSetEntry {
            index: newly_inserted_meta,
            did_insert_occur: false,
        }
    });
    total_start.stop();
    *index_meta_time += total_start.as_us();
    res
}

fn get_slot_meta_entry<'a>(
    db: &Database,
    slot_meta_working_set: &'a mut HashMap<u64, SlotMetaWorkingSetEntry>,
    slot: Slot,
    parent_slot: Slot,
) -> &'a mut SlotMetaWorkingSetEntry {
    let meta_cf = db.column::<cf::SlotMeta>();

    // Check if we've already inserted the slot metadata for this shred's slot
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

            SlotMetaWorkingSetEntry::new(Rc::new(RefCell::new(meta)), backup)
        } else {
            SlotMetaWorkingSetEntry::new(
                Rc::new(RefCell::new(SlotMeta::new(slot, parent_slot))),
                None,
            )
        }
    })
}

fn get_last_hash<'a>(iterator: impl Iterator<Item = &'a Entry> + 'a) -> Option<Hash> {
    iterator.last().map(|entry| entry.hash)
}

fn is_valid_write_to_slot_0(slot_to_write: u64, parent_slot: Slot, last_root: u64) -> bool {
    slot_to_write == 0 && last_root == 0 && parent_slot == 0
}

fn send_signals(
    new_shreds_signals: &[SyncSender<bool>],
    completed_slots_senders: &[SyncSender<Vec<u64>>],
    should_signal: bool,
    newly_completed_slots: Vec<u64>,
) -> Result<()> {
    if should_signal {
        for signal in new_shreds_signals {
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
                datapoint_error!(
                    "blocktree_error",
                    (
                        "error",
                        "Unable to send newly completed slot because channel is full".to_string(),
                        String
                    ),
                );
            }
        }
    }

    Ok(())
}

fn commit_slot_meta_working_set(
    slot_meta_working_set: &HashMap<u64, SlotMetaWorkingSetEntry>,
    completed_slots_senders: &[SyncSender<Vec<u64>>],
    write_batch: &mut WriteBatch,
) -> Result<(bool, Vec<u64>)> {
    let mut should_signal = false;
    let mut newly_completed_slots = vec![];

    // Check if any metadata was changed, if so, insert the new version of the
    // metadata into the write batch
    for (slot, slot_meta_entry) in slot_meta_working_set.iter() {
        // Any slot that wasn't written to should have been filtered out by now.
        assert!(slot_meta_entry.did_insert_occur);
        let meta: &SlotMeta = &RefCell::borrow(&*slot_meta_entry.new_slot_meta);
        let meta_backup = &slot_meta_entry.old_slot_meta;
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
    working_set: &'a HashMap<u64, SlotMetaWorkingSetEntry>,
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
    slot: Slot,
    insert_map: &'a mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
) -> Result<Rc<RefCell<SlotMeta>>> {
    if let Some(slot_meta) = db.column::<cf::SlotMeta>().get(slot)? {
        insert_map.insert(slot, Rc::new(RefCell::new(slot_meta)));
        Ok(insert_map.get(&slot).unwrap().clone())
    } else {
        // If this slot doesn't exist, make a orphan slot. This way we
        // remember which slots chained to this one when we eventually get a real shred
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
    working_set: &'a HashMap<u64, SlotMetaWorkingSetEntry>,
    chained_slots: &'a HashMap<u64, Rc<RefCell<SlotMeta>>>,
    slot: Slot,
) -> Result<Option<Rc<RefCell<SlotMeta>>>> {
    if let Some(entry) = working_set.get(&slot) {
        Ok(Some(entry.new_slot_meta.clone()))
    } else if let Some(entry) = chained_slots.get(&slot) {
        Ok(Some(entry.clone()))
    } else {
        Ok(None)
    }
}

// Chaining based on latest discussion here: https://github.com/solana-labs/solana/pull/2253
fn handle_chaining(
    db: &Database,
    write_batch: &mut WriteBatch,
    working_set: &mut HashMap<u64, SlotMetaWorkingSetEntry>,
) -> Result<()> {
    // Handle chaining for all the SlotMetas that were inserted into
    working_set.retain(|_, entry| entry.did_insert_occur);
    let mut new_chained_slots = HashMap::new();
    let working_set_slots: Vec<_> = working_set.keys().collect();
    for slot in working_set_slots {
        handle_chaining_for_slot(db, write_batch, working_set, &mut new_chained_slots, *slot)?;
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
    working_set: &HashMap<u64, SlotMetaWorkingSetEntry>,
    new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    slot: Slot,
) -> Result<()> {
    let slot_meta_entry = working_set
        .get(&slot)
        .expect("Slot must exist in the working_set hashmap");

    let meta = &slot_meta_entry.new_slot_meta;
    let meta_backup = &slot_meta_entry.old_slot_meta;

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
    // connected to trunk of the ledger
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
    slot: Slot,
    slot_meta: &Rc<RefCell<SlotMeta>>,
    working_set: &HashMap<u64, SlotMetaWorkingSetEntry>,
    new_chained_slots: &mut HashMap<u64, Rc<RefCell<SlotMeta>>>,
    slot_function: F,
) -> Result<()>
where
    F: Fn(&mut SlotMeta) -> bool,
{
    let mut next_slots: Vec<(u64, Rc<RefCell<SlotMeta>>)> = vec![(slot, slot_meta.clone())];
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
    current_slot: Slot,
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
pub fn create_new_ledger(ledger_path: &Path, genesis_config: &GenesisConfig) -> Result<Hash> {
    Blocktree::destroy(ledger_path)?;
    genesis_config.write(&ledger_path)?;

    // Fill slot 0 with ticks that link back to the genesis_config to bootstrap the ledger.
    let blocktree = Blocktree::open(ledger_path)?;
    let ticks_per_slot = genesis_config.ticks_per_slot;
    let hashes_per_tick = genesis_config.poh_config.hashes_per_tick.unwrap_or(0);
    let entries = create_ticks(ticks_per_slot, hashes_per_tick, genesis_config.hash());
    let last_hash = entries.last().unwrap().hash;
    let version = Shred::version_from_hash(&last_hash);

    let shredder = Shredder::new(0, 0, 0.0, Arc::new(Keypair::new()), 0, version)
        .expect("Failed to create entry shredder");
    let shreds = shredder.entries_to_shreds(&entries, true, 0).0;
    assert!(shreds.last().unwrap().last_in_slot());

    blocktree.insert_shreds(shreds, None, false)?;
    blocktree.set_roots(&[0])?;
    // Explicitly close the blocktree before we create the archived genesis file
    drop(blocktree);

    let archive_path = ledger_path.join("genesis.tar.bz2");
    let args = vec![
        "jcfhS",
        archive_path.to_str().unwrap(),
        "-C",
        ledger_path.to_str().unwrap(),
        "genesis.bin",
        "rocksdb",
    ];
    let output = std::process::Command::new("tar")
        .args(&args)
        .output()
        .unwrap();
    if !output.status.success() {
        use std::io::{Error as IOError, ErrorKind};
        use std::str::from_utf8;
        eprintln!("tar stdout: {}", from_utf8(&output.stdout).unwrap_or("?"));
        eprintln!("tar stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));

        return Err(BlocktreeError::IO(IOError::new(
            ErrorKind::Other,
            format!(
                "Error trying to generate snapshot archive: {}",
                output.status
            ),
        )));
    }

    Ok(last_hash)
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
        $crate::blocktree::get_ledger_path_from_name($crate::tmp_ledger_name!())
    };
}

pub fn get_ledger_path_from_name(name: &str) -> PathBuf {
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
    ($genesis_config:expr) => {
        $crate::blocktree::create_new_ledger_from_name($crate::tmp_ledger_name!(), $genesis_config)
    };
}

pub fn verify_shred_slots(slot: Slot, parent_slot: Slot, last_root: Slot) -> bool {
    if !is_valid_write_to_slot_0(slot, parent_slot, last_root) {
        // Check that the parent_slot < slot
        if parent_slot >= slot {
            return false;
        }

        // Ignore shreds that chain to slots before the last root
        if parent_slot < last_root {
            return false;
        }

        // Above two checks guarantee that by this point, slot > last_root
    }

    true
}

// Same as `create_new_ledger()` but use a temporary ledger name based on the provided `name`
//
// Note: like `create_new_ledger` the returned ledger will have slot 0 full of ticks (and only
// ticks)
pub fn create_new_ledger_from_name(name: &str, genesis_config: &GenesisConfig) -> (PathBuf, Hash) {
    let ledger_path = get_ledger_path_from_name(name);
    let blockhash = create_new_ledger(&ledger_path, genesis_config).unwrap();
    (ledger_path, blockhash)
}

pub fn entries_to_test_shreds(
    entries: Vec<Entry>,
    slot: Slot,
    parent_slot: Slot,
    is_full_slot: bool,
    version: u16,
) -> Vec<Shred> {
    let shredder = Shredder::new(slot, parent_slot, 0.0, Arc::new(Keypair::new()), 0, version)
        .expect("Failed to create entry shredder");

    shredder.entries_to_shreds(&entries, is_full_slot, 0).0
}

// used for tests only
pub fn make_slot_entries(
    slot: Slot,
    parent_slot: Slot,
    num_entries: u64,
) -> (Vec<Shred>, Vec<Entry>) {
    let entries = create_ticks(num_entries, 0, Hash::default());
    let shreds = entries_to_test_shreds(entries.clone(), slot, parent_slot, true, 0);
    (shreds, entries)
}

// used for tests only
pub fn make_many_slot_entries(
    start_slot: Slot,
    num_slots: u64,
    entries_per_slot: u64,
) -> (Vec<Shred>, Vec<Entry>) {
    let mut shreds = vec![];
    let mut entries = vec![];
    for slot in start_slot..start_slot + num_slots {
        let parent_slot = if slot == 0 { 0 } else { slot - 1 };

        let (slot_shreds, slot_entries) = make_slot_entries(slot, parent_slot, entries_per_slot);
        shreds.extend(slot_shreds);
        entries.extend(slot_entries);
    }

    (shreds, entries)
}

// Create shreds for slots that have a parent-child relationship defined by the input `chain`
// used for tests only
pub fn make_chaining_slot_entries(
    chain: &[u64],
    entries_per_slot: u64,
) -> Vec<(Vec<Shred>, Vec<Entry>)> {
    let mut slots_shreds_and_entries = vec![];
    for (i, slot) in chain.iter().enumerate() {
        let parent_slot = {
            if *slot == 0 || i == 0 {
                0
            } else {
                chain[i - 1]
            }
        };

        let result = make_slot_entries(*slot, parent_slot, entries_per_slot);
        slots_shreds_and_entries.push(result);
    }

    slots_shreds_and_entries
}

#[cfg(not(unix))]
fn adjust_ulimit_nofile() {}

#[cfg(unix)]
fn adjust_ulimit_nofile() {
    // Rocks DB likes to have many open files.  The default open file descriptor limit is
    // usually not enough
    let desired_nofile = 65000;

    fn get_nofile() -> libc::rlimit {
        let mut nofile = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut nofile) } != 0 {
            warn!("getrlimit(RLIMIT_NOFILE) failed");
        }
        nofile
    }

    let mut nofile = get_nofile();
    if nofile.rlim_cur < desired_nofile {
        nofile.rlim_cur = desired_nofile;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &nofile) } != 0 {
            error!(
                "Unable to increase the maximum open file descriptor limit to {}",
                desired_nofile
            );

            if cfg!(target_os = "macos") {
                error!("On mac OS you may need to run |sudo launchctl limit maxfiles 65536 200000| first");
            }
        }

        nofile = get_nofile();
    }
    info!("Maximum open file descriptors: {}", nofile.rlim_cur);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        entry::{next_entry, next_entry_mut},
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        leader_schedule::{FixedSchedule, LeaderSchedule},
        shred::{max_ticks_per_n_shreds, DataShredHeader},
    };
    use assert_matches::assert_matches;
    use bincode::serialize;
    use itertools::Itertools;
    use rand::{seq::SliceRandom, thread_rng};
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        hash::{self, hash, Hash},
        instruction::CompiledInstruction,
        packet::PACKET_DATA_SIZE,
        pubkey::Pubkey,
        signature::Signature,
        transaction::TransactionError,
    };
    use std::{iter::FromIterator, time::Duration};

    // used for tests only
    fn make_slot_entries_with_transactions(num_entries: u64) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        for x in 0..num_entries {
            let transaction = Transaction::new_with_compiled_instructions(
                &[&Keypair::new()],
                &[Pubkey::new_rand()],
                Hash::default(),
                vec![Pubkey::new_rand()],
                vec![CompiledInstruction::new(1, &(), vec![0])],
            );
            entries.push(next_entry_mut(&mut Hash::default(), 0, vec![transaction]));
            let mut tick = create_ticks(1, 0, hash(&serialize(&x).unwrap()));
            entries.append(&mut tick);
        }
        entries
    }

    #[test]
    fn test_create_new_ledger() {
        let mint_total = 1_000_000_000_000;
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(mint_total);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
        let ledger = Blocktree::open(&ledger_path).unwrap();

        let ticks = create_ticks(genesis_config.ticks_per_slot, 0, genesis_config.hash());
        let entries = ledger.get_slot_entries(0, 0, None).unwrap();

        assert_eq!(ticks, entries);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_get_bytes() {
        // Create enough entries to ensure there are at least two shreds created
        let num_entries = max_ticks_per_n_shreds(1) + 1;
        assert!(num_entries > 1);

        let (mut shreds, _) = make_slot_entries(0, 0, num_entries);

        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert last shred, test we can retrieve it
        let last_shred = shreds.pop().unwrap();
        assert!(last_shred.index() > 0);
        ledger
            .insert_shreds(vec![last_shred.clone()], None, false)
            .unwrap();

        let serialized_shred = ledger
            .data_shred_cf
            .get_bytes((0, last_shred.index() as u64))
            .unwrap()
            .unwrap();
        let deserialized_shred = Shred::new_from_serialized_shred(serialized_shred).unwrap();

        assert_eq!(last_shred, deserialized_shred);
        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

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
                let mut new_ticks = create_ticks(ticks_per_slot, 0, Hash::default());
                let num_shreds = ledger
                    .write_entries(
                        i,
                        0,
                        0,
                        ticks_per_slot,
                        Some(i.saturating_sub(1)),
                        true,
                        &Arc::new(Keypair::new()),
                        new_ticks.clone(),
                        0,
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
                        // received shred was ticks_per_slot - 2, so received should be ticks_per_slot - 2 + 1
                        assert_eq!(meta.received, ticks_per_slot - 1);
                        // last shred index ticks_per_slot - 2 because that's the shred that made tick_height == ticks_per_slot
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
        let ledger_path = get_tmp_ledger_path!();
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
        ledger
            .code_shred_cf
            .put_bytes(erasure_key, &erasure)
            .unwrap();

        let result = ledger
            .code_shred_cf
            .get_bytes(erasure_key)
            .unwrap()
            .expect("Expected erasure object to exist");

        assert_eq!(result, erasure);

        // Test data column family
        let data = vec![2u8; 16];
        let data_key = (0, 0);
        ledger.data_shred_cf.put_bytes(data_key, &data).unwrap();

        let result = ledger
            .data_shred_cf
            .get_bytes(data_key)
            .unwrap()
            .expect("Expected data object to exist");

        assert_eq!(result, data);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_read_shred_bytes() {
        let slot = 0;
        let (shreds, _) = make_slot_entries(slot, 0, 100);
        let num_shreds = shreds.len() as u64;
        let shred_bufs: Vec<_> = shreds.iter().map(|shred| shred.payload.clone()).collect();

        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blocktree::open(&ledger_path).unwrap();
        ledger.insert_shreds(shreds, None, false).unwrap();

        let mut buf = [0; 4096];
        let (_, bytes) = ledger.get_data_shreds(slot, 0, 1, &mut buf).unwrap();
        assert_eq!(buf[..bytes], shred_bufs[0][..bytes]);

        let (last_index, bytes2) = ledger.get_data_shreds(slot, 0, 2, &mut buf).unwrap();
        assert_eq!(last_index, 1);
        assert!(bytes2 > bytes);
        {
            let shred_data_1 = &buf[..bytes];
            assert_eq!(shred_data_1, &shred_bufs[0][..bytes]);

            let shred_data_2 = &buf[bytes..bytes2];
            assert_eq!(shred_data_2, &shred_bufs[1][..bytes2 - bytes]);
        }

        // buf size part-way into shred[1], should just return shred[0]
        let mut buf = vec![0; bytes + 1];
        let (last_index, bytes3) = ledger.get_data_shreds(slot, 0, 2, &mut buf).unwrap();
        assert_eq!(last_index, 0);
        assert_eq!(bytes3, bytes);

        let mut buf = vec![0; bytes2 - 1];
        let (last_index, bytes4) = ledger.get_data_shreds(slot, 0, 2, &mut buf).unwrap();
        assert_eq!(last_index, 0);
        assert_eq!(bytes4, bytes);

        let mut buf = vec![0; bytes * 2];
        let (last_index, bytes6) = ledger
            .get_data_shreds(slot, num_shreds - 1, num_shreds, &mut buf)
            .unwrap();
        assert_eq!(last_index, num_shreds - 1);

        {
            let shred_data = &buf[..bytes6];
            assert_eq!(shred_data, &shred_bufs[(num_shreds - 1) as usize][..bytes6]);
        }

        // Read out of range
        let (last_index, bytes6) = ledger
            .get_data_shreds(slot, num_shreds, num_shreds + 2, &mut buf)
            .unwrap();
        assert_eq!(last_index, 0);
        assert_eq!(bytes6, 0);

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_insert_data_shreds_basic() {
        // Create enough entries to ensure there are at least two shreds created
        let num_entries = max_ticks_per_n_shreds(1) + 1;
        assert!(num_entries > 1);

        let (mut shreds, entries) = make_slot_entries(0, 0, num_entries);
        let num_shreds = shreds.len() as u64;

        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert last shred, we're missing the other shreds, so no consecutive
        // shreds starting from slot 0, index 0 should exist.
        assert!(shreds.len() > 1);
        let last_shred = shreds.pop().unwrap();
        ledger.insert_shreds(vec![last_shred], None, false).unwrap();
        assert!(ledger.get_slot_entries(0, 0, None).unwrap().is_empty());

        let meta = ledger
            .meta(0)
            .unwrap()
            .expect("Expected new metadata object to be created");
        assert!(meta.consumed == 0 && meta.received == num_shreds);

        // Insert the other shreds, check for consecutive returned entries
        ledger.insert_shreds(shreds, None, false).unwrap();
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
        let num_shreds = 10;
        let num_entries = max_ticks_per_n_shreds(num_shreds);
        let (mut shreds, entries) = make_slot_entries(0, 0, num_entries);
        let num_shreds = shreds.len() as u64;

        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blocktree::open(&ledger_path).unwrap();

        // Insert shreds in reverse, check for consecutive returned shreds
        for i in (0..num_shreds).rev() {
            let shred = shreds.pop().unwrap();
            ledger.insert_shreds(vec![shred], None, false).unwrap();
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
        test_insert_data_shreds_slots("test_insert_data_shreds_slots_single", false);
        test_insert_data_shreds_slots("test_insert_data_shreds_slots_bulk", true);
    }

    /*
        #[test]
        pub fn test_iteration_order() {
            let slot = 0;
            let blocktree_path = get_tmp_ledger_path!();
            {
                let blocktree = Blocktree::open(&blocktree_path).unwrap();

                // Write entries
                let num_entries = 8;
                let entries = make_tiny_test_entries(num_entries);
                let mut shreds = entries.to_single_entry_shreds();

                for (i, b) in shreds.iter_mut().enumerate() {
                    b.set_index(1 << (i * 8));
                    b.set_slot(0);
                }

                blocktree
                    .write_shreds(&shreds)
                    .expect("Expected successful write of shreds");

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
    */

    #[test]
    pub fn test_get_slot_entries1() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let entries = create_ticks(8, 0, Hash::default());
            let shreds = entries_to_test_shreds(entries[0..4].to_vec(), 1, 0, false, 0);
            blocktree
                .insert_shreds(shreds, None, false)
                .expect("Expected successful write of shreds");

            let mut shreds1 = entries_to_test_shreds(entries[4..].to_vec(), 1, 0, false, 0);
            for (i, b) in shreds1.iter_mut().enumerate() {
                b.set_index(8 + i as u32);
            }
            blocktree
                .insert_shreds(shreds1, None, false)
                .expect("Expected successful write of shreds");

            assert_eq!(
                blocktree.get_slot_entries(1, 0, None).unwrap()[2..4],
                entries[2..4],
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    // This test seems to be unnecessary with introduction of data shreds. There are no
    // guarantees that a particular shred index contains a complete entry
    #[test]
    #[ignore]
    pub fn test_get_slot_entries2() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Write entries
            let num_slots = 5 as u64;
            let mut index = 0;
            for slot in 0..num_slots {
                let entries = create_ticks(slot + 1, 0, Hash::default());
                let last_entry = entries.last().unwrap().clone();
                let mut shreds =
                    entries_to_test_shreds(entries, slot, slot.saturating_sub(1), false, 0);
                for b in shreds.iter_mut() {
                    b.set_index(index);
                    b.set_slot(slot as u64);
                    index += 1;
                }
                blocktree
                    .insert_shreds(shreds, None, false)
                    .expect("Expected successful write of shreds");
                assert_eq!(
                    blocktree
                        .get_slot_entries(slot, u64::from(index - 1), None)
                        .unwrap(),
                    vec![last_entry],
                );
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_get_slot_entries3() {
        // Test inserting/fetching shreds which contain multiple entries per shred
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 5 as u64;
            let shreds_per_slot = 5 as u64;
            let entry_serialized_size =
                bincode::serialized_size(&create_ticks(1, 0, Hash::default())).unwrap();
            let entries_per_slot =
                (shreds_per_slot * PACKET_DATA_SIZE as u64) / entry_serialized_size;

            // Write entries
            for slot in 0..num_slots {
                let entries = create_ticks(entries_per_slot, 0, Hash::default());
                let shreds =
                    entries_to_test_shreds(entries.clone(), slot, slot.saturating_sub(1), false, 0);
                assert!(shreds.len() as u64 >= shreds_per_slot);
                blocktree
                    .insert_shreds(shreds, None, false)
                    .expect("Expected successful write of shreds");
                assert_eq!(blocktree.get_slot_entries(slot, 0, None).unwrap(), entries);
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_data_shreds_consecutive() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            // Create enough entries to ensure there are at least two shreds created
            let min_entries = max_ticks_per_n_shreds(1) + 1;
            for i in 0..4 {
                let slot = i;
                let parent_slot = if i == 0 { 0 } else { i - 1 };
                // Write entries
                let num_entries = min_entries * (i + 1);
                let (shreds, original_entries) = make_slot_entries(slot, parent_slot, num_entries);

                let num_shreds = shreds.len() as u64;
                assert!(num_shreds > 1);
                let mut even_shreds = vec![];
                let mut odd_shreds = vec![];

                for (i, shred) in shreds.into_iter().enumerate() {
                    if i % 2 == 0 {
                        even_shreds.push(shred);
                    } else {
                        odd_shreds.push(shred);
                    }
                }

                blocktree.insert_shreds(odd_shreds, None, false).unwrap();

                assert_eq!(blocktree.get_slot_entries(slot, 0, None).unwrap(), vec![]);

                let meta = blocktree.meta(slot).unwrap().unwrap();
                if num_shreds % 2 == 0 {
                    assert_eq!(meta.received, num_shreds);
                } else {
                    trace!("got here");
                    assert_eq!(meta.received, num_shreds - 1);
                }
                assert_eq!(meta.consumed, 0);
                if num_shreds % 2 == 0 {
                    assert_eq!(meta.last_index, num_shreds - 1);
                } else {
                    assert_eq!(meta.last_index, std::u64::MAX);
                }

                blocktree.insert_shreds(even_shreds, None, false).unwrap();

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
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Make duplicate entries and shreds
            let num_unique_entries = 10;
            let (mut original_shreds, original_entries) =
                make_slot_entries(0, 0, num_unique_entries);

            // Discard first shred
            original_shreds.remove(0);

            blocktree
                .insert_shreds(original_shreds, None, false)
                .unwrap();

            assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), vec![]);

            let duplicate_shreds = entries_to_test_shreds(original_entries.clone(), 0, 0, true, 0);
            let num_shreds = duplicate_shreds.len() as u64;
            blocktree
                .insert_shreds(duplicate_shreds, None, false)
                .unwrap();

            assert_eq!(
                blocktree.get_slot_entries(0, 0, None).unwrap(),
                original_entries
            );

            let meta = blocktree.meta(0).unwrap().unwrap();
            assert_eq!(meta.consumed, num_shreds);
            assert_eq!(meta.received, num_shreds);
            assert_eq!(meta.parent_slot, 0);
            assert_eq!(meta.last_index, num_shreds - 1);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_new_shreds_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path!();
        let (ledger, recvr, _) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 50;
        // Create entries for slot 0
        let (mut shreds, _) = make_slot_entries(0, 0, entries_per_slot);
        let shreds_per_slot = shreds.len() as u64;

        // Insert second shred, but we're missing the first shred, so no consecutive
        // shreds starting from slot 0, index 0 should exist.
        ledger
            .insert_shreds(vec![shreds.remove(1)], None, false)
            .unwrap();
        let timer = Duration::new(1, 0);
        assert!(recvr.recv_timeout(timer).is_err());
        // Insert first shred, now we've made a consecutive block
        ledger
            .insert_shreds(vec![shreds.remove(0)], None, false)
            .unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());
        // Insert the rest of the ticks
        ledger.insert_shreds(shreds, None, false).unwrap();
        // Wait to get notified of update, should only be one update
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Create some other slots, and send batches of ticks for each slot such that each slot
        // is missing the tick at shred index == slot index - 1. Thus, no consecutive blocks
        // will be formed
        let num_slots = shreds_per_slot;
        let mut shreds = vec![];
        let mut missing_shreds = vec![];
        for slot in 1..num_slots + 1 {
            let (mut slot_shreds, _) = make_slot_entries(slot, slot - 1, entries_per_slot);
            let missing_shred = slot_shreds.remove(slot as usize - 1);
            shreds.extend(slot_shreds);
            missing_shreds.push(missing_shred);
        }

        // Should be no updates, since no new chains from block 0 were formed
        ledger.insert_shreds(shreds, None, false).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // Insert a shred for each slot that doesn't make a consecutive block, we
        // should get no updates
        let shreds: Vec<_> = (1..num_slots + 1)
            .flat_map(|slot| {
                let (mut shred, _) = make_slot_entries(slot, slot - 1, 1);
                shred[0].set_index(2 * num_slots as u32);
                shred
            })
            .collect();

        ledger.insert_shreds(shreds, None, false).unwrap();
        assert!(recvr.recv_timeout(timer).is_err());

        // For slots 1..num_slots/2, fill in the holes in one batch insertion,
        // so we should only get one signal
        let missing_shreds2 = missing_shreds
            .drain((num_slots / 2) as usize..)
            .collect_vec();
        ledger.insert_shreds(missing_shreds, None, false).unwrap();
        assert!(recvr.recv_timeout(timer).is_ok());
        assert!(recvr.try_recv().is_err());

        // Fill in the holes for each of the remaining slots, we should get a single update
        // for each
        ledger.insert_shreds(missing_shreds2, None, false).unwrap();

        // Destroying database without closing it first is undefined behavior
        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_completed_shreds_signal() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path!();
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;

        // Create shreds for slot 0
        let (mut shreds, _) = make_slot_entries(0, 0, entries_per_slot);

        let shred0 = shreds.remove(0);
        // Insert all but the first shred in the slot, should not be considered complete
        ledger.insert_shreds(shreds, None, false).unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first shred, slot should now be considered complete
        ledger.insert_shreds(vec![shred0], None, false).unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![0]);
    }

    #[test]
    pub fn test_completed_shreds_signal_orphans() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path!();
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;
        let slots = vec![2, 5, 10];
        let mut all_shreds = make_chaining_slot_entries(&slots[..], entries_per_slot);

        // Get the shreds for slot 10, chaining to slot 5
        let (mut orphan_child, _) = all_shreds.remove(2);

        // Get the shreds for slot 5 chaining to slot 2
        let (mut orphan_shreds, _) = all_shreds.remove(1);

        // Insert all but the first shred in the slot, should not be considered complete
        let orphan_child0 = orphan_child.remove(0);
        ledger.insert_shreds(orphan_child, None, false).unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first shred, slot should now be considered complete
        ledger
            .insert_shreds(vec![orphan_child0], None, false)
            .unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![slots[2]]);

        // Insert the shreds for the orphan_slot
        let orphan_shred0 = orphan_shreds.remove(0);
        ledger.insert_shreds(orphan_shreds, None, false).unwrap();
        assert!(recvr.try_recv().is_err());

        // Insert first shred, slot should now be considered complete
        ledger
            .insert_shreds(vec![orphan_shred0], None, false)
            .unwrap();
        assert_eq!(recvr.try_recv().unwrap(), vec![slots[1]]);
    }

    #[test]
    pub fn test_completed_shreds_signal_many() {
        // Initialize ledger
        let ledger_path = get_tmp_ledger_path!();
        let (ledger, _, recvr) = Blocktree::open_with_signal(&ledger_path).unwrap();
        let ledger = Arc::new(ledger);

        let entries_per_slot = 10;
        let mut slots = vec![2, 5, 10];
        let mut all_shreds = make_chaining_slot_entries(&slots[..], entries_per_slot);
        let disconnected_slot = 4;

        let (shreds0, _) = all_shreds.remove(0);
        let (shreds1, _) = all_shreds.remove(0);
        let (shreds2, _) = all_shreds.remove(0);
        let (shreds3, _) = make_slot_entries(disconnected_slot, 1, entries_per_slot);

        let mut all_shreds: Vec<_> = vec![shreds0, shreds1, shreds2, shreds3]
            .into_iter()
            .flatten()
            .collect();

        all_shreds.shuffle(&mut thread_rng());
        ledger.insert_shreds(all_shreds, None, false).unwrap();
        let mut result = recvr.try_recv().unwrap();
        result.sort();
        slots.push(disconnected_slot);
        slots.sort();
        assert_eq!(result, slots);
    }

    #[test]
    pub fn test_handle_chaining_basic() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let entries_per_slot = 5;
            let num_slots = 3;
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Construct the shreds
            let (mut shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);
            let shreds_per_slot = shreds.len() / num_slots as usize;

            // 1) Write to the first slot
            let shreds1 = shreds
                .drain(shreds_per_slot..2 * shreds_per_slot)
                .collect_vec();
            blocktree.insert_shreds(shreds1, None, false).unwrap();
            let s1 = blocktree.meta(1).unwrap().unwrap();
            assert!(s1.next_slots.is_empty());
            // Slot 1 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s1.is_connected);
            assert_eq!(s1.parent_slot, 0);
            assert_eq!(s1.last_index, shreds_per_slot as u64 - 1);

            // 2) Write to the second slot
            let shreds2 = shreds
                .drain(shreds_per_slot..2 * shreds_per_slot)
                .collect_vec();
            blocktree.insert_shreds(shreds2, None, false).unwrap();
            let s2 = blocktree.meta(2).unwrap().unwrap();
            assert!(s2.next_slots.is_empty());
            // Slot 2 is not trunk because slot 0 hasn't been inserted yet
            assert!(!s2.is_connected);
            assert_eq!(s2.parent_slot, 1);
            assert_eq!(s2.last_index, shreds_per_slot as u64 - 1);

            // Check the first slot again, it should chain to the second slot,
            // but still isn't part of the trunk
            let s1 = blocktree.meta(1).unwrap().unwrap();
            assert_eq!(s1.next_slots, vec![2]);
            assert!(!s1.is_connected);
            assert_eq!(s1.parent_slot, 0);
            assert_eq!(s1.last_index, shreds_per_slot as u64 - 1);

            // 3) Write to the zeroth slot, check that every slot
            // is now part of the trunk
            blocktree.insert_shreds(shreds, None, false).unwrap();
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
                assert_eq!(s.last_index, shreds_per_slot as u64 - 1);
                assert!(s.is_connected);
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_handle_chaining_missing_slots() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 30;
            let entries_per_slot = 5;

            // Separate every other slot into two separate vectors
            let mut slots = vec![];
            let mut missing_slots = vec![];
            let mut shreds_per_slot = 2;
            for slot in 0..num_slots {
                let parent_slot = {
                    if slot == 0 {
                        0
                    } else {
                        slot - 1
                    }
                };
                let (slot_shreds, _) = make_slot_entries(slot, parent_slot, entries_per_slot);
                shreds_per_slot = slot_shreds.len();

                if slot % 2 == 1 {
                    slots.extend(slot_shreds);
                } else {
                    missing_slots.extend(slot_shreds);
                }
            }

            // Write the shreds for every other slot
            blocktree.insert_shreds(slots, None, false).unwrap();

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

            // Write the shreds for the other half of the slots that we didn't insert earlier
            blocktree.insert_shreds(missing_slots, None, false).unwrap();

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
                assert_eq!(s.last_index, shreds_per_slot as u64 - 1);
                assert!(s.is_connected);
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_forward_chaining_is_connected() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_slots = 15;
            // Create enough entries to ensure there are at least two shreds created
            let entries_per_slot = max_ticks_per_n_shreds(1) + 1;
            assert!(entries_per_slot > 1);

            let (mut shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);
            let shreds_per_slot = shreds.len() / num_slots as usize;
            assert!(shreds_per_slot > 1);

            // Write the shreds such that every 3rd slot has a gap in the beginning
            let mut missing_shreds = vec![];
            for slot in 0..num_slots {
                let mut shreds_for_slot = shreds.drain(..shreds_per_slot).collect_vec();
                if slot % 3 == 0 {
                    let shred0 = shreds_for_slot.remove(0);
                    missing_shreds.push(shred0);
                    blocktree
                        .insert_shreds(shreds_for_slot, None, false)
                        .unwrap();
                } else {
                    blocktree
                        .insert_shreds(shreds_for_slot, None, false)
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

                assert_eq!(s.last_index, shreds_per_slot as u64 - 1);

                // Other than slot 0, no slots should be part of the trunk
                if i != 0 {
                    assert!(!s.is_connected);
                } else {
                    assert!(s.is_connected);
                }
            }

            // Iteratively finish every 3rd slot, and check that all slots up to and including
            // slot_index + 3 become part of the trunk
            for slot_index in 0..num_slots {
                if slot_index % 3 == 0 {
                    let shred = missing_shreds.remove(0);
                    blocktree.insert_shreds(vec![shred], None, false).unwrap();

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

                        assert_eq!(s.last_index, shreds_per_slot as u64 - 1);
                    }
                }
            }
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
    /*
        #[test]
        pub fn test_chaining_tree() {
            let blocktree_path = get_tmp_ledger_path!();
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

                let (mut shreds, _) = make_many_slot_entries(0, num_slots, entries_per_slot);

                // Insert tree one slot at a time in a random order
                let mut slots: Vec<_> = (0..num_slots).collect();

                // Get shreds for the slot
                slots.shuffle(&mut thread_rng());
                for slot in slots {
                    // Get shreds for the slot "slot"
                    let slot_shreds = &mut shreds
                        [(slot * entries_per_slot) as usize..((slot + 1) * entries_per_slot) as usize];
                    for shred in slot_shreds.iter_mut() {
                        // Get the parent slot of the slot in the tree
                        let slot_parent = {
                            if slot == 0 {
                                0
                            } else {
                                (slot - 1) / branching_factor
                            }
                        };
                        shred.set_parent(slot_parent);
                    }

                    let shared_shreds: Vec<_> = slot_shreds
                        .iter()
                        .cloned()
                        .map(|shred| Arc::new(RwLock::new(shred)))
                        .collect();
                    let mut coding_generator = CodingGenerator::new_from_config(&erasure_config);
                    let coding_shreds = coding_generator.next(&shared_shreds);
                    assert_eq!(coding_shreds.len(), erasure_config.num_coding());

                    let mut rng = thread_rng();

                    // Randomly pick whether to insert erasure or coding shreds first
                    if rng.gen_bool(0.5) {
                        blocktree.write_shreds(slot_shreds).unwrap();
                        blocktree.put_shared_coding_shreds(&coding_shreds).unwrap();
                    } else {
                        blocktree.put_shared_coding_shreds(&coding_shreds).unwrap();
                        blocktree.write_shreds(slot_shreds).unwrap();
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
    */
    #[test]
    pub fn test_get_slots_since() {
        let blocktree_path = get_tmp_ledger_path!();

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
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create shreds and entries
            let entries_per_slot = 1;
            let (mut shreds, _) = make_many_slot_entries(0, 3, entries_per_slot);
            let shreds_per_slot = shreds.len() / 3;

            // Write slot 2, which chains to slot 1. We're missing slot 0,
            // so slot 1 is the orphan
            let shreds_for_slot = shreds.drain((shreds_per_slot * 2)..).collect_vec();
            blocktree
                .insert_shreds(shreds_for_slot, None, false)
                .unwrap();
            let meta = blocktree
                .meta(1)
                .expect("Expect database get to succeed")
                .unwrap();
            assert!(is_orphan(&meta));
            assert_eq!(blocktree.get_orphans(None), vec![1]);

            // Write slot 1 which chains to slot 0, so now slot 0 is the
            // orphan, and slot 1 is no longer the orphan.
            let shreds_for_slot = shreds.drain(shreds_per_slot..).collect_vec();
            blocktree
                .insert_shreds(shreds_for_slot, None, false)
                .unwrap();
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
            let (shred4, _) = make_slot_entries(4, 0, 1);
            let (shred5, _) = make_slot_entries(5, 1, 1);
            blocktree.insert_shreds(shred4, None, false).unwrap();
            blocktree.insert_shreds(shred5, None, false).unwrap();
            assert_eq!(blocktree.get_orphans(None), vec![0]);

            // Write zeroth slot, no more orphans
            blocktree.insert_shreds(shreds, None, false).unwrap();
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
        let blocktree_path = get_ledger_path_from_name(name);
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create shreds and entries
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

                let (mut shred, entry) = make_slot_entries(slot, parent_slot, 1);
                num_shreds_per_slot = shred.len() as u64;
                shred
                    .iter_mut()
                    .enumerate()
                    .for_each(|(_, shred)| shred.set_index(0));
                shreds.extend(shred);
                entries.extend(entry);
            }

            let num_shreds = shreds.len();
            // Write shreds to the database
            if should_bulk_write {
                blocktree.insert_shreds(shreds, None, false).unwrap();
            } else {
                for _ in 0..num_shreds {
                    let shred = shreds.remove(0);
                    blocktree.insert_shreds(vec![shred], None, false).unwrap();
                }
            }

            for i in 0..num_entries - 1 {
                assert_eq!(
                    blocktree.get_slot_entries(i, 0, None).unwrap()[0],
                    entries[i as usize]
                );

                let meta = blocktree.meta(i).unwrap().unwrap();
                assert_eq!(meta.received, 1);
                assert_eq!(meta.last_index, 0);
                if i != 0 {
                    assert_eq!(meta.parent_slot, i - 1);
                    assert_eq!(meta.consumed, 1);
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
        let gap: u64 = 10;
        assert!(gap > 3);
        // Create enough entries to ensure there are at least two shreds created
        let num_entries = max_ticks_per_n_shreds(1) + 1;
        let entries = create_ticks(num_entries, 0, Hash::default());
        let mut shreds = entries_to_test_shreds(entries, slot, 0, true, 0);
        let num_shreds = shreds.len();
        assert!(num_shreds > 1);
        for (i, s) in shreds.iter_mut().enumerate() {
            s.set_index(i as u32 * gap as u32);
            s.set_slot(slot);
        }
        blocktree.insert_shreds(shreds, None, false).unwrap();

        // Index of the first shred is 0
        // Index of the second shred is "gap"
        // Thus, the missing indexes should then be [1, gap - 1] for the input index
        // range of [0, gap)
        let expected: Vec<u64> = (1..gap).collect();
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 0, gap, gap as usize),
            expected
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 1, gap, (gap - 1) as usize),
            expected,
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 0, gap - 1, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap - 2, gap, gap as usize),
            vec![gap - 2, gap - 1],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, gap - 2, gap, 1),
            vec![gap - 2],
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 0, gap, 1),
            vec![1],
        );

        // Test with a range that encompasses a shred with index == gap which was
        // already inserted.
        let mut expected: Vec<u64> = (1..gap).collect();
        expected.push(gap + 1);
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 0, gap + 2, (gap + 2) as usize),
            expected,
        );
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, 0, 0, gap + 2, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );

        for i in 0..num_shreds as u64 {
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
                        0,
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
    fn test_find_missing_data_indexes_timeout() {
        let slot = 0;
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Write entries
        let gap: u64 = 10;
        let shreds: Vec<_> = (0..64)
            .map(|i| {
                Shred::new_from_data(slot, (i * gap) as u32, 0, None, false, false, i as u8, 0)
            })
            .collect();
        blocktree.insert_shreds(shreds, None, false).unwrap();

        let empty: Vec<u64> = vec![];
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, timestamp(), 0, 50, 1),
            empty
        );
        let expected: Vec<_> = (1..=9).collect();
        assert_eq!(
            blocktree.find_missing_data_indexes(slot, timestamp() - 400, 0, 50, 9),
            expected
        );

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
        assert_eq!(blocktree.find_missing_data_indexes(slot, 0, 0, 0, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 0, 5, 5, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 0, 4, 3, 1), empty);
        assert_eq!(blocktree.find_missing_data_indexes(slot, 0, 1, 2, 0), empty);

        let entries = create_ticks(100, 0, Hash::default());
        let mut shreds = entries_to_test_shreds(entries, slot, 0, true, 0);
        assert!(shreds.len() > 2);
        shreds.drain(2..);

        const ONE: u64 = 1;
        const OTHER: u64 = 4;

        shreds[0].set_index(ONE as u32);
        shreds[1].set_index(OTHER as u32);

        // Insert one shred at index = first_index
        blocktree.insert_shreds(shreds, None, false).unwrap();

        const STARTS: u64 = OTHER * 2;
        const END: u64 = OTHER * 3;
        const MAX: usize = 10;
        // The first shred has index = first_index. Thus, for i < first_index,
        // given the input range of [i, first_index], the missing indexes should be
        // [i, first_index - 1]
        for start in 0..STARTS {
            let result = blocktree.find_missing_data_indexes(
                slot, 0, start, // start
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
    pub fn test_no_missing_shred_indexes() {
        let slot = 0;
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        // Write entries
        let num_entries = 10;
        let entries = create_ticks(num_entries, 0, Hash::default());
        let shreds = entries_to_test_shreds(entries, slot, 0, true, 0);
        let num_shreds = shreds.len();

        blocktree.insert_shreds(shreds, None, false).unwrap();

        let empty: Vec<u64> = vec![];
        for i in 0..num_shreds as u64 {
            for j in 0..i {
                assert_eq!(
                    blocktree.find_missing_data_indexes(slot, 0, j, i, (i - j) as usize),
                    empty
                );
            }
        }

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_should_insert_data_shred() {
        let (mut shreds, _) = make_slot_entries(0, 0, 200);
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let index_cf = blocktree.db.column::<cf::Index>();
            let last_root = RwLock::new(0);

            // Insert the first 5 shreds, we don't have a "is_last" shred yet
            blocktree
                .insert_shreds(shreds[0..5].to_vec(), None, false)
                .unwrap();

            // Trying to insert a shred less than `slot_meta.consumed` should fail
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            let index = index_cf.get(0).unwrap().unwrap();
            assert_eq!(slot_meta.consumed, 5);
            assert_eq!(
                Blocktree::should_insert_data_shred(
                    &shreds[1],
                    &slot_meta,
                    index.data(),
                    &last_root
                ),
                false
            );

            // Trying to insert the same shred again should fail
            // skip over shred 5 so the `slot_meta.consumed` doesn't increment
            blocktree
                .insert_shreds(shreds[6..7].to_vec(), None, false)
                .unwrap();
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            let index = index_cf.get(0).unwrap().unwrap();
            assert_eq!(
                Blocktree::should_insert_data_shred(
                    &shreds[6],
                    &slot_meta,
                    index.data(),
                    &last_root
                ),
                false
            );

            // Trying to insert another "is_last" shred with index < the received index should fail
            // skip over shred 7
            blocktree
                .insert_shreds(shreds[8..9].to_vec(), None, false)
                .unwrap();
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            let index = index_cf.get(0).unwrap().unwrap();
            assert_eq!(slot_meta.received, 9);
            let shred7 = {
                if shreds[7].is_data() {
                    shreds[7].set_last_in_slot();
                    shreds[7].clone()
                } else {
                    panic!("Shred in unexpected format")
                }
            };
            assert_eq!(
                Blocktree::should_insert_data_shred(&shred7, &slot_meta, index.data(), &last_root),
                false
            );

            // Insert all pending shreds
            let mut shred8 = shreds[8].clone();
            blocktree.insert_shreds(shreds, None, false).unwrap();
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            let index = index_cf.get(0).unwrap().unwrap();

            // Trying to insert a shred with index > the "is_last" shred should fail
            if shred8.is_data() {
                shred8.set_slot(slot_meta.last_index + 1);
            } else {
                panic!("Shred in unexpected format")
            }
            assert_eq!(
                Blocktree::should_insert_data_shred(&shred7, &slot_meta, index.data(), &last_root),
                false
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_should_insert_coding_shred() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let index_cf = blocktree.db.column::<cf::Index>();
            let last_root = RwLock::new(0);

            let slot = 1;
            let (mut shred, coding) = Shredder::new_coding_shred_header(slot, 11, 11, 11, 10, 0);
            let coding_shred = Shred::new_empty_from_header(
                shred.clone(),
                DataShredHeader::default(),
                coding.clone(),
            );

            // Insert a good coding shred
            assert!(Blocktree::should_insert_coding_shred(
                &coding_shred,
                Index::new(slot).coding(),
                &last_root
            ));

            // Insertion should succeed
            blocktree
                .insert_shreds(vec![coding_shred.clone()], None, false)
                .unwrap();

            // Trying to insert the same shred again should fail
            {
                let index = index_cf.get(shred.slot).unwrap().unwrap();
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }

            shred.index += 1;

            // Establish a baseline that works
            {
                let coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                let index = index_cf.get(shred.slot).unwrap().unwrap();
                assert!(Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }

            // Trying to insert a shred with index < position should fail
            {
                let mut coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                let index = coding_shred.coding_header.position - 1;
                coding_shred.set_index(index as u32);

                let index = index_cf.get(coding_shred.slot()).unwrap().unwrap();
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }

            // Trying to insert shred with num_coding == 0 should fail
            {
                let mut coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                coding_shred.coding_header.num_coding_shreds = 0;
                let index = index_cf.get(coding_shred.slot()).unwrap().unwrap();
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }

            // Trying to insert shred with pos >= num_coding should fail
            {
                let mut coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                coding_shred.coding_header.num_coding_shreds = coding_shred.coding_header.position;
                let index = index_cf.get(coding_shred.slot()).unwrap().unwrap();
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }

            // Trying to insert with set_index with num_coding that would imply the last shred
            // has index > u32::MAX should fail
            {
                let mut coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                coding_shred.coding_header.num_coding_shreds = 3;
                coding_shred.common_header.index = std::u32::MAX - 1;
                coding_shred.coding_header.position = 0;
                let index = index_cf.get(coding_shred.slot()).unwrap().unwrap();
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));

                // Decreasing the number of num_coding_shreds will put it within the allowed limit
                coding_shred.coding_header.num_coding_shreds = 2;
                assert!(Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));

                // Insertion should succeed
                blocktree
                    .insert_shreds(vec![coding_shred], None, false)
                    .unwrap();
            }

            // Trying to insert value into slot <= than last root should fail
            {
                let mut coding_shred = Shred::new_empty_from_header(
                    shred.clone(),
                    DataShredHeader::default(),
                    coding.clone(),
                );
                let index = index_cf.get(coding_shred.slot()).unwrap().unwrap();
                coding_shred.set_slot(*last_root.read().unwrap());
                assert!(!Blocktree::should_insert_coding_shred(
                    &coding_shred,
                    index.coding(),
                    &last_root
                ));
            }
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_insert_multiple_is_last() {
        let (shreds, _) = make_slot_entries(0, 0, 20);
        let num_shreds = shreds.len() as u64;
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();

        blocktree.insert_shreds(shreds, None, false).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();

        assert_eq!(slot_meta.consumed, num_shreds);
        assert_eq!(slot_meta.received, num_shreds);
        assert_eq!(slot_meta.last_index, num_shreds - 1);
        assert!(slot_meta.is_full());

        let (shreds, _) = make_slot_entries(0, 0, 22);
        blocktree.insert_shreds(shreds, None, false).unwrap();
        let slot_meta = blocktree.meta(0).unwrap().unwrap();

        assert_eq!(slot_meta.consumed, num_shreds);
        assert_eq!(slot_meta.received, num_shreds);
        assert_eq!(slot_meta.last_index, num_shreds - 1);
        assert!(slot_meta.is_full());

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_slot_data_iterator() {
        // Construct the shreds
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let shreds_per_slot = 10;
        let slots = vec![2, 4, 8, 12];
        let all_shreds = make_chaining_slot_entries(&slots, shreds_per_slot);
        let slot_8_shreds = all_shreds[2].0.clone();
        for (slot_shreds, _) in all_shreds {
            blocktree.insert_shreds(slot_shreds, None, false).unwrap();
        }

        // Slot doesnt exist, iterator should be empty
        let shred_iter = blocktree.slot_data_iterator(5).unwrap();
        let result: Vec<_> = shred_iter.collect();
        assert_eq!(result, vec![]);

        // Test that the iterator for slot 8 contains what was inserted earlier
        let shred_iter = blocktree.slot_data_iterator(8).unwrap();
        let result: Vec<Shred> = shred_iter
            .filter_map(|(_, bytes)| Shred::new_from_serialized_shred(bytes.to_vec()).ok())
            .collect();
        assert_eq!(result.len(), slot_8_shreds.len());
        assert_eq!(result, slot_8_shreds);

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_set_roots() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&blocktree_path).unwrap();
        let chained_slots = vec![0, 2, 4, 7, 12, 15];
        assert_eq!(blocktree.last_root(), 0);

        blocktree.set_roots(&chained_slots).unwrap();

        assert_eq!(blocktree.last_root(), 15);

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
        let (shreds, _) = make_many_slot_entries(0, 50, 6);
        let shreds_per_slot = shreds.len() as u64 / 50;
        blocktree.insert_shreds(shreds, None, false).unwrap();
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(_, meta)| assert_eq!(meta.last_index, shreds_per_slot - 1));

        blocktree.prune(5);

        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(slot, meta)| {
                assert!(slot <= 5);
                assert_eq!(meta.last_index, shreds_per_slot - 1)
            });

        let data_iter = blocktree
            .data_shred_cf
            .iter(IteratorMode::From((0, 0), IteratorDirection::Forward))
            .unwrap();
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
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blocktree.insert_shreds(shreds, None, false).unwrap();

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
        let (shreds, _) = make_many_slot_entries(0, 5000, 10);
        blocktree.insert_shreds(shreds, None, false).unwrap();

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

    #[test]
    fn test_get_completed_data_ranges() {
        let completed_data_end_indexes = vec![2, 4, 9, 11];

        // Consumed is 1, which means we're missing shred with index 1, should return empty
        let start_index = 0;
        let consumed = 1;
        assert_eq!(
            Blocktree::get_completed_data_ranges(
                start_index,
                &completed_data_end_indexes[..],
                consumed
            ),
            vec![]
        );

        let start_index = 0;
        let consumed = 3;
        assert_eq!(
            Blocktree::get_completed_data_ranges(
                start_index,
                &completed_data_end_indexes[..],
                consumed
            ),
            vec![(0, 2)]
        );

        // Test all possible ranges:
        //
        // `consumed == completed_data_end_indexes[j] + 1`, means we have all the shreds up to index
        // `completed_data_end_indexes[j] + 1`. Thus the completed data blocks is everything in the
        // range:
        // [start_index, completed_data_end_indexes[j]] ==
        // [completed_data_end_indexes[i], completed_data_end_indexes[j]],
        for i in 0..completed_data_end_indexes.len() {
            for j in i..completed_data_end_indexes.len() {
                let start_index = completed_data_end_indexes[i];
                let consumed = completed_data_end_indexes[j] + 1;
                // When start_index == completed_data_end_indexes[i], then that means
                // the shred with index == start_index is a single-shred data block,
                // so the start index is the end index for that data block.
                let mut expected = vec![(start_index, start_index)];
                expected.extend(
                    completed_data_end_indexes[i..=j]
                        .windows(2)
                        .map(|end_indexes| (end_indexes[0] + 1, end_indexes[1])),
                );

                assert_eq!(
                    Blocktree::get_completed_data_ranges(
                        start_index,
                        &completed_data_end_indexes[..],
                        consumed
                    ),
                    expected
                );
            }
        }
    }

    #[test]
    fn test_get_slot_entries_with_shred_count_corruption() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let num_ticks = 8;
            let entries = create_ticks(num_ticks, 0, Hash::default());
            let slot = 1;
            let shreds = entries_to_test_shreds(entries, slot, 0, false, 0);
            let next_shred_index = shreds.len();
            blocktree
                .insert_shreds(shreds, None, false)
                .expect("Expected successful write of shreds");
            assert_eq!(
                blocktree.get_slot_entries(slot, 0, None).unwrap().len() as u64,
                num_ticks
            );

            // Insert an empty shred that won't deshred into entries
            let shreds = vec![Shred::new_from_data(
                slot,
                next_shred_index as u32,
                1,
                Some(&[1, 1, 1]),
                true,
                true,
                0,
                0,
            )];

            // With the corruption, nothing should be returned, even though an
            // earlier data block was valid
            blocktree
                .insert_shreds(shreds, None, false)
                .expect("Expected successful write of shreds");
            assert!(blocktree.get_slot_entries(slot, 0, None).is_err());
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_no_insert_but_modify_slot_meta() {
        // This tests correctness of the SlotMeta in various cases in which a shred
        // that gets filtered out by checks
        let (shreds0, _) = make_slot_entries(0, 0, 200);
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Insert the first 5 shreds, we don't have a "is_last" shred yet
            blocktree
                .insert_shreds(shreds0[0..5].to_vec(), None, false)
                .unwrap();

            // Insert a repetitive shred for slot 's', should get ignored, but also
            // insert shreds that chains to 's', should see the update in the SlotMeta
            // for 's'.
            let (mut shreds2, _) = make_slot_entries(2, 0, 200);
            let (mut shreds3, _) = make_slot_entries(3, 0, 200);
            shreds2.push(shreds0[1].clone());
            shreds3.insert(0, shreds0[1].clone());
            blocktree.insert_shreds(shreds2, None, false).unwrap();
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            assert_eq!(slot_meta.next_slots, vec![2]);
            blocktree.insert_shreds(shreds3, None, false).unwrap();
            let slot_meta = blocktree.meta(0).unwrap().unwrap();
            assert_eq!(slot_meta.next_slots, vec![2, 3]);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_trusted_insert_shreds() {
        // Make shred for slot 1
        let (shreds1, _) = make_slot_entries(1, 0, 1);
        let blocktree_path = get_tmp_ledger_path!();
        let last_root = 100;
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            blocktree.set_roots(&[last_root]).unwrap();

            // Insert will fail, slot < root
            blocktree
                .insert_shreds(shreds1.clone()[..].to_vec(), None, false)
                .unwrap();
            assert!(blocktree.get_data_shred(1, 0).unwrap().is_none());

            // Insert through trusted path will succeed
            blocktree
                .insert_shreds(shreds1[..].to_vec(), None, true)
                .unwrap();
            assert!(blocktree.get_data_shred(1, 0).unwrap().is_some());
        }
    }

    #[test]
    fn test_get_confirmed_block() {
        let slot = 10;
        let entries = make_slot_entries_with_transactions(100);
        let blockhash = get_last_hash(entries.iter()).unwrap();
        let shreds = entries_to_test_shreds(entries.clone(), slot, slot - 1, true, 0);
        let more_shreds = entries_to_test_shreds(entries.clone(), slot + 1, slot, true, 0);
        let ledger_path = get_tmp_ledger_path!();
        let ledger = Blocktree::open(&ledger_path).unwrap();
        ledger.insert_shreds(shreds, None, false).unwrap();
        ledger.insert_shreds(more_shreds, None, false).unwrap();
        ledger.set_roots(&[slot - 1, slot, slot + 1]).unwrap();

        let mut parent_meta = SlotMeta::default();
        parent_meta.parent_slot = std::u64::MAX;
        ledger
            .put_meta_bytes(slot - 1, &serialize(&parent_meta).unwrap())
            .unwrap();

        let expected_transactions: Vec<(Transaction, Option<RpcTransactionStatus>)> = entries
            .iter()
            .cloned()
            .filter(|entry| !entry.is_tick())
            .flat_map(|entry| entry.transactions)
            .map(|transaction| {
                let signature = transaction.signatures[0];
                ledger
                    .transaction_status_cf
                    .put(
                        (slot, signature),
                        &RpcTransactionStatus {
                            status: Ok(()),
                            fee: 42,
                        },
                    )
                    .unwrap();
                ledger
                    .transaction_status_cf
                    .put(
                        (slot + 1, signature),
                        &RpcTransactionStatus {
                            status: Ok(()),
                            fee: 42,
                        },
                    )
                    .unwrap();
                (
                    transaction,
                    Some(RpcTransactionStatus {
                        status: Ok(()),
                        fee: 42,
                    }),
                )
            })
            .collect();

        // Even if marked as root, a slot that is empty of entries should return an error
        let confirmed_block_err = ledger.get_confirmed_block(slot - 1).unwrap_err();
        assert_matches!(confirmed_block_err, BlocktreeError::SlotNotRooted);

        let confirmed_block = ledger.get_confirmed_block(slot).unwrap();
        assert_eq!(confirmed_block.transactions.len(), 100);

        let mut expected_block = RpcConfirmedBlock::default();
        expected_block.transactions = expected_transactions.clone();
        expected_block.parent_slot = slot - 1;
        expected_block.blockhash = blockhash;
        // The previous_blockhash of `expected_block` is default because its parent slot is a
        // root, but empty of entries. This is special handling for snapshot root slots.
        assert_eq!(confirmed_block, expected_block);

        let confirmed_block = ledger.get_confirmed_block(slot + 1).unwrap();
        assert_eq!(confirmed_block.transactions.len(), 100);

        let mut expected_block = RpcConfirmedBlock::default();
        expected_block.transactions = expected_transactions;
        expected_block.parent_slot = slot;
        expected_block.previous_blockhash = blockhash;
        expected_block.blockhash = blockhash;
        assert_eq!(confirmed_block, expected_block);

        let not_root = ledger.get_confirmed_block(slot + 2).unwrap_err();
        assert_matches!(not_root, BlocktreeError::SlotNotRooted);

        drop(ledger);
        Blocktree::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_persist_transaction_status() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let transaction_status_cf = blocktree.db.column::<cf::TransactionStatus>();

            // result not found
            assert!(transaction_status_cf
                .get((0, Signature::default()))
                .unwrap()
                .is_none());

            // insert value
            assert!(transaction_status_cf
                .put(
                    (0, Signature::default()),
                    &RpcTransactionStatus {
                        status: solana_sdk::transaction::Result::<()>::Err(
                            TransactionError::AccountNotFound
                        ),
                        fee: 5u64
                    },
                )
                .is_ok());

            // result found
            let RpcTransactionStatus { status, fee } = transaction_status_cf
                .get((0, Signature::default()))
                .unwrap()
                .unwrap();
            assert_eq!(status, Err(TransactionError::AccountNotFound));
            assert_eq!(fee, 5u64);

            // insert value
            assert!(transaction_status_cf
                .put(
                    (9, Signature::default()),
                    &RpcTransactionStatus {
                        status: solana_sdk::transaction::Result::<()>::Ok(()),
                        fee: 9u64
                    },
                )
                .is_ok());

            // result found
            let RpcTransactionStatus { status, fee } = transaction_status_cf
                .get((9, Signature::default()))
                .unwrap()
                .unwrap();

            // deserialize
            assert_eq!(status, Ok(()));
            assert_eq!(fee, 9u64);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_get_last_hash() {
        let mut entries: Vec<Entry> = vec![];
        let empty_entries_iterator = entries.iter();
        assert!(get_last_hash(empty_entries_iterator).is_none());

        let mut prev_hash = hash::hash(&[42u8]);
        for _ in 0..10 {
            let entry = next_entry(&prev_hash, 1, vec![]);
            prev_hash = entry.hash;
            entries.push(entry);
        }
        let entries_iterator = entries.iter();
        assert_eq!(get_last_hash(entries_iterator).unwrap(), entries[9].hash);
    }

    #[test]
    fn test_map_transactions_to_statuses() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            let transaction_status_cf = blocktree.db.column::<cf::TransactionStatus>();

            let slot = 0;
            let mut transactions: Vec<Transaction> = vec![];
            for x in 0..4 {
                let transaction = Transaction::new_with_compiled_instructions(
                    &[&Keypair::new()],
                    &[Pubkey::new_rand()],
                    Hash::default(),
                    vec![Pubkey::new_rand()],
                    vec![CompiledInstruction::new(1, &(), vec![0])],
                );
                transaction_status_cf
                    .put(
                        (slot, transaction.signatures[0]),
                        &RpcTransactionStatus {
                            status: solana_sdk::transaction::Result::<()>::Err(
                                TransactionError::AccountNotFound,
                            ),
                            fee: x,
                        },
                    )
                    .unwrap();
                transactions.push(transaction);
            }
            // Push transaction that will not have matching status, as a test case
            transactions.push(Transaction::new_with_compiled_instructions(
                &[&Keypair::new()],
                &[Pubkey::new_rand()],
                Hash::default(),
                vec![Pubkey::new_rand()],
                vec![CompiledInstruction::new(1, &(), vec![0])],
            ));

            let map = blocktree.map_transactions_to_statuses(slot, transactions.into_iter());
            assert_eq!(map.len(), 5);
            for x in 0..4 {
                assert_eq!(map[x].1.as_ref().unwrap().fee, x as u64);
            }
            assert_eq!(map[4].1.as_ref(), None);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_lowest_slot() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            for i in 0..10 {
                let slot = i;
                let (shreds, _) = make_slot_entries(slot, 0, 1);
                blocktree.insert_shreds(shreds, None, false).unwrap();
            }
            assert_eq!(blocktree.lowest_slot(), 1);
            blocktree.run_purge_batch(0, 5).unwrap();
            assert_eq!(blocktree.lowest_slot(), 6);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_recovery() {
        let slot = 1;
        let (data_shreds, coding_shreds, leader_schedule_cache) =
            setup_erasure_shreds(slot, 0, 100, 1.0);
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            blocktree
                .insert_shreds(coding_shreds, Some(&leader_schedule_cache), false)
                .unwrap();
            let shred_bufs: Vec<_> = data_shreds
                .iter()
                .map(|shred| shred.payload.clone())
                .collect();

            // Check all the data shreds were recovered
            for (s, buf) in data_shreds.iter().zip(shred_bufs) {
                assert_eq!(
                    blocktree
                        .get_data_shred(s.slot(), s.index() as u64)
                        .unwrap()
                        .unwrap(),
                    buf
                );
            }

            verify_index_integrity(&blocktree, slot);
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_index_integrity() {
        let slot = 1;
        let num_entries = 100;
        let (data_shreds, coding_shreds, leader_schedule_cache) =
            setup_erasure_shreds(slot, 0, num_entries, 1.0);
        assert!(data_shreds.len() > 3);
        assert!(coding_shreds.len() > 3);
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();
            // Test inserting all the shreds
            let all_shreds: Vec<_> = data_shreds
                .iter()
                .cloned()
                .chain(coding_shreds.iter().cloned())
                .collect();
            blocktree
                .insert_shreds(all_shreds, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test inserting just the codes, enough for recovery
            blocktree
                .insert_shreds(coding_shreds.clone(), Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test inserting some codes, but not enough for recovery
            blocktree
                .insert_shreds(
                    coding_shreds[..coding_shreds.len() - 1].to_vec(),
                    Some(&leader_schedule_cache),
                    false,
                )
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test inserting just the codes, and some data, enough for recovery
            let shreds: Vec<_> = data_shreds[..data_shreds.len() - 1]
                .iter()
                .cloned()
                .chain(coding_shreds[..coding_shreds.len() - 1].iter().cloned())
                .collect();
            blocktree
                .insert_shreds(shreds, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test inserting some codes, and some data, but enough for recovery
            let shreds: Vec<_> = data_shreds[..data_shreds.len() / 2 - 1]
                .iter()
                .cloned()
                .chain(coding_shreds[..coding_shreds.len() / 2 - 1].iter().cloned())
                .collect();
            blocktree
                .insert_shreds(shreds, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test inserting all shreds in 2 rounds, make sure nothing is lost
            let shreds1: Vec<_> = data_shreds[..data_shreds.len() / 2 - 1]
                .iter()
                .cloned()
                .chain(coding_shreds[..coding_shreds.len() / 2 - 1].iter().cloned())
                .collect();
            let shreds2: Vec<_> = data_shreds[data_shreds.len() / 2 - 1..]
                .iter()
                .cloned()
                .chain(coding_shreds[coding_shreds.len() / 2 - 1..].iter().cloned())
                .collect();
            blocktree
                .insert_shreds(shreds1, Some(&leader_schedule_cache), false)
                .unwrap();
            blocktree
                .insert_shreds(shreds2, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test not all, but enough data and coding shreds in 2 rounds to trigger recovery,
            // make sure nothing is lost
            let shreds1: Vec<_> = data_shreds[..data_shreds.len() / 2 - 1]
                .iter()
                .cloned()
                .chain(coding_shreds[..coding_shreds.len() / 2 - 1].iter().cloned())
                .collect();
            let shreds2: Vec<_> = data_shreds[data_shreds.len() / 2 - 1..data_shreds.len() / 2]
                .iter()
                .cloned()
                .chain(
                    coding_shreds[coding_shreds.len() / 2 - 1..data_shreds.len() / 2]
                        .iter()
                        .cloned(),
                )
                .collect();
            blocktree
                .insert_shreds(shreds1, Some(&leader_schedule_cache), false)
                .unwrap();
            blocktree
                .insert_shreds(shreds2, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));

            // Test insert shreds in 2 rounds, but not enough to trigger
            // recovery, make sure nothing is lost
            let shreds1: Vec<_> = data_shreds[..data_shreds.len() / 2 - 2]
                .iter()
                .cloned()
                .chain(coding_shreds[..coding_shreds.len() / 2 - 2].iter().cloned())
                .collect();
            let shreds2: Vec<_> = data_shreds[data_shreds.len() / 2 - 2..data_shreds.len() / 2 - 1]
                .iter()
                .cloned()
                .chain(
                    coding_shreds[coding_shreds.len() / 2 - 2..coding_shreds.len() / 2 - 1]
                        .iter()
                        .cloned(),
                )
                .collect();
            blocktree
                .insert_shreds(shreds1, Some(&leader_schedule_cache), false)
                .unwrap();
            blocktree
                .insert_shreds(shreds2, Some(&leader_schedule_cache), false)
                .unwrap();
            verify_index_integrity(&blocktree, slot);
            blocktree.purge_slots(0, Some(slot));
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    fn setup_erasure_shreds(
        slot: u64,
        parent_slot: u64,
        num_entries: u64,
        erasure_rate: f32,
    ) -> (Vec<Shred>, Vec<Shred>, Arc<LeaderScheduleCache>) {
        let entries = make_slot_entries_with_transactions(num_entries);
        let leader_keypair = Arc::new(Keypair::new());
        let shredder = Shredder::new(
            slot,
            parent_slot,
            erasure_rate,
            leader_keypair.clone(),
            0,
            0,
        )
        .expect("Failed in creating shredder");
        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(&entries, true, 0);

        let genesis_config = create_genesis_config(2).genesis_config;
        let bank = Arc::new(Bank::new(&genesis_config));
        let mut leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank);
        let fixed_schedule = FixedSchedule {
            leader_schedule: Arc::new(LeaderSchedule::new_from_schedule(vec![
                leader_keypair.pubkey()
            ])),
            start_epoch: 0,
        };
        leader_schedule_cache.set_fixed_leader_schedule(Some(fixed_schedule));

        (data_shreds, coding_shreds, Arc::new(leader_schedule_cache))
    }

    fn verify_index_integrity(blocktree: &Blocktree, slot: u64) {
        let index = blocktree.get_index(slot).unwrap().unwrap();
        // Test the set of data shreds in the index and in the data column
        // family are the same
        let data_iter = blocktree.slot_data_iterator(slot).unwrap();
        let mut num_data = 0;
        for ((slot, index), _) in data_iter {
            num_data += 1;
            assert!(blocktree.get_data_shred(slot, index).unwrap().is_some());
        }

        // Test the data index doesn't have anything extra
        let num_data_in_index = index.data().num_data();
        assert_eq!(num_data_in_index, num_data);

        // Test the set of coding shreds in the index and in the coding column
        // family are the same
        let coding_iter = blocktree.slot_coding_iterator(slot).unwrap();
        let mut num_coding = 0;
        for ((slot, index), _) in coding_iter {
            num_coding += 1;
            assert!(blocktree.get_coding_shred(slot, index).unwrap().is_some());
        }

        // Test the data index doesn't have anything extra
        let num_coding_in_index = index.coding().num_coding();
        assert_eq!(num_coding_in_index, num_coding);
    }
}
