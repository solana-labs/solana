//! The `recorder` module provides an object for generating a Proof of History.
//! It records Transaction items on behalf of its users.

use entry::Entry;
use hash::{hash, Hash};
use ledger;
use std::time::{Duration, Instant};
use transaction::Transaction;

pub struct Recorder {
    last_hash: Hash,
    num_hashes: u64,
    num_ticks: u32,
}

impl Recorder {
    pub fn new(last_hash: Hash) -> Self {
        Recorder {
            last_hash,
            num_hashes: 0,
            num_ticks: 0,
        }
    }

    pub fn hash(&mut self) {
        self.last_hash = hash(&self.last_hash);
        self.num_hashes += 1;
    }

    pub fn record(&mut self, transactions: Vec<Transaction>) -> Vec<Entry> {
        ledger::next_entries_mut(&mut self.last_hash, &mut self.num_hashes, transactions)
    }

    pub fn tick(&mut self, start_time: Instant, tick_duration: Duration) -> Option<Entry> {
        if start_time.elapsed() > tick_duration * (self.num_ticks + 1) {
            // TODO: don't let this overflow u32
            self.num_ticks += 1;
            Some(Entry::new_mut(
                &mut self.last_hash,
                &mut self.num_hashes,
                vec![],
                false,
            ))
        } else {
            None
        }
    }
}
