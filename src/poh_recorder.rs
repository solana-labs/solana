//! The `poh_recorder` module provides an object for synchronizing with Proof of History.
//! It synchronizes PoH, bank's register_tick and the ledger
//!
use crate::entry::Entry;
use crate::poh::Poh;
use crate::result::{Error, Result};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::transaction::Transaction;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PohRecorderError {
    InvalidCallingObject,
    MaxHeightReached,
    MinHeightNotReached,
}

#[derive(Clone)]
pub struct WorkingBank {
    pub bank: Arc<Bank>,
    pub sender: Sender<Vec<Entry>>,
    pub min_tick_height: u64,
    pub max_tick_height: u64,
}

#[derive(Clone)]
pub struct PohRecorder {
    poh: Arc<Mutex<Poh>>,
    tick_cache: Arc<Mutex<Vec<Entry>>>,
}

impl PohRecorder {
    pub fn hash(&self) {
        // TODO: amortize the cost of this lock by doing the loop in here for
        // some min amount of hashes
        let mut poh = self.poh.lock().unwrap();

        poh.hash();
    }

    fn flush_cache(&self, working_bank: &WorkingBank) -> Result<()> {
        let mut cache = vec![];
        std::mem::swap(&mut cache, &mut self.tick_cache.lock().unwrap());
        if !cache.is_empty() {
            for t in &cache {
                working_bank.bank.register_tick(&t.id);
            }
            working_bank.sender.send(cache)?;
        }
        Ok(())
    }

    pub fn tick(&self, working_bank: &WorkingBank) -> Result<()> {
        // Register and send the entry out while holding the lock if the max PoH height
        // hasn't been reached.
        // This guarantees PoH order and Entry production and banks LastId queue is the same
        let mut poh = self.poh.lock().unwrap();

        Self::check_tick_height(&poh, working_bank).map_err(|e| {
            let tick = Self::generate_tick(&mut poh);
            self.tick_cache.lock().unwrap().push(tick);
            e
        })?;
                                                      ;
        self.flush_cache(working_bank)?;

        Self::register_and_send_tick(&mut *poh, working_bank)
    }

    pub fn record(
        &self,
        mixin: Hash,
        txs: Vec<Transaction>,
        working_bank: &WorkingBank,
    ) -> Result<()> {
        // Register and send the entry out while holding the lock.
        // This guarantees PoH order and Entry production and banks LastId queue is the same.
        let mut poh = self.poh.lock().unwrap();

        Self::check_tick_height(&poh, working_bank)?;
        self.flush_cache(working_bank)?;

        Self::record_and_send_txs(&mut *poh, mixin, txs, working_bank)
    }

    /// A recorder to synchronize PoH with the following data structures
    /// * bank - the LastId's queue is updated on `tick` and `record` events
    /// * sender - the Entry channel that outputs to the ledger
    pub fn new(tick_height: u64, last_entry_id: Hash) -> Self {
        let poh = Arc::new(Mutex::new(Poh::new(last_entry_id, tick_height)));
        let tick_cache = Arc::new(Mutex::new(vec![]));
        PohRecorder { poh, tick_cache }
    }

    fn check_tick_height(poh: &Poh, working_bank: &WorkingBank) -> Result<()> {
        if poh.tick_height < working_bank.min_tick_height {
            Err(Error::PohRecorderError(
                PohRecorderError::MinHeightNotReached,
            ))
        } else if poh.tick_height >= working_bank.max_tick_height {
            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
        } else {
            Ok(())
        }
    }

    fn record_and_send_txs(
        poh: &mut Poh,
        mixin: Hash,
        txs: Vec<Transaction>,
        working_bank: &WorkingBank,
    ) -> Result<()> {
        let entry = poh.record(mixin);
        assert!(!txs.is_empty(), "Entries without transactions are used to track real-time passing in the ledger and can only be generated with PohRecorder::tick function");
        let entry = Entry {
            tick_height: entry.tick_height,
            num_hashes: entry.num_hashes,
            id: entry.id,
            transactions: txs,
        };
        working_bank.sender.send(vec![entry])?;
        Ok(())
    }

    fn generate_tick(poh: &mut Poh) -> Entry {
        let tick = poh.tick();
        Entry {
            tick_height: tick.tick_height,
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: vec![],
        }
    }

    fn register_and_send_tick(poh: &mut Poh, working_bank: &WorkingBank) -> Result<()> {
        let tick = Self::generate_tick(poh);
        working_bank.bank.register_tick(&tick.id);
        working_bank.sender.send(vec![tick])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tx::test_tx;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    #[test]
    fn test_poh_recorder() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(0, prev_id);

        let working_bank = WorkingBank {
            bank,
            sender: entry_sender,
            min_tick_height: 0,
            max_tick_height: 2,
        };

        //send some data
        let h1 = hash(b"hello world!");
        let tx = test_tx();
        poh_recorder
            .record(h1, vec![tx.clone()], &working_bank)
            .unwrap();
        //get some events
        let e = entry_receiver.recv().unwrap();
        assert_eq!(e[0].tick_height, 0); // super weird case, but ok!

        poh_recorder.tick(&working_bank).unwrap();
        let e = entry_receiver.recv().unwrap();
        assert_eq!(e[0].tick_height, 1);

        poh_recorder.tick(&working_bank).unwrap();
        let e = entry_receiver.recv().unwrap();
        assert_eq!(e[0].tick_height, 2);

        // max tick height reached
        assert!(poh_recorder.tick(&working_bank).is_err());
        assert!(poh_recorder.record(h1, vec![tx], &working_bank).is_err());

        //make sure it handles channel close correctly
        drop(entry_receiver);
        assert!(poh_recorder.tick(&working_bank).is_err());
    }

    #[test]
    fn test_poh_recorder_tick_cache() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(0, prev_id);

        let working_bank = WorkingBank {
            bank,
            sender: entry_sender,
            min_tick_height: 1,
            max_tick_height: 2,
        };

        // tick should be cached
        assert!(poh_recorder.tick(&working_bank).is_err());
        assert!(entry_receiver.try_recv().is_err());

        // working_bank should be at the right height
        poh_recorder.tick(&working_bank).unwrap();

        let e = entry_receiver.recv().unwrap();
        assert_eq!(e[0].tick_height, 1);
        let e = entry_receiver.recv().unwrap();
        assert_eq!(e[0].tick_height, 2);
    }

    #[test]
    fn test_poh_recorder_tick_cache_old_working_bank() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let prev_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let poh_recorder = PohRecorder::new(0, prev_id);

        let working_bank = WorkingBank {
            bank,
            sender: entry_sender,
            min_tick_height: 1,
            max_tick_height: 1,
        };

        // tick should be cached
        assert_matches!(
            poh_recorder.tick(&working_bank),
            Err(Error::PohRecorderError(
                PohRecorderError::MinHeightNotReached
            ))
        );

        // working_bank should be past MaxHeight
        assert_matches!(
            poh_recorder.tick(&working_bank),
            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
        );
        assert_eq!(poh_recorder.tick_cache.lock().unwrap().len(), 2);

        assert!(entry_receiver.try_recv().is_err());
    }
}
