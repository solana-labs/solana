//! The `accounting_stage` module implements the accounting stage of the TPU.

use accountant::Accountant;
use entry::Entry;
use event::Event;
use hash::Hash;
use historian::Historian;
use recorder::Signal;
use result::Result;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub struct AccountingStage {
    pub output: Mutex<Receiver<Entry>>,
    entry_sender: Mutex<Sender<Entry>>,
    pub accountant: Arc<Accountant>,
    historian_input: Mutex<Sender<Signal>>,
    historian: Mutex<Historian>,
}

impl AccountingStage {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(accountant: Accountant, start_hash: &Hash, ms_per_tick: Option<u64>) -> Self {
        let (historian_input, event_receiver) = channel();
        let historian = Historian::new(event_receiver, start_hash, ms_per_tick);
        let (entry_sender, output) = channel();
        AccountingStage {
            output: Mutex::new(output),
            entry_sender: Mutex::new(entry_sender),
            accountant: Arc::new(accountant),
            historian_input: Mutex::new(historian_input),
            historian: Mutex::new(historian),
        }
    }

    /// Process the transactions in parallel and then log the successful ones.
    pub fn process_events(&self, events: Vec<Event>) -> Result<()> {
        let historian = self.historian.lock().unwrap();
        let results = self.accountant.process_verified_events(events);
        let events = results.into_iter().filter_map(|x| x.ok()).collect();
        let sender = self.historian_input.lock().unwrap();
        sender.send(Signal::Events(events))?;

        // Wait for the historian to tag our Events with an ID and then register it.
        let entry = historian.output.lock().unwrap().recv()?;
        self.accountant.register_entry_id(&entry.id);
        self.entry_sender.lock().unwrap().send(entry)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use accountant::Accountant;
    use accounting_stage::AccountingStage;
    use entry::Entry;
    use event::Event;
    use mint::Mint;
    use signature::{KeyPair, KeyPairUtil};
    use transaction::Transaction;

    #[test]
    fn test_accounting_sequential_consistency() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let mint = Mint::new(2);
        let accountant = Accountant::new(&mint);
        let accounting_stage = AccountingStage::new(accountant, &mint.last_id(), None);

        // Process a batch that includes a transaction that receives two tokens.
        let alice = KeyPair::new();
        let tr = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());
        let events = vec![Event::Transaction(tr)];
        assert!(accounting_stage.process_events(events).is_ok());

        // Process a second batch that spends one of those tokens.
        let tr = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
        let events = vec![Event::Transaction(tr)];
        assert!(accounting_stage.process_events(events).is_ok());

        // Collect the ledger and feed it to a new accountant.
        drop(accounting_stage.entry_sender);
        let entries: Vec<Entry> = accounting_stage.output.lock().unwrap().iter().collect();

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
    use accounting_stage::*;
    use bincode::serialize;
    use hash::hash;
    use historian::Historian;
    use mint::Mint;
    use rayon::prelude::*;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::HashSet;
    use std::sync::mpsc::channel;
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

        let (input, event_receiver) = channel();
        let accounting_stage = AccountingStage::new(accountant, &mint.last_id(), None);

        let now = Instant::now();
        assert!(accounting_stage.process_events(events).is_ok());
        let duration = now.elapsed();
        let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
        let tps = txs as f64 / sec;

        // Ensure that all transactions were successfully logged.
        drop(accounting_stage.historian_input);
        let entries: Vec<Entry> = accounting_stage.output.lock().unwrap().iter().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].events.len(), txs as usize);

        println!("{} tps", tps);
    }
}
