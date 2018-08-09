//! The `recorder` module provides an object for generating a Proof of History.
//! It records Transaction items on behalf of its users.

use entry::Entry;
use hash::{hash, Hash};
use ledger;
use std::time::{Duration, Instant};
use transaction::Transaction;

pub struct Recorder {
    last_hash: Hash,
    hash_count: u64,
    tick_count: u32,
}

impl Recorder {
    pub fn new(last_hash: Hash) -> Self {
        Recorder {
            last_hash,
            hash_count: 0,
            tick_count: 0,
        }
    }

    pub fn hash(&mut self) {
        self.last_hash = hash(&self.last_hash.as_ref());
        self.hash_count += 1;
    }

    pub fn record(&mut self, transactions: Vec<Transaction>) -> Vec<Entry> {
        ledger::next_entries_mut(&mut self.last_hash, &mut self.hash_count, transactions)
    }

    pub fn tick(&mut self, start_time: Instant, tick_duration: Duration) -> Option<Entry> {
        if start_time.elapsed() > tick_duration * (self.tick_count + 1) {
            // TODO: don't let this overflow u32
            self.tick_count += 1;
            Some(Entry::new_mut(
                &mut self.last_hash,
                &mut self.hash_count,
                vec![],
                false,
            ))
        } else {
            None
        }
    }
}
