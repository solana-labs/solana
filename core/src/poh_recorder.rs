//! The `poh_recorder` module provides an object for synchronizing with Proof of History.
//! It synchronizes PoH, bank's register_tick and the ledger
//!
//! PohRecorder will send ticks or entries to a WorkingBank, if the current range of ticks is
//! within the specified WorkingBank range.
//!
//! For Ticks:
//! * new tick_height must be > WorkingBank::min_tick_height && new tick_height must be <= WorkingBank::max_tick_height
//!
//! For Entries:
//! * recorded entry must be >= WorkingBank::min_tick_height && entry must be < WorkingBank::max_tick_height
//!
use solana_ledger::blockstore::Blockstore;
use solana_ledger::entry::Entry;
use solana_ledger::leader_schedule_cache::LeaderScheduleCache;
use solana_ledger::poh::Poh;
use solana_runtime::bank::Bank;
pub use solana_sdk::clock::Slot;
use solana_sdk::clock::NUM_CONSECUTIVE_LEADER_SLOTS;
use solana_sdk::hash::Hash;
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing;
use solana_sdk::transaction::Transaction;
use std::cmp;
use std::sync::mpsc::{channel, Receiver, SendError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;

pub const GRACE_TICKS_FACTOR: u64 = 2;
pub const MAX_GRACE_SLOTS: u64 = 2;

#[derive(Error, Debug, Clone)]
pub enum PohRecorderError {
    #[error("invalid calling object")]
    InvalidCallingObject,

    #[error("max height reached")]
    MaxHeightReached,

    #[error("min height not reached")]
    MinHeightNotReached,

    #[error("send WorkingBankEntry error")]
    SendError(#[from] SendError<WorkingBankEntry>),
}

type Result<T> = std::result::Result<T, PohRecorderError>;

pub type WorkingBankEntry = (Arc<Bank>, (Entry, u64));

#[derive(Clone)]
pub struct WorkingBank {
    pub bank: Arc<Bank>,
    pub min_tick_height: u64,
    pub max_tick_height: u64,
}

pub struct PohRecorder {
    pub poh: Arc<Mutex<Poh>>,
    tick_height: u64,
    clear_bank_signal: Option<SyncSender<bool>>,
    start_slot: Slot,              // parent slot
    start_tick_height: u64,        // first tick_height this recorder will observe
    tick_cache: Vec<(Entry, u64)>, // cache of entry and its tick_height
    working_bank: Option<WorkingBank>,
    sender: Sender<WorkingBankEntry>,
    leader_first_tick_height: Option<u64>,
    leader_last_tick_height: u64, // zero if none
    grace_ticks: u64,
    id: Pubkey,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    poh_config: Arc<PohConfig>,
    ticks_per_slot: u64,
}

impl PohRecorder {
    fn clear_bank(&mut self) {
        if let Some(working_bank) = self.working_bank.take() {
            let bank = working_bank.bank;
            let next_leader_slot = self.leader_schedule_cache.next_leader_slot(
                &self.id,
                bank.slot(),
                &bank,
                Some(&self.blockstore),
                GRACE_TICKS_FACTOR * MAX_GRACE_SLOTS,
            );
            assert_eq!(self.ticks_per_slot, bank.ticks_per_slot());
            let (leader_first_tick_height, leader_last_tick_height, grace_ticks) =
                Self::compute_leader_slot_tick_heights(next_leader_slot, self.ticks_per_slot);
            self.grace_ticks = grace_ticks;
            self.leader_first_tick_height = leader_first_tick_height;
            self.leader_last_tick_height = leader_last_tick_height;
        }
        if let Some(ref signal) = self.clear_bank_signal {
            let _ = signal.try_send(true);
        }
    }

    pub fn would_be_leader(&self, within_next_n_ticks: u64) -> bool {
        self.has_bank()
            || self
                .leader_first_tick_height
                .map_or(false, |leader_first_tick_height| {
                    let ideal_leader_tick_height =
                        leader_first_tick_height.saturating_sub(self.grace_ticks);
                    self.tick_height + within_next_n_ticks >= ideal_leader_tick_height
                        && self.tick_height <= self.leader_last_tick_height
                })
    }

    pub fn leader_after_n_slots(&self, slots: u64) -> Option<Pubkey> {
        let current_slot = self.tick_height.saturating_sub(1) / self.ticks_per_slot;
        self.leader_schedule_cache
            .slot_leader_at(current_slot + slots, None)
    }

    pub fn next_slot_leader(&self) -> Option<Pubkey> {
        self.leader_after_n_slots(1)
    }

    pub fn bank(&self) -> Option<Arc<Bank>> {
        self.working_bank.clone().map(|w| w.bank)
    }

    pub fn has_bank(&self) -> bool {
        self.working_bank.is_some()
    }

    pub fn tick_height(&self) -> u64 {
        self.tick_height
    }

    pub fn ticks_per_slot(&self) -> u64 {
        self.ticks_per_slot
    }

    fn received_any_previous_leader_data(&self, slot: Slot) -> bool {
        (slot.saturating_sub(NUM_CONSECUTIVE_LEADER_SLOTS)..slot).any(|i| {
            // Check if we have received any data in previous leader's slots
            if let Ok(slot_meta) = self.blockstore.meta(i as Slot) {
                if let Some(slot_meta) = slot_meta {
                    slot_meta.received > 0
                } else {
                    false
                }
            } else {
                false
            }
        })
    }

    fn prev_slot_was_mine(&self, current_slot: Slot) -> bool {
        if let Some(leader_id) = self
            .leader_schedule_cache
            .slot_leader_at(current_slot.saturating_sub(1), None)
        {
            leader_id == self.id
        } else {
            false
        }
    }

    fn reached_leader_tick(&self, leader_first_tick_height: u64) -> bool {
        let target_tick_height = leader_first_tick_height.saturating_sub(1);
        let ideal_target_tick_height = target_tick_height.saturating_sub(self.grace_ticks);
        let current_slot = self.tick_height / self.ticks_per_slot;
        // we've approached target_tick_height OR poh was reset to run immediately
        // Or, previous leader didn't transmit in any of its leader slots, so ignore grace ticks
        self.tick_height >= target_tick_height
            || self.start_tick_height + self.grace_ticks == leader_first_tick_height
            || (self.tick_height >= ideal_target_tick_height
                && (self.prev_slot_was_mine(current_slot)
                    || !self.received_any_previous_leader_data(current_slot)))
    }

    /// returns if leader slot has been reached, how many grace ticks were afforded,
    ///   imputed leader_slot and self.start_slot
    /// reached_leader_slot() == true means "ready for a bank"
    pub fn reached_leader_slot(&self) -> (bool, u64, Slot, Slot) {
        trace!(
            "tick_height {}, start_tick_height {}, leader_first_tick_height {:?}, grace_ticks {}, has_bank {}",
            self.tick_height,
            self.start_tick_height,
            self.leader_first_tick_height,
            self.grace_ticks,
            self.has_bank()
        );

        let next_tick_height = self.tick_height + 1;
        let next_leader_slot = (next_tick_height - 1) / self.ticks_per_slot;
        if let Some(leader_first_tick_height) = self.leader_first_tick_height {
            let target_tick_height = leader_first_tick_height.saturating_sub(1);
            if self.reached_leader_tick(leader_first_tick_height) {
                assert!(next_tick_height >= self.start_tick_height);
                let ideal_target_tick_height = target_tick_height.saturating_sub(self.grace_ticks);

                return (
                    true,
                    self.tick_height.saturating_sub(ideal_target_tick_height),
                    next_leader_slot,
                    self.start_slot,
                );
            }
        }
        (false, 0, next_leader_slot, self.start_slot)
    }

    // returns (leader_first_tick_height, leader_last_tick_height, grace_ticks) given the next
    //  slot this recorder will lead
    fn compute_leader_slot_tick_heights(
        next_leader_slot: Option<(Slot, Slot)>,
        ticks_per_slot: u64,
    ) -> (Option<u64>, u64, u64) {
        next_leader_slot
            .map(|(first_slot, last_slot)| {
                let leader_first_tick_height = first_slot * ticks_per_slot + 1;
                let last_tick_height = (last_slot + 1) * ticks_per_slot;
                let num_slots = last_slot - first_slot + 1;
                let grace_ticks = cmp::min(
                    ticks_per_slot * MAX_GRACE_SLOTS,
                    ticks_per_slot * num_slots / GRACE_TICKS_FACTOR,
                );
                (
                    Some(leader_first_tick_height + grace_ticks),
                    last_tick_height,
                    grace_ticks,
                )
            })
            .unwrap_or((
                None,
                0,
                cmp::min(
                    ticks_per_slot * MAX_GRACE_SLOTS,
                    ticks_per_slot * NUM_CONSECUTIVE_LEADER_SLOTS / GRACE_TICKS_FACTOR,
                ),
            ))
    }

    // synchronize PoH with a bank
    pub fn reset(
        &mut self,
        blockhash: Hash,
        start_slot: Slot,
        next_leader_slot: Option<(Slot, Slot)>,
    ) {
        self.clear_bank();
        let mut cache = vec![];
        {
            let mut poh = self.poh.lock().unwrap();
            info!(
                "reset poh from: {},{},{} to: {},{}",
                poh.hash, self.tick_height, self.start_slot, blockhash, start_slot
            );
            poh.reset(blockhash, self.poh_config.hashes_per_tick);
        }

        std::mem::swap(&mut cache, &mut self.tick_cache);

        self.start_slot = start_slot;
        self.tick_height = (start_slot + 1) * self.ticks_per_slot;
        self.start_tick_height = self.tick_height + 1;

        let (leader_first_tick_height, leader_last_tick_height, grace_ticks) =
            Self::compute_leader_slot_tick_heights(next_leader_slot, self.ticks_per_slot);
        self.grace_ticks = grace_ticks;
        self.leader_first_tick_height = leader_first_tick_height;
        self.leader_last_tick_height = leader_last_tick_height;
    }

    pub fn set_working_bank(&mut self, working_bank: WorkingBank) {
        trace!("new working bank");
        assert_eq!(working_bank.bank.ticks_per_slot(), self.ticks_per_slot());
        self.working_bank = Some(working_bank);
        let _ = self.flush_cache(false);
    }
    pub fn set_bank(&mut self, bank: &Arc<Bank>) {
        let working_bank = WorkingBank {
            bank: bank.clone(),
            min_tick_height: bank.tick_height(),
            max_tick_height: bank.max_tick_height(),
        };
        self.set_working_bank(working_bank);
    }

    // Flush cache will delay flushing the cache for a bank until it past the WorkingBank::min_tick_height
    // On a record flush will flush the cache at the WorkingBank::min_tick_height, since a record
    // occurs after the min_tick_height was generated
    fn flush_cache(&mut self, tick: bool) -> Result<()> {
        // check_tick_height is called before flush cache, so it cannot overrun the bank
        // so a bank that is so late that it's slot fully generated before it starts recording
        // will fail instead of broadcasting any ticks
        let working_bank = self
            .working_bank
            .as_ref()
            .ok_or(PohRecorderError::MaxHeightReached)?;
        if self.tick_height < working_bank.min_tick_height {
            return Err(PohRecorderError::MinHeightNotReached);
        }
        if tick && self.tick_height == working_bank.min_tick_height {
            return Err(PohRecorderError::MinHeightNotReached);
        }

        let entry_count = self
            .tick_cache
            .iter()
            .take_while(|x| x.1 <= working_bank.max_tick_height)
            .count();
        let mut send_result: std::result::Result<(), SendError<WorkingBankEntry>> = Ok(());

        if entry_count > 0 {
            trace!(
                "flush_cache: bank_slot: {} tick_height: {} max: {} sending: {}",
                working_bank.bank.slot(),
                working_bank.bank.tick_height(),
                working_bank.max_tick_height,
                entry_count,
            );

            for tick in &self.tick_cache[..entry_count] {
                working_bank.bank.register_tick(&tick.0.hash);
                send_result = self.sender.send((working_bank.bank.clone(), tick.clone()));
                if send_result.is_err() {
                    break;
                }
            }
        }
        if self.tick_height >= working_bank.max_tick_height {
            info!(
                "poh_record: max_tick_height {} reached, clearing working_bank {}",
                working_bank.max_tick_height,
                working_bank.bank.slot()
            );
            let working_slot =
                (working_bank.max_tick_height / self.ticks_per_slot).saturating_sub(1);
            self.start_slot = working_slot;
            self.start_tick_height = working_slot * self.ticks_per_slot + 1;
            self.clear_bank();
        }
        if send_result.is_err() {
            info!("WorkingBank::sender disconnected {:?}", send_result);
            // revert the cache, but clear the working bank
            self.clear_bank();
        } else {
            // commit the flush
            let _ = self.tick_cache.drain(..entry_count);
        }

        Ok(())
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let poh_entry = self.poh.lock().unwrap().tick();
        inc_new_counter_info!(
            "poh_recorder-tick_lock_contention",
            timing::duration_as_us(&now.elapsed()) as usize
        );
        let now = Instant::now();
        if let Some(poh_entry) = poh_entry {
            self.tick_height += 1;
            trace!("tick_height {}", self.tick_height);

            if self.leader_first_tick_height.is_none() {
                inc_new_counter_info!(
                    "poh_recorder-tick_overhead",
                    timing::duration_as_us(&now.elapsed()) as usize
                );
                return;
            }

            let entry = Entry {
                num_hashes: poh_entry.num_hashes,
                hash: poh_entry.hash,
                transactions: vec![],
            };

            self.tick_cache.push((entry, self.tick_height));
            let _ = self.flush_cache(true);
        }
        inc_new_counter_info!(
            "poh_recorder-tick_overhead",
            timing::duration_as_us(&now.elapsed()) as usize
        );
    }

    pub fn record(
        &mut self,
        bank_slot: Slot,
        mixin: Hash,
        transactions: Vec<Transaction>,
    ) -> Result<()> {
        // Entries without transactions are used to track real-time passing in the ledger and
        // cannot be generated by `record()`
        assert!(!transactions.is_empty(), "No transactions provided");
        loop {
            self.flush_cache(false)?;

            let working_bank = self
                .working_bank
                .as_ref()
                .ok_or(PohRecorderError::MaxHeightReached)?;
            if bank_slot != working_bank.bank.slot() {
                return Err(PohRecorderError::MaxHeightReached);
            }

            {
                let now = Instant::now();
                let mut poh_lock = self.poh.lock().unwrap();
                inc_new_counter_warn!(
                    "poh_recorder-record_lock_contention",
                    timing::duration_as_us(&now.elapsed()) as usize
                );
                let now = Instant::now();
                let res = poh_lock.record(mixin);
                inc_new_counter_warn!(
                    "poh_recorder-record_ms",
                    timing::duration_as_us(&now.elapsed()) as usize
                );
                if let Some(poh_entry) = res {
                    let entry = Entry {
                        num_hashes: poh_entry.num_hashes,
                        hash: poh_entry.hash,
                        transactions,
                    };
                    self.sender
                        .send((working_bank.bank.clone(), (entry, self.tick_height)))?;
                    return Ok(());
                }
            }
            // record() might fail if the next PoH hash needs to be a tick.  But that's ok, tick()
            // and re-record()
            self.tick();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_clear_signal(
        tick_height: u64,
        last_entry_hash: Hash,
        start_slot: Slot,
        next_leader_slot: Option<(Slot, Slot)>,
        ticks_per_slot: u64,
        id: &Pubkey,
        blockstore: &Arc<Blockstore>,
        clear_bank_signal: Option<SyncSender<bool>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        poh_config: &Arc<PohConfig>,
    ) -> (Self, Receiver<WorkingBankEntry>) {
        let poh = Arc::new(Mutex::new(Poh::new(
            last_entry_hash,
            poh_config.hashes_per_tick,
        )));
        let (sender, receiver) = channel();
        let (leader_first_tick_height, leader_last_tick_height, grace_ticks) =
            Self::compute_leader_slot_tick_heights(next_leader_slot, ticks_per_slot);
        (
            Self {
                poh,
                tick_height,
                tick_cache: vec![],
                working_bank: None,
                sender,
                clear_bank_signal,
                start_slot,
                start_tick_height: tick_height + 1,
                leader_first_tick_height,
                leader_last_tick_height,
                grace_ticks,
                id: *id,
                blockstore: blockstore.clone(),
                leader_schedule_cache: leader_schedule_cache.clone(),
                ticks_per_slot,
                poh_config: poh_config.clone(),
            },
            receiver,
        )
    }

    /// A recorder to synchronize PoH with the following data structures
    /// * bank - the LastId's queue is updated on `tick` and `record` events
    /// * sender - the Entry channel that outputs to the ledger
    pub fn new(
        tick_height: u64,
        last_entry_hash: Hash,
        start_slot: Slot,
        next_leader_slot: Option<(Slot, Slot)>,
        ticks_per_slot: u64,
        id: &Pubkey,
        blockstore: &Arc<Blockstore>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        poh_config: &Arc<PohConfig>,
    ) -> (Self, Receiver<WorkingBankEntry>) {
        Self::new_with_clear_signal(
            tick_height,
            last_entry_hash,
            start_slot,
            next_leader_slot,
            ticks_per_slot,
            id,
            blockstore,
            None,
            leader_schedule_cache,
            poh_config,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use solana_ledger::{blockstore::Blockstore, blockstore_meta::SlotMeta, get_tmp_ledger_path};
    use solana_perf::test_tx::test_tx;
    use solana_sdk::clock::DEFAULT_TICKS_PER_SLOT;
    use solana_sdk::hash::hash;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn test_poh_recorder_no_zero_tick() {
        let prev_hash = Hash::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");

            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.tick_cache[0].1, 1);
            assert_eq!(poh_recorder.tick_height, 1);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_height_is_last_tick() {
        let prev_hash = Hash::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");

            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            assert_eq!(poh_recorder.tick_cache[1].1, 2);
            assert_eq!(poh_recorder.tick_height, 2);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_reset_clears_cache() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            poh_recorder.reset(Hash::default(), 0, Some((4, 4)));
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_clear() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            assert!(poh_recorder.working_bank.is_some());
            poh_recorder.clear_bank();
            assert!(poh_recorder.working_bank.is_none());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_sent_after_min() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            poh_recorder.tick();
            //tick height equal to min_tick_height
            //no tick has been sent
            assert_eq!(poh_recorder.tick_cache.last().unwrap().1, 2);
            assert!(entry_receiver.try_recv().is_err());

            // all ticks are sent after height > min
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_height, 3);
            assert_eq!(poh_recorder.tick_cache.len(), 0);
            let mut num_entries = 0;
            while let Ok((wbank, (_entry, _tick_height))) = entry_receiver.try_recv() {
                assert_eq!(wbank.slot(), bank.slot());
                num_entries += 1;
            }
            assert_eq!(num_entries, 3);
            assert!(poh_recorder.working_bank.is_none());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_sent_upto_and_including_max() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.last().unwrap().1, 4);
            assert_eq!(poh_recorder.tick_height, 4);

            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();

            assert_eq!(poh_recorder.tick_height, 5);
            assert!(poh_recorder.working_bank.is_none());
            let mut num_entries = 0;
            while entry_receiver.try_recv().is_ok() {
                num_entries += 1;
            }
            assert_eq!(num_entries, 3);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_to_early() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder.record(bank.slot(), h1, vec![tx]).is_err());
            assert!(entry_receiver.try_recv().is_err());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_bad_slot() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.tick_height, 1);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert_matches!(
                poh_recorder.record(bank.slot() + 1, h1, vec![tx]),
                Err(PohRecorderError::MaxHeightReached)
            );
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_at_min_passes() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.tick_height, 1);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder.record(bank.slot(), h1, vec![tx]).is_ok());
            assert_eq!(poh_recorder.tick_cache.len(), 0);

            //tick in the cache + entry
            let (_bank, (e, _tick_height)) = entry_receiver.recv().expect("recv 1");
            assert!(e.is_tick());
            let (_bank, (e, _tick_height)) = entry_receiver.recv().expect("recv 2");
            assert!(!e.is_tick());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_at_max_fails() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_height, 2);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder.record(bank.slot(), h1, vec![tx]).is_err());

            let (_bank, (entry, _tick_height)) = entry_receiver.recv().unwrap();
            assert!(entry.is_tick());
            let (_bank, (entry, _tick_height)) = entry_receiver.recv().unwrap();
            assert!(entry.is_tick());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_cache_on_disconnect() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_height, 2);
            drop(entry_receiver);
            poh_recorder.tick();
            assert!(poh_recorder.working_bank.is_none());
            assert_eq!(poh_recorder.tick_cache.len(), 3);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_current() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            let hash = poh_recorder.poh.lock().unwrap().hash;
            poh_recorder.reset(hash, 0, Some((4, 4)));
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_with_cached() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            poh_recorder.reset(poh_recorder.tick_cache[0].0.hash, 0, Some((4, 4)));
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_to_new_value() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some((4, 4)),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 4);
            assert_eq!(poh_recorder.tick_height, 4);
            poh_recorder.reset(hash(b"hello"), 0, Some((4, 4))); // parent slot 0 implies tick_height of 3
            assert_eq!(poh_recorder.tick_cache.len(), 0);
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_height, DEFAULT_TICKS_PER_SLOT + 1);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_clear_bank() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.reset(hash(b"hello"), 0, Some((4, 4)));
            assert!(poh_recorder.working_bank.is_none());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    pub fn test_clear_signal() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let (sender, receiver) = sync_channel(1);
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new_with_clear_signal(
                0,
                Hash::default(),
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                Some(sender),
                &Arc::new(LeaderScheduleCache::default()),
                &Arc::new(PohConfig::default()),
            );
            poh_recorder.set_bank(&bank);
            poh_recorder.clear_bank();
            assert!(receiver.try_recv().is_ok());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_reset_start_slot() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let ticks_per_slot = 5;
            let GenesisConfigInfo {
                mut genesis_config, ..
            } = create_genesis_config(2);
            genesis_config.ticks_per_slot = ticks_per_slot;
            let bank = Arc::new(Bank::new(&genesis_config));

            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            let end_slot = 3;
            let max_tick_height = (end_slot + 1) * ticks_per_slot;
            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height,
            };

            poh_recorder.set_working_bank(working_bank);
            for _ in 0..max_tick_height {
                poh_recorder.tick();
            }

            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder.record(bank.slot(), h1, vec![tx]).is_err());
            assert!(poh_recorder.working_bank.is_none());
            // Make sure the starting slot is updated
            assert_eq!(poh_recorder.start_slot, end_slot);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reached_leader_tick() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &leader_schedule_cache,
                &Arc::new(PohConfig::default()),
            );

            let bootstrap_validator_id = leader_schedule_cache.slot_leader_at(0, None).unwrap();

            assert_eq!(poh_recorder.reached_leader_tick(0), true);

            let grace_ticks = bank.ticks_per_slot() * MAX_GRACE_SLOTS;
            let new_tick_height = NUM_CONSECUTIVE_LEADER_SLOTS * bank.ticks_per_slot();
            for _ in 0..new_tick_height {
                poh_recorder.tick();
            }

            poh_recorder.grace_ticks = grace_ticks;
            // True, as previous leader did not transmit in its slots
            assert_eq!(
                poh_recorder.reached_leader_tick(new_tick_height + grace_ticks),
                true
            );

            let mut parent_meta = SlotMeta::default();
            parent_meta.received = 1;
            poh_recorder
                .blockstore
                .put_meta_bytes(0, &serialize(&parent_meta).unwrap())
                .unwrap();

            // False, as previous leader transmitted in one of its recent slots
            // and grace ticks have not expired
            assert_eq!(
                poh_recorder.reached_leader_tick(new_tick_height + grace_ticks),
                false
            );

            // From the bootstrap validator's perspective, it should have reached
            // the tick
            poh_recorder.id = bootstrap_validator_id;
            assert!(poh_recorder.reached_leader_tick(new_tick_height + grace_ticks));
        }
    }

    #[test]
    fn test_reached_leader_slot() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            // Test that with no next leader slot, we don't reach the leader slot
            assert_eq!(poh_recorder.reached_leader_slot().0, false);

            // Test that with no next leader slot in reset(), we don't reach the leader slot
            poh_recorder.reset(bank.last_blockhash(), 0, None);
            assert_eq!(poh_recorder.reached_leader_slot().0, false);

            // Provide a leader slot one slot down
            poh_recorder.reset(bank.last_blockhash(), 0, Some((2, 2)));

            let init_ticks = poh_recorder.tick_height();

            // Send one slot worth of ticks
            for _ in 0..bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // Tick should be recorded
            assert_eq!(
                poh_recorder.tick_height(),
                init_ticks + bank.ticks_per_slot()
            );

            let mut parent_meta = SlotMeta::default();
            parent_meta.received = 1;
            poh_recorder
                .blockstore
                .put_meta_bytes(0, &serialize(&parent_meta).unwrap())
                .unwrap();

            // Test that we don't reach the leader slot because of grace ticks
            assert_eq!(poh_recorder.reached_leader_slot().0, false);

            // reset poh now. we should immediately be leader
            poh_recorder.reset(bank.last_blockhash(), 1, Some((2, 2)));
            let (reached_leader_slot, grace_ticks, leader_slot, ..) =
                poh_recorder.reached_leader_slot();
            assert_eq!(reached_leader_slot, true);
            assert_eq!(grace_ticks, 0);
            assert_eq!(leader_slot, 2);

            // Now test that with grace ticks we can reach leader slot
            // Set the leader slot one slot down
            poh_recorder.reset(bank.last_blockhash(), 1, Some((3, 3)));

            // Send one slot worth of ticks ("skips" slot 2)
            for _ in 0..bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // We are not the leader yet, as expected
            assert_eq!(poh_recorder.reached_leader_slot().0, false);

            // Send the grace ticks
            for _ in 0..bank.ticks_per_slot() / GRACE_TICKS_FACTOR {
                poh_recorder.tick();
            }

            // We should be the leader now
            let (reached_leader_slot, grace_ticks, leader_slot, ..) =
                poh_recorder.reached_leader_slot();
            assert_eq!(reached_leader_slot, true);
            assert_eq!(grace_ticks, bank.ticks_per_slot() / GRACE_TICKS_FACTOR);
            assert_eq!(leader_slot, 3);

            // Let's test that correct grace ticks are reported
            // Set the leader slot one slot down
            poh_recorder.reset(bank.last_blockhash(), 2, Some((4, 4)));

            // send ticks for a slot
            for _ in 0..bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // We are not the leader yet, as expected
            assert_eq!(poh_recorder.reached_leader_slot().0, false);
            poh_recorder.reset(bank.last_blockhash(), 3, Some((4, 4)));

            // without sending more ticks, we should be leader now
            let (reached_leader_slot, grace_ticks, leader_slot, ..) =
                poh_recorder.reached_leader_slot();
            assert_eq!(reached_leader_slot, true);
            assert_eq!(grace_ticks, 0);
            assert_eq!(leader_slot, 4);

            // Let's test that if a node overshoots the ticks for its target
            // leader slot, reached_leader_slot() will return true, because it's overdue
            // Set the leader slot one slot down
            poh_recorder.reset(bank.last_blockhash(), 4, Some((5, 5)));

            // Overshoot ticks for the slot
            let overshoot_factor = 4;
            for _ in 0..overshoot_factor * bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // We are overdue to lead
            let (reached_leader_slot, grace_ticks, leader_slot, ..) =
                poh_recorder.reached_leader_slot();
            assert_eq!(reached_leader_slot, true);
            assert_eq!(grace_ticks, overshoot_factor * bank.ticks_per_slot());
            assert_eq!(leader_slot, 9);
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_would_be_leader_soon() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            // Test that with no leader slot, we don't reach the leader tick
            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            poh_recorder.reset(bank.last_blockhash(), 0, None);

            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            // We reset with leader slot after 3 slots
            let bank_slot = bank.slot() + 3;
            poh_recorder.reset(bank.last_blockhash(), 0, Some((bank_slot, bank_slot)));

            // Test that the node won't be leader in next 2 slots
            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            // Test that the node will be leader in next 3 slots
            assert_eq!(
                poh_recorder.would_be_leader(3 * bank.ticks_per_slot()),
                true
            );

            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            // Move the bank up a slot (so that max_tick_height > slot 0's tick_height)
            let bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 1));
            // If we set the working bank, the node should be leader within next 2 slots
            poh_recorder.set_bank(&bank);
            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                true
            );
        }
    }

    #[test]
    fn test_flush_virtual_ticks() {
        let ledger_path = get_tmp_ledger_path!();
        {
            // test that virtual ticks are flushed into a newly set bank asap
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to be able to open database ledger");
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(2);
            let bank = Arc::new(Bank::new(&genesis_config));
            let genesis_hash = bank.last_blockhash();

            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                bank.last_blockhash(),
                0,
                Some((2, 2)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            //create a new bank
            let bank = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 2));
            //put 2 slots worth of virtual ticks into poh
            for _ in 0..(bank.ticks_per_slot() * 2) {
                poh_recorder.tick();
            }
            poh_recorder.set_bank(&bank);
            assert_eq!(Some(false), bank.check_hash_age(&genesis_hash, 1));
        }
    }

    #[test]
    fn test_compute_leader_slot_tick_heights() {
        assert_eq!(
            PohRecorder::compute_leader_slot_tick_heights(None, 0),
            (None, 0, 0)
        );

        assert_eq!(
            PohRecorder::compute_leader_slot_tick_heights(Some((4, 4)), 8),
            (Some(37), 40, 4)
        );

        assert_eq!(
            PohRecorder::compute_leader_slot_tick_heights(Some((4, 7)), 8),
            (Some(49), 64, 2 * 8)
        );

        assert_eq!(
            PohRecorder::compute_leader_slot_tick_heights(Some((6, 7)), 8),
            (Some(57), 64, 8)
        );

        assert_eq!(
            PohRecorder::compute_leader_slot_tick_heights(Some((6, 7)), 4),
            (Some(29), 32, 4)
        );
    }
}
