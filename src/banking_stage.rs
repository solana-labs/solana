//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::Bank;
use bincode::deserialize;
use counter::Counter;
use log::Level;
use packet::{PacketRecycler, Packets, SharedPackets};
use rayon::prelude::*;
use record_stage::Signal;
use result::{Error, Result};
use service::Service;
use std::net::SocketAddr;
use std::result;
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

fn recv_multiple_packets(
    verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
    wait_ms: u64,
    max_tries: usize,
) -> result::Result<Vec<(SharedPackets, Vec<u8>)>, RecvTimeoutError> {
    let timer = Duration::new(1, 0);
    let mut mms = verified_receiver.recv_timeout(timer)?;
    let mut recv_tries = 1;

    // Try receiving more packets from verified_receiver. Let's coalesce any packets
    // that are received within "wait_ms" ms of each other.
    while let Ok(mut nq) = verified_receiver.recv_timeout(Duration::from_millis(wait_ms)) {
        recv_tries += 1;
        mms.append(&mut nq);

        if recv_tries >= max_tries {
            inc_new_counter_info!("banking_stage-max_packets_coalesced", 1);
            break;
        }
    }
    Ok(mms)
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    /// Discard input packets using `packet_recycler` to minimize memory
    /// allocations in a previous stage such as the `fetch_stage`.
    pub fn new(
        bank: Arc<Bank>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        packet_recycler: PacketRecycler,
    ) -> (Self, Receiver<Signal>) {
        let (signal_sender, signal_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-banking-stage".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::process_packets(
                    &bank,
                    &verified_receiver,
                    &signal_sender,
                    &packet_recycler,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();
        (BankingStage { thread_hdl }, signal_receiver)
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
            })
            .collect()
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        signal_sender: &Sender<Signal>,
        packet_recycler: &PacketRecycler,
    ) -> Result<()> {
        // Coalesce upto 512 transactions before sending it to the next stage
        let max_coalesced_txs = 512;
        let max_recv_tries = 10;
        let recv_start = Instant::now();
        let mms = recv_multiple_packets(verified_receiver, 20, max_recv_tries)?;
        let mut reqs_len = 0;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            mms.len(),
        );
        let bank_starting_tx_count = bank.transaction_count();
        let count = mms.iter().map(|x| x.1.len()).sum();
        let proc_start = Instant::now();
        let mut txs: Vec<Transaction> = Vec::new();
        let mut num_sent = 0;
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
            let mut transactions: Vec<Transaction> =
                results.into_iter().filter_map(|x| x.ok()).collect();
            txs.append(&mut transactions);
            if txs.len() >= max_coalesced_txs {
                signal_sender.send(Signal::Transactions(txs.clone()))?;
                txs.clear();
                num_sent += 1;
            }
            debug!("done process_transactions");

            packet_recycler.recycle(msgs);
        }

        // Send now, if there are pending transactions, or if there was
        // no transactions sent to the next stage yet.
        if !txs.is_empty() || num_sent == 0 {
            signal_sender.send(Signal::Transactions(txs))?;
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
        inc_new_counter_info!("banking_stage-process_packets", count);
        inc_new_counter_info!(
            "banking_stage-process_transactions",
            bank.transaction_count() - bank_starting_tx_count
        );
        Ok(())
    }
}

impl Service for BankingStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod test {
    use banking_stage::recv_multiple_packets;
    use packet::SharedPackets;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::sync::Arc;
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    pub fn recv_multiple_packets_test() {
        let (sender, receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));

        assert_eq!(
            recv_multiple_packets(&receiver, 20, 10).unwrap_err(),
            RecvTimeoutError::Timeout
        );

        {
            let exit = exit.clone();
            thread::spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    let testdata: Vec<(SharedPackets, Vec<u8>)> = Vec::new();
                    sender.send(testdata).expect("Failed to send message");
                    sleep(Duration::from_millis(10));
                }
            });
        }

        assert_eq!(recv_multiple_packets(&receiver, 20, 10).is_ok(), true);
        exit.store(true, Ordering::Relaxed);
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
//    use signature::{Keypair, KeypairUtil};
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
//        let alice = Keypair::new();
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
