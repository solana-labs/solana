//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::Bank;
use bincode::deserialize;
use counter::Counter;
use entry::Entry;
use hash::{Hash, Hasher};
use log::Level;
use packet::{Packets, SharedPackets};
use poh::PohEntry;
use poh_service::PohService;
use rayon::prelude::*;
use result::{Error, Result};
use service::Service;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    /// Handle to the stage's thread.
    thread_hdl: JoinHandle<()>,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    /// Discard input packets using `packet_recycler` to minimize memory
    /// allocations in a previous stage such as the `fetch_stage`.
    pub fn new(
        bank: Arc<Bank>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        tick_duration: Option<Duration>,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (entry_sender, entry_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-banking-stage".to_string())
            .spawn(move || {
                let (hash_sender, hash_receiver) = channel();
                let (poh_service, poh_receiver) =
                    PohService::new(bank.last_id(), hash_receiver, tick_duration);
                loop {
                    if let Err(e) = Self::process_packets(
                        &bank,
                        &hash_sender,
                        &poh_receiver,
                        &verified_receiver,
                        &entry_sender,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            Error::SendError => break,
                            _ => error!("{:?}", e),
                        }
                    }
                }
                drop(hash_sender);
                poh_service.join().unwrap();
            }).unwrap();
        (BankingStage { thread_hdl }, entry_receiver)
    }

    /// Convert the transactions from a blob of binary data to a vector of transactions and
    /// an unused `SocketAddr` that could be used to send a response.
    fn deserialize_transactions(p: &Packets) -> Vec<Option<(Transaction, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            }).collect()
    }

    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        hash_sender: &Sender<Hash>,
        poh_receiver: &Receiver<PohEntry>,
        entry_sender: &Sender<Vec<Entry>>,
    ) -> Result<()> {
        let mut entries = Vec::new();

        debug!("transactions: {}", transactions.len());

        let mut chunk_start = 0;
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start + Entry::num_will_fit(&transactions[chunk_start..]);

            let results = bank.process_transactions(&transactions[chunk_start..chunk_end]);
            debug!("results: {}", results.len());

            let mut hasher = Hasher::default();

            let processed_transactions: Vec<_> = transactions[chunk_start..chunk_end]
                .into_iter()
                .enumerate()
                .filter_map(|(i, x)| match results[i] {
                    Ok(_) => {
                        hasher.hash(&x.signature.as_ref());
                        Some(x.clone())
                    }
                    Err(ref e) => {
                        debug!("process transaction failed {:?}", e);
                        None
                    }
                }).collect();

            debug!("processed ok: {}", processed_transactions.len());

            chunk_start = chunk_end;

            let hash = hasher.result();

            if !processed_transactions.is_empty() {
                hash_sender.send(hash)?;

                let mut answered = false;
                while !answered {
                    entries.extend(poh_receiver.try_iter().map(|poh| {
                        if let Some(mixin) = poh.mixin {
                            answered = true;
                            assert_eq!(mixin, hash);
                            bank.register_entry_id(&poh.id);
                            Entry {
                                num_hashes: poh.num_hashes,
                                id: poh.id,
                                transactions: processed_transactions.clone(),
                            }
                        } else {
                            Entry {
                                num_hashes: poh.num_hashes,
                                id: poh.id,
                                transactions: vec![],
                            }
                        }
                    }));
                }
            } else {
                entries.extend(poh_receiver.try_iter().map(|poh| Entry {
                    num_hashes: poh.num_hashes,
                    id: poh.id,
                    transactions: vec![],
                }));
            }
        }

        debug!("done process_transactions, {} entries", entries.len());

        entry_sender.send(entries)?;
        Ok(())
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        bank: &Arc<Bank>,
        hash_sender: &Sender<Hash>,
        poh_receiver: &Receiver<PohEntry>,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        entry_sender: &Sender<Vec<Entry>>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let recv_start = Instant::now();
        let mms = verified_receiver.recv_timeout(timer)?;
        debug!("verified_recevier {:?}", verified_receiver);
        let now = Instant::now();
        let mut reqs_len = 0;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            mms.len(),
        );
        inc_new_counter_info!("banking_stage-entries_received", mms_len);
        let bank_starting_tx_count = bank.transaction_count();
        let count = mms.iter().map(|x| x.1.len()).sum();
        let proc_start = Instant::now();
        for (msgs, vers) in mms {
            let transactions = Self::deserialize_transactions(&msgs.read());
            reqs_len += transactions.len();

            let transactions: Vec<_> = transactions
                .into_iter()
                .zip(vers)
                .filter_map(|(tx, ver)| match tx {
                    None => None,
                    Some((tx, _addr)) => if tx.verify_plan() && ver != 0 {
                        Some(tx)
                    } else {
                        None
                    },
                }).collect();

            Self::process_transactions(
                bank,
                &transactions,
                hash_sender,
                poh_receiver,
                entry_sender,
            )?;
        }

        inc_new_counter_info!(
            "banking_stage-time_ms",
            timing::duration_as_ms(&now.elapsed()) as usize
        );
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
        inc_new_counter_info!("banking_stage-process_packets", count);
        inc_new_counter_info!(
            "banking_stage-process_transactions",
            bank.transaction_count() - bank_starting_tx_count
        );
        Ok(())
    }
}

impl Service for BankingStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
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
