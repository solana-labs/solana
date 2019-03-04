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
use crate::entry::Entry;
use crate::poh::Poh;
use crate::result::{Error, Result};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::transaction::Transaction;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

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
    pub poh: Poh,
    tick_cache: Vec<(Entry, u64)>,
    working_bank: Option<WorkingBank>,
    sender: Sender<WorkingBankEntries>,
}

impl PohRecorder {
    pub fn clear_bank(&mut self) {
        self.working_bank = None;
    }

    pub fn hash(&mut self) {
        // TODO: amortize the cost of this lock by doing the loop in here for
        // some min amount of hashes
        self.poh.hash();
    }

    pub fn bank(&self) -> Option<Arc<Bank>> {
        self.working_bank.clone().map(|w| w.bank)
    }
    // synchronize PoH with a bank
    pub fn reset(&mut self, tick_height: u64, blockhash: Hash) {
        if self.poh.hash == blockhash {
            assert_eq!(self.poh.tick_height, tick_height);
            info!(
                "reset skipped for: {},{}",
                self.poh.hash, self.poh.tick_height
            );
            return;
        }
        let mut cache = vec![];
        info!(
            "reset poh from: {},{} to: {},{}",
            self.poh.hash, self.poh.tick_height, blockhash, tick_height,
        );
        std::mem::swap(&mut cache, &mut self.tick_cache);
        self.poh = Poh::new(blockhash, tick_height);
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
                "flush_cache: bank_id: {} tick_height: {} max: {} sending: {}",
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
            self.working_bank = None;
        }
        if e.is_err() {
            info!("WorkingBank::sender disconnected {:?}", e);
            //revert the cache, but clear the working bank
            self.working_bank = None;
        } else {
            //commit the flush
            let _ = self.tick_cache.drain(..cnt);
        }

        Ok(())
    }

    pub fn tick(&mut self) {
        let tick = self.generate_tick();
        trace!("tick {}", tick.1);
        self.tick_cache.push(tick);
        let _ = self.flush_cache(true);
    }

    pub fn record(&mut self, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        self.flush_cache(false)?;
        self.record_and_send_txs(mixin, txs)
    }

    /// A recorder to synchronize PoH with the following data structures
    /// * bank - the LastId's queue is updated on `tick` and `record` events
    /// * sender - the Entry channel that outputs to the ledger
    pub fn new(tick_height: u64, last_entry_hash: Hash) -> (Self, Receiver<WorkingBankEntries>) {
        let poh = Poh::new(last_entry_hash, tick_height);
        let (sender, receiver) = channel();
        (
            PohRecorder {
                poh,
                tick_cache: vec![],
                working_bank: None,
                sender,
            },
            receiver,
        )
    }

    fn record_and_send_txs(&mut self, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        let working_bank = self
            .working_bank
            .as_ref()
            .ok_or(Error::PohRecorderError(PohRecorderError::MaxHeightReached))?;
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
    use crate::test_tx::test_tx;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use std::sync::Arc;

    #[test]
    fn test_poh_recorder_no_zero_tick() {
        let prev_hash = Hash::default();
        let (mut poh_recorder, _entry_receiver) = PohRecorder::new(0, prev_hash);
        poh_recorder.tick();
        assert_eq!(poh_recorder.tick_cache.len(), 1);
        assert_eq!(poh_recorder.tick_cache[0].1, 1);
        assert_eq!(poh_recorder.poh.tick_height, 1);
    }

    #[test]
    fn test_poh_recorder_tick_height_is_last_tick() {
        let prev_hash = Hash::default();
        let (mut poh_recorder, _entry_receiver) = PohRecorder::new(0, prev_hash);
        poh_recorder.tick();
        poh_recorder.tick();
        assert_eq!(poh_recorder.tick_cache.len(), 2);
        assert_eq!(poh_recorder.tick_cache[1].1, 2);
        assert_eq!(poh_recorder.poh.tick_height, 2);
    }

    #[test]
    fn test_poh_recorder_reset_clears_cache() {
        let (mut poh_recorder, _entry_receiver) = PohRecorder::new(0, Hash::default());
        poh_recorder.tick();
        assert_eq!(poh_recorder.tick_cache.len(), 1);
        poh_recorder.reset(0, Hash::default());
        assert_eq!(poh_recorder.tick_cache.len(), 0);
    }

    #[test]
    fn test_poh_recorder_clear() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, _entry_receiver) = PohRecorder::new(0, prev_hash);

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

    #[test]
    fn test_poh_recorder_tick_sent_after_min() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

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

    #[test]
    fn test_poh_recorder_tick_sent_upto_and_including_max() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

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

    #[test]
    fn test_poh_recorder_record_to_early() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

        let working_bank = WorkingBank {
            bank,
            min_tick_height: 2,
            max_tick_height: 3,
        };
        poh_recorder.set_working_bank(working_bank);
        poh_recorder.tick();
        let tx = test_tx();
        let h1 = hash(b"hello world!");
        assert!(poh_recorder.record(h1, vec![tx.clone()]).is_err());
        assert!(entry_receiver.try_recv().is_err());
    }

    #[test]
    fn test_poh_recorder_record_at_min_passes() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

        let working_bank = WorkingBank {
            bank,
            min_tick_height: 1,
            max_tick_height: 2,
        };
        poh_recorder.set_working_bank(working_bank);
        poh_recorder.tick();
        assert_eq!(poh_recorder.tick_cache.len(), 1);
        assert_eq!(poh_recorder.poh.tick_height, 1);
        let tx = test_tx();
        let h1 = hash(b"hello world!");
        assert!(poh_recorder.record(h1, vec![tx.clone()]).is_ok());
        assert_eq!(poh_recorder.tick_cache.len(), 0);

        //tick in the cache + entry
        let (_b, e) = entry_receiver.recv().expect("recv 1");
        assert_eq!(e.len(), 1);
        assert!(e[0].0.is_tick());
        let (_b, e) = entry_receiver.recv().expect("recv 2");
        assert!(!e[0].0.is_tick());
    }

    #[test]
    fn test_poh_recorder_record_at_max_fails() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

        let working_bank = WorkingBank {
            bank,
            min_tick_height: 1,
            max_tick_height: 2,
        };
        poh_recorder.set_working_bank(working_bank);
        poh_recorder.tick();
        poh_recorder.tick();
        assert_eq!(poh_recorder.poh.tick_height, 2);
        let tx = test_tx();
        let h1 = hash(b"hello world!");
        assert!(poh_recorder.record(h1, vec![tx.clone()]).is_err());

        let (_bank, e) = entry_receiver.recv().expect("recv 1");
        assert_eq!(e.len(), 2);
        assert!(e[0].0.is_tick());
        assert!(e[1].0.is_tick());
    }

    #[test]
    fn test_poh_cache_on_disconnect() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_hash = bank.last_blockhash();
        let (mut poh_recorder, entry_receiver) = PohRecorder::new(0, prev_hash);

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
}
