//! The `poh_recorder` module provides an object for synchronizing with Proof of History.
//! It synchronizes PoH, bank's register_tick and the ledger
//!
//! PohRecorder will send ticks or entries to a WorkingBank, if the current range of ticks is
//! within the specified WorkingBank range.
//!
//! For Ticks:
//! * tick must be > WorkingBank::min_tick_height && tick must be <= WorkingBank::man_tick_height
//!
//! For Entries:
//! * recorded entry must be >= WorkingBank::min_tick_height && entry must be < WorkingBank::man_tick_height
//!
use crate::blocktree::Blocktree;
use crate::entry::Entry;
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::leader_schedule_utils;
use crate::poh::Poh;
use crate::result::{Error, Result};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::sync::mpsc::{channel, Receiver, Sender, SyncSender};
use std::sync::Arc;

const MAX_LAST_LEADER_GRACE_TICKS_FACTOR: u64 = 2;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PohRecorderError {
    InvalidCallingObject,
    MaxHeightReached,
    MinHeightNotReached,
}

pub type WorkingBankEntries = (Arc<Bank>, Vec<(Entry, u64)>);

#[derive(Clone)]
pub struct WorkingBank {
    pub bank: Arc<Bank>,
    pub min_tick_height: u64,
    pub max_tick_height: u64,
}

pub struct PohRecorder {
    poh: Poh,
    clear_bank_signal: Option<SyncSender<bool>>,
    start_slot: u64,
    start_tick: u64,
    tick_cache: Vec<(Entry, u64)>,
    working_bank: Option<WorkingBank>,
    sender: Sender<WorkingBankEntries>,
    start_leader_at_tick: Option<u64>,
    last_leader_tick: Option<u64>,
    max_last_leader_grace_ticks: u64,
    id: Pubkey,
    blocktree: Arc<Blocktree>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
}

impl PohRecorder {
    fn clear_bank(&mut self) {
        if let Some(working_bank) = self.working_bank.take() {
            let bank = working_bank.bank;
            let next_leader_slot = self.leader_schedule_cache.next_leader_slot(
                &self.id,
                bank.slot(),
                &bank,
                Some(&self.blocktree),
            );
            let (start_leader_at_tick, last_leader_tick) = Self::compute_leader_slot_ticks(
                &next_leader_slot,
                bank.ticks_per_slot(),
                self.max_last_leader_grace_ticks,
            );
            self.start_leader_at_tick = start_leader_at_tick;
            self.last_leader_tick = last_leader_tick;
        }
        if let Some(ref signal) = self.clear_bank_signal {
            let _ = signal.try_send(true);
        }
    }

    pub fn would_be_leader(&self, within_next_n_ticks: u64) -> bool {
        let close_to_leader_tick = self.start_leader_at_tick.map_or(false, |leader_tick| {
            let leader_ideal_start_tick =
                leader_tick.saturating_sub(self.max_last_leader_grace_ticks);

            self.tick_height() <= self.last_leader_tick.unwrap_or(0)
                && self.tick_height() >= leader_ideal_start_tick.saturating_sub(within_next_n_ticks)
        });

        self.working_bank.is_some() || close_to_leader_tick
    }

    pub fn next_slot_leader(&self, ticks_per_slot: u64, bank: Option<&Bank>) -> Option<Pubkey> {
        let slot = leader_schedule_utils::tick_height_to_slot(ticks_per_slot, self.tick_height());
        self.leader_schedule_cache.slot_leader_at(slot + 1, bank)
    }

    pub fn hash(&mut self) {
        // TODO: amortize the cost of this lock by doing the loop in here for
        // some min amount of hashes
        self.poh.hash();
    }

    pub fn start_slot(&self) -> u64 {
        self.start_slot
    }

    pub fn bank(&self) -> Option<Arc<Bank>> {
        self.working_bank.clone().map(|w| w.bank)
    }

    pub fn tick_height(&self) -> u64 {
        self.poh.tick_height
    }

    // returns if leader tick has reached, and how many grace ticks were afforded
    pub fn reached_leader_tick(&self) -> (bool, u64) {
        self.start_leader_at_tick
            .map(|target_tick| {
                debug!(
                    "Current tick {}, start tick {} target {}, grace {}",
                    self.tick_height(),
                    self.start_tick,
                    target_tick,
                    self.max_last_leader_grace_ticks
                );

                let leader_ideal_start_tick =
                    target_tick.saturating_sub(self.max_last_leader_grace_ticks);
                // Is the current tick in the same slot as the target tick?
                // Check if either grace period has expired,
                // or target tick is = grace period (i.e. poh recorder was just reset)
                if self.tick_height() <= self.last_leader_tick.unwrap_or(0)
                    && (self.tick_height() >= target_tick
                        || self.max_last_leader_grace_ticks
                            >= target_tick.saturating_sub(self.start_tick))
                {
                    return (
                        true,
                        self.tick_height().saturating_sub(leader_ideal_start_tick),
                    );
                }

                (false, 0)
            })
            .unwrap_or((false, 0))
    }

    fn compute_leader_slot_ticks(
        next_leader_slot: &Option<u64>,
        ticks_per_slot: u64,
        grace_ticks: u64,
    ) -> (Option<u64>, Option<u64>) {
        next_leader_slot
            .map(|slot| {
                (
                    Some(slot * ticks_per_slot + grace_ticks),
                    Some((slot + 1) * ticks_per_slot - 1),
                )
            })
            .unwrap_or((None, None))
    }

    // synchronize PoH with a bank
    pub fn reset(
        &mut self,
        tick_height: u64,
        blockhash: Hash,
        start_slot: u64,
        my_next_leader_slot: Option<u64>,
        ticks_per_slot: u64,
    ) {
        self.clear_bank();
        let mut cache = vec![];
        info!(
            "reset poh from: {},{} to: {},{}",
            self.poh.hash, self.poh.tick_height, blockhash, tick_height,
        );
        std::mem::swap(&mut cache, &mut self.tick_cache);
        self.start_slot = start_slot;
        self.start_tick = tick_height + 1;
        self.poh = Poh::new(blockhash, tick_height);
        self.max_last_leader_grace_ticks = ticks_per_slot / MAX_LAST_LEADER_GRACE_TICKS_FACTOR;
        let (start_leader_at_tick, last_leader_tick) = Self::compute_leader_slot_ticks(
            &my_next_leader_slot,
            ticks_per_slot,
            self.max_last_leader_grace_ticks,
        );
        self.start_leader_at_tick = start_leader_at_tick;
        self.last_leader_tick = last_leader_tick;
    }

    pub fn set_working_bank(&mut self, working_bank: WorkingBank) {
        trace!("new working bank");
        self.working_bank = Some(working_bank);
    }
    pub fn set_bank(&mut self, bank: &Arc<Bank>) {
        let max_tick_height = (bank.slot() + 1) * bank.ticks_per_slot() - 1;
        let working_bank = WorkingBank {
            bank: bank.clone(),
            min_tick_height: bank.tick_height(),
            max_tick_height,
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
            .ok_or(Error::PohRecorderError(PohRecorderError::MaxHeightReached))?;
        if self.poh.tick_height < working_bank.min_tick_height {
            return Err(Error::PohRecorderError(
                PohRecorderError::MinHeightNotReached,
            ));
        }
        if tick && self.poh.tick_height == working_bank.min_tick_height {
            return Err(Error::PohRecorderError(
                PohRecorderError::MinHeightNotReached,
            ));
        }

        let cnt = self
            .tick_cache
            .iter()
            .take_while(|x| x.1 <= working_bank.max_tick_height)
            .count();
        let e = if cnt > 0 {
            debug!(
                "flush_cache: bank_slot: {} tick_height: {} max: {} sending: {}",
                working_bank.bank.slot(),
                working_bank.bank.tick_height(),
                working_bank.max_tick_height,
                cnt,
            );
            let cache = &self.tick_cache[..cnt];
            for t in cache {
                working_bank.bank.register_tick(&t.0.hash);
            }
            self.sender
                .send((working_bank.bank.clone(), cache.to_vec()))
        } else {
            Ok(())
        };
        if self.poh.tick_height >= working_bank.max_tick_height {
            info!(
                "poh_record: max_tick_height reached, setting working bank {} to None",
                working_bank.bank.slot()
            );
            self.start_slot = working_bank.max_tick_height / working_bank.bank.ticks_per_slot();
            self.start_tick = (self.start_slot + 1) * working_bank.bank.ticks_per_slot();
            self.clear_bank();
        }
        if e.is_err() {
            info!("WorkingBank::sender disconnected {:?}", e);
            //revert the cache, but clear the working bank
            self.clear_bank();
        } else {
            //commit the flush
            let _ = self.tick_cache.drain(..cnt);
        }

        Ok(())
    }

    pub fn tick(&mut self) {
        let tick = self.generate_tick();
        trace!("tick {}", tick.1);

        if self.start_leader_at_tick.is_none() {
            return;
        }

        self.tick_cache.push(tick);
        let _ = self.flush_cache(true);
    }

    pub fn record(&mut self, bank_slot: u64, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        self.flush_cache(false)?;
        self.record_and_send_txs(bank_slot, mixin, txs)
    }

    pub fn new_with_clear_signal(
        tick_height: u64,
        last_entry_hash: Hash,
        start_slot: u64,
        my_leader_slot_index: Option<u64>,
        ticks_per_slot: u64,
        id: &Pubkey,
        blocktree: &Arc<Blocktree>,
        clear_bank_signal: Option<SyncSender<bool>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) -> (Self, Receiver<WorkingBankEntries>) {
        let poh = Poh::new(last_entry_hash, tick_height);
        let (sender, receiver) = channel();
        let max_last_leader_grace_ticks = ticks_per_slot / MAX_LAST_LEADER_GRACE_TICKS_FACTOR;
        let (start_leader_at_tick, last_leader_tick) = Self::compute_leader_slot_ticks(
            &my_leader_slot_index,
            ticks_per_slot,
            max_last_leader_grace_ticks,
        );
        (
            Self {
                poh,
                tick_cache: vec![],
                working_bank: None,
                sender,
                clear_bank_signal,
                start_slot,
                start_tick: tick_height + 1,
                start_leader_at_tick,
                last_leader_tick,
                max_last_leader_grace_ticks,
                id: *id,
                blocktree: blocktree.clone(),
                leader_schedule_cache: leader_schedule_cache.clone(),
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
        start_slot: u64,
        my_leader_slot_index: Option<u64>,
        ticks_per_slot: u64,
        id: &Pubkey,
        blocktree: &Arc<Blocktree>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) -> (Self, Receiver<WorkingBankEntries>) {
        Self::new_with_clear_signal(
            tick_height,
            last_entry_hash,
            start_slot,
            my_leader_slot_index,
            ticks_per_slot,
            id,
            blocktree,
            None,
            leader_schedule_cache,
        )
    }

    fn record_and_send_txs(
        &mut self,
        bank_slot: u64,
        mixin: Hash,
        txs: Vec<Transaction>,
    ) -> Result<()> {
        let working_bank = self
            .working_bank
            .as_ref()
            .ok_or(Error::PohRecorderError(PohRecorderError::MaxHeightReached))?;
        if bank_slot != working_bank.bank.slot() {
            return Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached));
        }
        let poh_entry = self.poh.record(mixin);
        assert!(!txs.is_empty(), "Entries without transactions are used to track real-time passing in the ledger and can only be generated with PohRecorder::tick function");
        let recorded_entry = Entry {
            num_hashes: poh_entry.num_hashes,
            hash: poh_entry.hash,
            transactions: txs,
        };
        trace!("sending entry {}", recorded_entry.is_tick());
        self.sender.send((
            working_bank.bank.clone(),
            vec![(recorded_entry, poh_entry.tick_height)],
        ))?;
        Ok(())
    }

    fn generate_tick(&mut self) -> (Entry, u64) {
        let tick = self.poh.tick();
        assert_ne!(tick.tick_height, 0);
        (
            Entry {
                num_hashes: tick.num_hashes,
                hash: tick.hash,
                transactions: vec![],
            },
            tick.tick_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};
    use crate::leader_schedule_cache::LeaderScheduleCache;
    use crate::test_tx::test_tx;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
    use std::sync::mpsc::sync_channel;
    use std::sync::Arc;

    #[test]
    fn test_poh_recorder_no_zero_tick() {
        let prev_hash = Hash::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");

            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.tick_cache[0].1, 1);
            assert_eq!(poh_recorder.poh.tick_height, 1);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_height_is_last_tick() {
        let prev_hash = Hash::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");

            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            assert_eq!(poh_recorder.tick_cache[1].1, 2);
            assert_eq!(poh_recorder.poh.tick_height, 2);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_reset_clears_cache() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            poh_recorder.reset(0, Hash::default(), 0, Some(4), DEFAULT_TICKS_PER_SLOT);
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_clear() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
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
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_sent_after_min() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
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
            assert_eq!(poh_recorder.poh.tick_height, 3);
            assert_eq!(poh_recorder.tick_cache.len(), 0);
            let (bank_, e) = entry_receiver.recv().expect("recv 1");
            assert_eq!(e.len(), 3);
            assert_eq!(bank_.slot(), bank.slot());
            assert!(poh_recorder.working_bank.is_none());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_tick_sent_upto_and_including_max() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.last().unwrap().1, 4);
            assert_eq!(poh_recorder.poh.tick_height, 4);

            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();

            assert_eq!(poh_recorder.poh.tick_height, 5);
            assert!(poh_recorder.working_bank.is_none());
            let (_, e) = entry_receiver.recv().expect("recv 1");
            assert_eq!(e.len(), 3);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_to_early() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
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
            assert!(poh_recorder
                .record(bank.slot(), h1, vec![tx.clone()])
                .is_err());
            assert!(entry_receiver.try_recv().is_err());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_bad_slot() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.poh.tick_height, 1);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert_matches!(
                poh_recorder.record(bank.slot() + 1, h1, vec![tx.clone()]),
                Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
            );
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_at_min_passes() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 1);
            assert_eq!(poh_recorder.poh.tick_height, 1);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder
                .record(bank.slot(), h1, vec![tx.clone()])
                .is_ok());
            assert_eq!(poh_recorder.tick_cache.len(), 0);

            //tick in the cache + entry
            let (_b, e) = entry_receiver.recv().expect("recv 1");
            assert_eq!(e.len(), 1);
            assert!(e[0].0.is_tick());
            let (_b, e) = entry_receiver.recv().expect("recv 2");
            assert!(!e[0].0.is_tick());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_record_at_max_fails() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            let working_bank = WorkingBank {
                bank: bank.clone(),
                min_tick_height: 1,
                max_tick_height: 2,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.poh.tick_height, 2);
            let tx = test_tx();
            let h1 = hash(b"hello world!");
            assert!(poh_recorder
                .record(bank.slot(), h1, vec![tx.clone()])
                .is_err());

            let (_bank, e) = entry_receiver.recv().expect("recv 1");
            assert_eq!(e.len(), 2);
            assert!(e[0].0.is_tick());
            assert!(e[1].0.is_tick());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_cache_on_disconnect() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.poh.tick_height, 2);
            drop(entry_receiver);
            poh_recorder.tick();
            assert!(poh_recorder.working_bank.is_none());
            assert_eq!(poh_recorder.tick_cache.len(), 3);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_current() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            poh_recorder.reset(
                poh_recorder.poh.tick_height,
                poh_recorder.poh.hash,
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
            );
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_with_cached() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 2);
            poh_recorder.reset(
                poh_recorder.tick_cache[0].1,
                poh_recorder.tick_cache[0].0.hash,
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
            );
            assert_eq!(poh_recorder.tick_cache.len(), 0);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_to_new_value() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some(4),
                DEFAULT_TICKS_PER_SLOT,
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.tick();
            poh_recorder.tick();
            poh_recorder.tick();
            assert_eq!(poh_recorder.tick_cache.len(), 3);
            assert_eq!(poh_recorder.poh.tick_height, 3);
            poh_recorder.reset(1, hash(b"hello"), 0, Some(4), DEFAULT_TICKS_PER_SLOT);
            assert_eq!(poh_recorder.tick_cache.len(), 0);
            poh_recorder.tick();
            assert_eq!(poh_recorder.poh.tick_height, 2);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reset_clear_bank() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                Hash::default(),
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );
            let ticks_per_slot = bank.ticks_per_slot();
            let working_bank = WorkingBank {
                bank,
                min_tick_height: 2,
                max_tick_height: 3,
            };
            poh_recorder.set_working_bank(working_bank);
            poh_recorder.reset(1, hash(b"hello"), 0, Some(4), ticks_per_slot);
            assert!(poh_recorder.working_bank.is_none());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    pub fn test_clear_signal() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let (sender, receiver) = sync_channel(1);
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new_with_clear_signal(
                0,
                Hash::default(),
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                Some(sender),
                &Arc::new(LeaderScheduleCache::default()),
            );
            poh_recorder.set_bank(&bank);
            poh_recorder.clear_bank();
            assert!(receiver.try_recv().is_ok());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_poh_recorder_reset_start_slot() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let ticks_per_slot = 5;
            let (mut genesis_block, _mint_keypair) = GenesisBlock::new(2);
            genesis_block.ticks_per_slot = ticks_per_slot;
            let bank = Arc::new(Bank::new(&genesis_block));

            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                Some(4),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            let end_slot = 3;
            let max_tick_height = (end_slot + 1) * ticks_per_slot - 1;
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
            assert!(poh_recorder
                .record(bank.slot(), h1, vec![tx.clone()])
                .is_err());
            assert!(poh_recorder.working_bank.is_none());
            // Make sure the starting slot is updated
            assert_eq!(poh_recorder.start_slot(), end_slot);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_reached_leader_tick() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            // Test that with no leader slot, we don't reach the leader tick
            assert_eq!(poh_recorder.reached_leader_tick().0, false);

            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                0,
                None,
                bank.ticks_per_slot(),
            );

            // Test that with no leader slot in reset(), we don't reach the leader tick
            assert_eq!(poh_recorder.reached_leader_tick().0, false);

            // Provide a leader slot 1 slot down
            poh_recorder.reset(
                bank.ticks_per_slot(),
                bank.last_blockhash(),
                0,
                Some(2),
                bank.ticks_per_slot(),
            );

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

            // Test that we don't reach the leader tick because of grace ticks
            assert_eq!(poh_recorder.reached_leader_tick().0, false);

            // reset poh now. it should discard the grace ticks wait
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                1,
                Some(2),
                bank.ticks_per_slot(),
            );
            // without sending more ticks, we should be leader now
            assert_eq!(poh_recorder.reached_leader_tick().0, true);
            assert_eq!(poh_recorder.reached_leader_tick().1, 0);

            // Now test that with grace ticks we can reach leader ticks
            // Set the leader slot 1 slot down
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                2,
                Some(3),
                bank.ticks_per_slot(),
            );

            // Send one slot worth of ticks
            for _ in 0..bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // We are not the leader yet, as expected
            assert_eq!(poh_recorder.reached_leader_tick().0, false);

            // Send 1 less tick than the grace ticks
            for _ in 0..bank.ticks_per_slot() / MAX_LAST_LEADER_GRACE_TICKS_FACTOR - 1 {
                poh_recorder.tick();
            }
            // We are still not the leader
            assert_eq!(poh_recorder.reached_leader_tick().0, false);

            // Send one more tick
            poh_recorder.tick();

            // We should be the leader now
            assert_eq!(poh_recorder.reached_leader_tick().0, true);
            assert_eq!(
                poh_recorder.reached_leader_tick().1,
                bank.ticks_per_slot() / MAX_LAST_LEADER_GRACE_TICKS_FACTOR
            );

            // Let's test that correct grace ticks are reported
            // Set the leader slot 1 slot down
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                3,
                Some(4),
                bank.ticks_per_slot(),
            );

            // Send remaining ticks for the slot (remember we sent extra ticks in the previous part of the test)
            for _ in
                bank.ticks_per_slot() / MAX_LAST_LEADER_GRACE_TICKS_FACTOR..bank.ticks_per_slot()
            {
                poh_recorder.tick();
            }

            // Send one extra tick before resetting (so that there's one grace tick)
            poh_recorder.tick();

            // We are not the leader yet, as expected
            assert_eq!(poh_recorder.reached_leader_tick().0, false);
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                3,
                Some(4),
                bank.ticks_per_slot(),
            );
            // without sending more ticks, we should be leader now
            assert_eq!(poh_recorder.reached_leader_tick().0, true);
            assert_eq!(poh_recorder.reached_leader_tick().1, 1);

            // Let's test that if a node overshoots the ticks for its target
            // leader slot, reached_leader_tick() will return false
            // Set the leader slot 1 slot down
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                4,
                Some(5),
                bank.ticks_per_slot(),
            );

            // Send remaining ticks for the slot (remember we sent extra ticks in the previous part of the test)
            for _ in 0..4 * bank.ticks_per_slot() {
                poh_recorder.tick();
            }

            // We are not the leader, as expected
            assert_eq!(poh_recorder.reached_leader_tick().0, false);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_would_be_leader_soon() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
            let bank = Arc::new(Bank::new(&genesis_block));
            let prev_hash = bank.last_blockhash();
            let (mut poh_recorder, _entry_receiver) = PohRecorder::new(
                0,
                prev_hash,
                0,
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            );

            // Test that with no leader slot, we don't reach the leader tick
            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                0,
                None,
                bank.ticks_per_slot(),
            );

            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                false
            );

            // We reset with leader slot after 3 slots
            poh_recorder.reset(
                poh_recorder.tick_height(),
                bank.last_blockhash(),
                0,
                Some(bank.slot() + 3),
                bank.ticks_per_slot(),
            );

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

            // If we set the working bank, the node should be leader within next 2 slots
            poh_recorder.set_bank(&bank);
            assert_eq!(
                poh_recorder.would_be_leader(2 * bank.ticks_per_slot()),
                true
            );
        }
    }
}
