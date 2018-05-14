//! The `request_stage` processes thin client Request messages.

use packet;
use packet::SharedPackets;
use recorder::Signal;
use request_processor::RequestProcessor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct RequestStage {
    pub thread_hdl: JoinHandle<()>,
    pub signal_receiver: Receiver<Signal>,
    pub blob_receiver: streamer::BlobReceiver,
    pub request_processor: Arc<RequestProcessor>,
}

impl RequestStage {
    pub fn new(
        request_processor: RequestProcessor,
        exit: Arc<AtomicBool>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        packet_recycler: packet::PacketRecycler,
        blob_recycler: packet::BlobRecycler,
    ) -> Self {
        let request_processor = Arc::new(request_processor);
        let request_processor_ = request_processor.clone();
        let (signal_sender, signal_receiver) = channel();
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = spawn(move || loop {
            let e = request_processor_.process_request_packets(
                &verified_receiver,
                &signal_sender,
                &blob_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
            }
        });
        RequestStage {
            thread_hdl,
            signal_receiver,
            blob_receiver,
            request_processor,
        }
    }
}

// TODO: When banking is pulled out of RequestStage, add this test back in.

//use bank::Bank;
//use entry::Entry;
//use event::Event;
//use hash::Hash;
//use record_stage::RecordStage;
//use recorder::Signal;
//use result::Result;
//use std::sync::mpsc::{channel, Sender};
//use std::sync::{Arc, Mutex};
//use std::time::Duration;
//
//#[cfg(test)]
//mod tests {
//    use bank::Bank;
//    use event::Event;
//    use event_processor::EventProcessor;
//    use mint::Mint;
//    use signature::{KeyPair, KeyPairUtil};
//    use transaction::Transaction;
//
//    #[test]
//    // TODO: Move this test banking_stage. Calling process_events() directly
//    // defeats the purpose of this test.
//    fn test_banking_sequential_consistency() {
//        // In this attack we'll demonstrate that a verifier can interpret the ledger
//        // differently if either the server doesn't signal the ledger to add an
//        // Entry OR if the verifier tries to parallelize across multiple Entries.
//        let mint = Mint::new(2);
//        let bank = Bank::new(&mint);
//        let event_processor = EventProcessor::new(bank, &mint.last_id(), None);
//
//        // Process a batch that includes a transaction that receives two tokens.
//        let alice = KeyPair::new();
//        let tr = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());
//        let events = vec![Event::Transaction(tr)];
//        let entry0 = event_processor.process_events(events).unwrap();
//
//        // Process a second batch that spends one of those tokens.
//        let tr = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
//        let events = vec![Event::Transaction(tr)];
//        let entry1 = event_processor.process_events(events).unwrap();
//
//        // Collect the ledger and feed it to a new bank.
//        let entries = vec![entry0, entry1];
//
//        // Assert the user holds one token, not two. If the server only output one
//        // entry, then the second transaction will be rejected, because it drives
//        // the account balance below zero before the credit is added.
//        let bank = Bank::new(&mint);
//        for entry in entries {
//            assert!(
//                bank
//                    .process_verified_events(entry.events)
//                    .into_iter()
//                    .all(|x| x.is_ok())
//            );
//        }
//        assert_eq!(bank.get_balance(&alice.pubkey()), Some(1));
//    }
//}
//
//#[cfg(all(feature = "unstable", test))]
//mod bench {
//    extern crate test;
//    use self::test::Bencher;
//    use bank::{Bank, MAX_ENTRY_IDS};
//    use bincode::serialize;
//    use event_processor::*;
//    use hash::hash;
//    use mint::Mint;
//    use rayon::prelude::*;
//    use signature::{KeyPair, KeyPairUtil};
//    use std::collections::HashSet;
//    use std::time::Instant;
//    use transaction::Transaction;
//
//    #[bench]
//    fn process_events_bench(_bencher: &mut Bencher) {
//        let mint = Mint::new(100_000_000);
//        let bank = Bank::new(&mint);
//        // Create transactions between unrelated parties.
//        let txs = 100_000;
//        let last_ids: Mutex<HashSet<Hash>> = Mutex::new(HashSet::new());
//        let transactions: Vec<_> = (0..txs)
//            .into_par_iter()
//            .map(|i| {
//                // Seed the 'to' account and a cell for its signature.
//                let dummy_id = i % (MAX_ENTRY_IDS as i32);
//                let last_id = hash(&serialize(&dummy_id).unwrap()); // Semi-unique hash
//                {
//                    let mut last_ids = last_ids.lock().unwrap();
//                    if !last_ids.contains(&last_id) {
//                        last_ids.insert(last_id);
//                        bank.register_entry_id(&last_id);
//                    }
//                }
//
//                // Seed the 'from' account.
//                let rando0 = KeyPair::new();
//                let tr = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
//                bank.process_verified_transaction(&tr).unwrap();
//
//                let rando1 = KeyPair::new();
//                let tr = Transaction::new(&rando0, rando1.pubkey(), 2, last_id);
//                bank.process_verified_transaction(&tr).unwrap();
//
//                // Finally, return a transaction that's unique
//                Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
//            })
//            .collect();
//
//        let events: Vec<_> = transactions
//            .into_iter()
//            .map(|tr| Event::Transaction(tr))
//            .collect();
//
//        let event_processor = EventProcessor::new(bank, &mint.last_id(), None);
//
//        let now = Instant::now();
//        assert!(event_processor.process_events(events).is_ok());
//        let duration = now.elapsed();
//        let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
//        let tps = txs as f64 / sec;
//
//        // Ensure that all transactions were successfully logged.
//        drop(event_processor.historian_input);
//        let entries: Vec<Entry> = event_processor.output.lock().unwrap().iter().collect();
//        assert_eq!(entries.len(), 1);
//        assert_eq!(entries[0].events.len(), txs as usize);
//
//        println!("{} tps", tps);
//    }
//}
