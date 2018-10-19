//! The `poh_recorder` module provides an object for synchronizing with Proof of History.
//! It synchronizes PoH, bank's register_entry_id and the ledger
//!
use bank::Bank;
use entry::Entry;
use hash::Hash;
use poh::Poh;
use result::{Error, Result};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use transaction::Transaction;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PohRecorderError {
    MaxHeightReached,
}

#[derive(Clone)]
pub struct PohRecorder {
    poh: Arc<Mutex<Poh>>,
    bank: Arc<Bank>,
    sender: Sender<Vec<Entry>>,
    // TODO: whe extracting PoH generator into a separate standalone service,
    // use this field for checking timeouts when running as a validator, and as
    // a transmission guard when running as the leader.
    pub max_tick_height: Option<u64>,
}

impl PohRecorder {
    /// A recorder to synchronize PoH with the following data structures
    /// * bank - the LastId's queue is updated on `tick` and `record` events
    /// * sender - the Entry channel that outputs to the ledger
    pub fn new(
        bank: Arc<Bank>,
        sender: Sender<Vec<Entry>>,
        last_entry_id: Hash,
        tick_height: u64,
        max_tick_height: Option<u64>,
    ) -> Self {
        let poh = Arc::new(Mutex::new(Poh::new(last_entry_id, tick_height)));
        PohRecorder {
            poh,
            bank,
            sender,
            max_tick_height,
        }
    }

    pub fn hash(&self) -> Result<()> {
        // TODO: amortize the cost of this lock by doing the loop in here for
        // some min amount of hashes
        let mut poh = self.poh.lock().unwrap();
        if self.check_max_tick_height_reached(&*poh) {
            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
        } else {
            poh.hash();
            Ok(())
        }
    }

    pub fn tick(&mut self) -> Result<u64> {
        // Register and send the entry out while holding the lock if the max PoH height
        // hasn't been reached.
        // This guarantees PoH order and Entry production and banks LastId queue is the same
        let mut poh = self.poh.lock().unwrap();
        if self.check_max_tick_height_reached(&*poh) {
            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
        } else {
            self.register_and_send_tick(&mut *poh)?;
            Ok(poh.tick_height)
        }
    }

    pub fn record(&self, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        // Register and send the entry out while holding the lock.
        // This guarantees PoH order and Entry production and banks LastId queue is the same.
        let mut poh = self.poh.lock().unwrap();
        if self.check_max_tick_height_reached(&*poh) {
            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
        } else {
            self.record_and_send_txs(&mut *poh, mixin, txs)?;
            Ok(())
        }
    }

    fn check_max_tick_height_reached(&self, poh: &Poh) -> bool {
        if let Some(max_tick_height) = self.max_tick_height {
            poh.tick_height >= max_tick_height
        } else {
            false
        }
    }

    fn record_and_send_txs(&self, poh: &mut Poh, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        let tick = poh.record(mixin);
        assert!(!txs.is_empty(), "Entries without transactions are used to track real-time passing in the ledger and can only be generated with PohRecorder::tick function");
        let entry = Entry {
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: txs,
        };
        self.sender.send(vec![entry])?;
        Ok(())
    }

    fn register_and_send_tick(&self, poh: &mut Poh) -> Result<()> {
        let tick = poh.tick();
        self.bank.register_entry_id(&tick.id);
        let entry = Entry {
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: vec![],
        };
        self.sender.send(vec![entry])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hash::hash;
    use mint::Mint;
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use system_transaction::test_tx;

    #[test]
    fn test_poh() {
        let mint = Mint::new(1);
        let bank = Arc::new(Bank::new(&mint));
        let last_id = bank.last_id();
        let (entry_sender, entry_receiver) = channel();
        let mut poh_recorder = PohRecorder::new(bank, entry_sender, last_id, 0, None);

        //send some data
        let h1 = hash(b"hello world!");
        let tx = test_tx();
        assert!(poh_recorder.record(h1, vec![tx]).is_ok());
        assert!(poh_recorder.tick().is_ok());

        //get some events
        let _ = entry_receiver.recv().unwrap();
        let _ = entry_receiver.recv().unwrap();

        //make sure it handles channel close correctly
        drop(entry_receiver);
        assert!(poh_recorder.tick().is_err());
    }
}
