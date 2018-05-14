//! The `event_processor` module implements the accounting stage of the TPU.

use accountant::Accountant;
use entry::Entry;
use event::Event;
use hash::Hash;
use historian::Historian;
use recorder::Signal;
use result::Result;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

pub struct EventProcessor {
    pub accountant: Arc<Accountant>,
    historian_input: Mutex<Sender<Signal>>,
    historian: Mutex<Historian>,
    pub start_hash: Hash,
    pub ms_per_tick: Option<u64>,
}

impl EventProcessor {
    /// Create a new stage of the TPU for event and transaction processing
    pub fn new(accountant: Accountant, start_hash: &Hash, ms_per_tick: Option<u64>) -> Self {
        let (historian_input, event_receiver) = channel();
        let historian = Historian::new(event_receiver, start_hash, ms_per_tick);
        EventProcessor {
            accountant: Arc::new(accountant),
            historian_input: Mutex::new(historian_input),
            historian: Mutex::new(historian),
            start_hash: *start_hash,
            ms_per_tick,
        }
    }

    /// Process the transactions in parallel and then log the successful ones.
    pub fn process_events(&self, events: Vec<Event>) -> Result<Entry> {
        let historian = self.historian.lock().unwrap();
        let results = self.accountant.process_verified_events(events);
        let events = results.into_iter().filter_map(|x| x.ok()).collect();
        let sender = self.historian_input.lock().unwrap();
        sender.send(Signal::Events(events))?;

        // Wait for the historian to tag our Events with an ID and then register it.
        let entry = historian.entry_receiver.recv()?;
        self.accountant.register_entry_id(&entry.id);
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use accountant::Accountant;
    use event::Event;
    use event_processor::EventProcessor;
    use mint::Mint;
    use signature::{KeyPair, KeyPairUtil};
    use transaction::Transaction;

    #[test]
    // TODO: Move this test accounting_stage. Calling process_events() directly
    // defeats the purpose of this test.
    fn test_accounting_sequential_consistency() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let mint = Mint::new(2);
        let accountant = Accountant::new(&mint);
        let event_processor = EventProcessor::new(accountant, &mint.last_id(), None);

        // Process a batch that includes a transaction that receives two tokens.
        let alice = KeyPair::new();
        let tr = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());
        let events = vec![Event::Transaction(tr)];
        let entry0 = event_processor.process_events(events).unwrap();

        // Process a second batch that spends one of those tokens.
        let tr = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
        let events = vec![Event::Transaction(tr)];
        let entry1 = event_processor.process_events(events).unwrap();

        // Collect the ledger and feed it to a new accountant.
        let entries = vec![entry0, entry1];

        // Assert the user holds one token, not two. If the server only output one
        // entry, then the second transaction will be rejected, because it drives
        // the account balance below zero before the credit is added.
        let accountant = Accountant::new(&mint);
        for entry in entries {
            assert!(
                accountant
                    .process_verified_events(entry.events)
                    .into_iter()
                    .all(|x| x.is_ok())
            );
        }
        assert_eq!(accountant.get_balance(&alice.pubkey()), Some(1));
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use accountant::{Accountant, MAX_ENTRY_IDS};
    use bincode::serialize;
    use event_processor::*;
    use hash::hash;
    use mint::Mint;
    use rayon::prelude::*;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::HashSet;
    use std::time::Instant;
    use transaction::Transaction;

    #[bench]
    fn process_events_bench(_bencher: &mut Bencher) {
        let mint = Mint::new(100_000_000);
        let accountant = Accountant::new(&mint);
        // Create transactions between unrelated parties.
        let txs = 100_000;
        let last_ids: Mutex<HashSet<Hash>> = Mutex::new(HashSet::new());
        let transactions: Vec<_> = (0..txs)
            .into_par_iter()
            .map(|i| {
                // Seed the 'to' account and a cell for its signature.
                let dummy_id = i % (MAX_ENTRY_IDS as i32);
                let last_id = hash(&serialize(&dummy_id).unwrap()); // Semi-unique hash
                {
                    let mut last_ids = last_ids.lock().unwrap();
                    if !last_ids.contains(&last_id) {
                        last_ids.insert(last_id);
                        accountant.register_entry_id(&last_id);
                    }
                }

                // Seed the 'from' account.
                let rando0 = KeyPair::new();
                let tr = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
                accountant.process_verified_transaction(&tr).unwrap();

                let rando1 = KeyPair::new();
                let tr = Transaction::new(&rando0, rando1.pubkey(), 2, last_id);
                accountant.process_verified_transaction(&tr).unwrap();

                // Finally, return a transaction that's unique
                Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
            })
            .collect();

        let events: Vec<_> = transactions
            .into_iter()
            .map(|tr| Event::Transaction(tr))
            .collect();

        let event_processor = EventProcessor::new(accountant, &mint.last_id(), None);

        let now = Instant::now();
        assert!(event_processor.process_events(events).is_ok());
        let duration = now.elapsed();
        let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
        let tps = txs as f64 / sec;

        // Ensure that all transactions were successfully logged.
        drop(event_processor.historian_input);
        let entries: Vec<Entry> = event_processor.output.lock().unwrap().iter().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].events.len(), txs as usize);

        println!("{} tps", tps);
    }
}
