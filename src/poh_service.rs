//! The `poh_service` module provides an object for generating a Proof of History.
//! It records Hashes items on behalf of its users. It continuously generates
//! new Hashes, only stopping to check if it has been sent a Hash to mix in
//! to the Poh.
//!
//! The returned Entry includes the mix-in request, the latest Poh Hash, and the
//! number of Hashes generated in the service since the last mix-in request.
//!
//! The resulting stream of Hashes represents ordered events in time.
//!
use bank::Bank;
use entry::Entry;
use hash::Hash;
use poh::Poh;
use result::Result;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use transaction::Transaction;

#[derive(Clone)]
pub struct PohService {
    poh: Arc<Mutex<Poh>>,
    bank: Arc<Bank>,
    sender: Sender<Vec<Entry>>,
}

impl PohService {
    /// A background thread that will continue tagging received Transaction messages and
    /// sending back Entry messages until either the receiver or sender channel is closed.
    /// if tick_duration is some, service will automatically produce entries every
    ///  `tick_duration`.
    pub fn new(bank: Arc<Bank>, sender: Sender<Vec<Entry>>) -> Self {
        let poh = Arc::new(Mutex::new(Poh::new(bank.last_id())));
        PohService { poh, bank, sender }
    }

    pub fn hash(&self) {
        // TODO: amortize the cost of this lock by doing the loop in here for
        // some min amount of hashes
        let mut poh = self.poh.lock().unwrap();
        poh.hash()
    }

    pub fn tick(&self) -> Result<()> {
        // Register and send the entry out while holding the lock.
        // This guarantees PoH order and Entry production and banks LastId queue is the same
        let mut poh = self.poh.lock().unwrap();
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

    pub fn record(&self, mixin: Hash, txs: Vec<Transaction>) -> Result<()> {
        // Register and send the entry out while holding the lock.
        // This guarantees PoH order and Entry production and banks LastId queue is the same.
        let mut poh = self.poh.lock().unwrap();
        let tick = poh.record(mixin);
        self.bank.register_entry_id(&tick.id);
        let entry = Entry {
            num_hashes: tick.num_hashes,
            id: tick.id,
            transactions: txs,
        };
        self.sender.send(vec![entry])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poh::verify;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_poh() {
        let poh_service = PohService::new(Hash::default(), false);

        let entry0 = poh_service.record(Hash::default());
        sleep(Duration::from_millis(1));
        let entry1 = poh_service.record(Hash::default());
        sleep(Duration::from_millis(1));
        let entry2 = poh_service.record(Hash::default());

        assert_eq!(entry0.num_hashes, 1);
        assert_eq!(entry1.num_hashes, 1);
        assert_eq!(entry2.num_hashes, 1);

        assert_eq!(poh_service.join().unwrap(), ());

        assert!(verify(Hash::default(), &[entry0, entry1, entry2]));
    }

    #[test]
    fn test_do_poh() {
        let poh_service = PohService::new(Hash::default(), true);

        sleep(Duration::from_millis(50));
        let entry = poh_service.tick();
        assert!(entry.num_hashes > 1);

        assert_eq!(poh_service.join().unwrap(), ());

        assert!(verify(Hash::default(), &vec![entry]));
    }
}
