//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::Bank;
use bincode::deserialize;
use counter::Counter;
use packet;
use packet::SharedPackets;
use rayon::prelude::*;
use record_stage::Signal;
use result::Result;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    /// Handle to the stage's thread.
    pub thread_hdl: JoinHandle<()>,

    /// Output receiver for the following stage.
    pub signal_receiver: Receiver<Signal>,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when either `exit` is set or
    /// when `verified_receiver` or the stage's output receiver is dropped.
    /// Discard input packets using `packet_recycler` to minimize memory
    /// allocations in a previous stage such as the `fetch_stage`.
    pub fn new(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        packet_recycler: packet::PacketRecycler,
    ) -> Self {
        let (signal_sender, signal_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-banking-stage".to_string())
            .spawn(move || loop {
                let e = Self::process_packets(
                    bank.clone(),
                    &verified_receiver,
                    &signal_sender,
                    &packet_recycler,
                );
                if e.is_err() {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                }
            })
            .unwrap();
        BankingStage {
            thread_hdl,
            signal_receiver,
        }
    }

    /// Convert the transactions from a blob of binary data to a vector of transactions and
    /// an unused `SocketAddr` that could be used to send a response.
    fn deserialize_transactions(p: &packet::Packets) -> Vec<Option<(Transaction, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            })
            .collect()
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    fn process_packets(
        bank: Arc<Bank>,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        signal_sender: &Sender<Signal>,
        packet_recycler: &packet::PacketRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let recv_start = Instant::now();
        let mms = verified_receiver.recv_timeout(timer)?;
        let mut reqs_len = 0;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            mms.len(),
        );
        let count = mms.iter().map(|x| x.1.len()).sum();
        static mut COUNTER: Counter = create_counter!("banking_stage_process_packets", 1);
        let proc_start = Instant::now();
        for (msgs, vers) in mms {
            let transactions = Self::deserialize_transactions(&msgs.read().unwrap());
            reqs_len += transactions.len();
            let transactions = transactions
                .into_iter()
                .zip(vers)
                .filter_map(|(tx, ver)| match tx {
                    None => None,
                    Some((tx, _addr)) => if tx.verify_plan() && ver != 0 {
                        Some(tx)
                    } else {
                        None
                    },
                })
                .collect();

            debug!("process_transactions");
            let results = bank.process_transactions(transactions);
            let transactions = results.into_iter().filter_map(|x| x.ok()).collect();
            signal_sender.send(Signal::Transactions(transactions))?;
            debug!("done process_transactions");

            packet_recycler.recycle(msgs);
        }
        let total_time_s = timing::duration_as_s(&proc_start.elapsed());
        let total_time_ms = timing::duration_as_ms(&proc_start.elapsed());
        info!(
            "@{:?} done processing transaction batches: {} time: {:?}ms reqs: {} reqs/s: {}",
            timing::timestamp(),
            mms_len,
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        inc_counter!(COUNTER, count, proc_start);
        Ok(())
    }
}

// TODO: When banking is pulled out of RequestStage, add this test back in.

//use bank::Bank;
//use entry::Entry;
//use hash::Hash;
//use record_stage::RecordStage;
//use record_stage::Signal;
//use result::Result;
//use std::sync::mpsc::{channel, Sender};
//use std::sync::{Arc, Mutex};
//use std::time::Duration;
//use transaction::Transaction;
//
//#[cfg(test)]
//mod tests {
//    use bank::Bank;
//    use mint::Mint;
//    use signature::{KeyPair, KeyPairUtil};
//    use transaction::Transaction;
//
//    #[test]
//    // TODO: Move this test banking_stage. Calling process_transactions() directly
//    // defeats the purpose of this test.
//    fn test_banking_sequential_consistency() {
//        // In this attack we'll demonstrate that a verifier can interpret the ledger
//        // differently if either the server doesn't signal the ledger to add an
//        // Entry OR if the verifier tries to parallelize across multiple Entries.
//        let mint = Mint::new(2);
//        let bank = Bank::new(&mint);
//        let banking_stage = EventProcessor::new(bank, &mint.last_id(), None);
//
//        // Process a batch that includes a transaction that receives two tokens.
//        let alice = KeyPair::new();
//        let tx = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());
//        let transactions = vec![tx];
//        let entry0 = banking_stage.process_transactions(transactions).unwrap();
//
//        // Process a second batch that spends one of those tokens.
//        let tx = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
//        let transactions = vec![tx];
//        let entry1 = banking_stage.process_transactions(transactions).unwrap();
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
//                    .process_transactions(entry.transactions)
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
//    use hash::hash;
//    use mint::Mint;
//    use rayon::prelude::*;
//    use signature::{KeyPair, KeyPairUtil};
//    use std::collections::HashSet;
//    use std::time::Instant;
//    use transaction::Transaction;
//
//    #[bench]
//    fn bench_process_transactions(_bencher: &mut Bencher) {
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
//                let tx = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
//                bank.process_transaction(&tx).unwrap();
//
//                let rando1 = KeyPair::new();
//                let tx = Transaction::new(&rando0, rando1.pubkey(), 2, last_id);
//                bank.process_transaction(&tx).unwrap();
//
//                // Finally, return a transaction that's unique
//                Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
//            })
//            .collect();
//
//        let banking_stage = EventProcessor::new(bank, &mint.last_id(), None);
//
//        let now = Instant::now();
//        assert!(banking_stage.process_transactions(transactions).is_ok());
//        let duration = now.elapsed();
//        let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
//        let tps = txs as f64 / sec;
//
//        // Ensure that all transactions were successfully logged.
//        drop(banking_stage.historian_input);
//        let entries: Vec<Entry> = banking_stage.output.lock().unwrap().iter().collect();
//        assert_eq!(entries.len(), 1);
//        assert_eq!(entries[0].transactions.len(), txs as usize);
//
//        println!("{} tps", tps);
//    }
//}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use bank::*;
    use banking_stage::BankingStage;
    use logger;
    use mint::Mint;
    use packet::{to_packets_chunked, PacketRecycler};
    use record_stage::Signal;
    use signature::{KeyPair, KeyPairUtil};
    use std::iter;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::Arc;
    use transaction::Transaction;
    use rayon::prelude::*;

    fn check_txs(batches: usize,
                 receiver: &Receiver<Signal>,
                 ref_tx_count: usize) {
        let mut total = 0;
        for _ in 0..batches {
            let signal = receiver.recv().unwrap();
            if let Signal::Transactions(transactions) = signal {
                total += transactions.len();
            } else {
                assert!(false);
            }
        }
        assert_eq!(total, ref_tx_count);
    }

    #[bench]
    fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
        logger::setup();
        let tx = 30_000_usize;
        let mint_total = 1_000_000_000_000;
        let mint = Mint::new(mint_total);
        let num_dst_accounts = 8 * 1024;
        let num_src_accounts = 8 * 1024;

        let srckeys: Vec<_> = (0..num_src_accounts).map(|_| { KeyPair::new() }).collect();
        let dstkeys: Vec<_> = (0..num_dst_accounts).map(|_| { KeyPair::new().pubkey() }).collect();

        info!("created keys src: {} dst: {}", srckeys.len(), dstkeys.len());

        let transactions: Vec<_> = (0..tx)
            .map(|i| {
                Transaction::new(
                    &srckeys[i % num_src_accounts],
                    dstkeys[i % num_dst_accounts],
                    i as i64,
                    mint.last_id(),
                )
            })
            .collect();

        info!("created transactions");

        let (verified_sender, verified_receiver) = channel();
        let (signal_sender, signal_receiver) = channel();
        let packet_recycler = PacketRecycler::default();
        let verified: Vec<_> = to_packets_chunked(&packet_recycler, transactions, tx)
            .into_iter()
            .map(|x| {
                let len = (*x).read().unwrap().packets.len();
                (x, iter::repeat(1).take(len).collect())
            })
            .collect();

        let setup_transactions: Vec<_> = (0..num_src_accounts)
            .map(|i| {
                Transaction::new(
                    &mint.keypair(),
                    srckeys[i].pubkey(),
                    mint_total / num_src_accounts as i64,
                    mint.last_id(),
                )
            })
            .collect();

        let verified_setup: Vec<_> = to_packets_chunked(&packet_recycler, setup_transactions, tx)
            .into_iter()
            .map(|x| {
                let len = (*x).read().unwrap().packets.len();
                (x, iter::repeat(1).take(len).collect())
            })
            .collect();

        bencher.iter(move || {
            let bank = Arc::new(Bank::new(&mint));

            verified_sender.send(verified_setup.clone()).unwrap();
            BankingStage::process_packets(
                bank.clone(),
                &verified_receiver,
                &signal_sender,
                &packet_recycler,
            ).unwrap();

            check_txs(verified_setup.len(), &signal_receiver, num_src_accounts);

            verified_sender.send(verified.clone()).unwrap();
            BankingStage::process_packets(
                bank.clone(),
                &verified_receiver,
                &signal_sender,
                &packet_recycler,
            ).unwrap();

            check_txs(verified.len(), &signal_receiver, tx);
        });
    }

    #[bench]
    fn bench_banking_stage_single_from(bencher: &mut Bencher) {
        logger::setup();
        let tx = 20_000_usize;
        let mint = Mint::new(1_000_000_000_000);
        let mut pubkeys = Vec::new();
        let num_keys = 8;
        for _ in 0..num_keys {
            pubkeys.push(KeyPair::new().pubkey());
        }

        let transactions: Vec<_> = (0..tx)
            .into_par_iter()
            .map(|i| {
                Transaction::new(
                    &mint.keypair(),
                    pubkeys[i % num_keys],
                    i as i64,
                    mint.last_id(),
                )
            })
            .collect();

        let (verified_sender, verified_receiver) = channel();
        let (signal_sender, signal_receiver) = channel();
        let packet_recycler = PacketRecycler::default();
        let verified: Vec<_> = to_packets_chunked(&packet_recycler, transactions, tx)
            .into_iter()
            .map(|x| {
                let len = (*x).read().unwrap().packets.len();
                (x, iter::repeat(1).take(len).collect())
            })
            .collect();

        bencher.iter(move || {
            let bank = Arc::new(Bank::new(&mint));
            verified_sender.send(verified.clone()).unwrap();
            BankingStage::process_packets(
                bank.clone(),
                &verified_receiver,
                &signal_sender,
                &packet_recycler,
            ).unwrap();

            check_txs(verified.len(), &signal_receiver, tx);
        });
    }

}
