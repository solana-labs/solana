//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::Bank;
use bincode::deserialize;
use counter::Counter;
use log::Level;
use packet::{PacketRecycler, SharedPackets};
use rayon::prelude::*;
use record_stage::Signal;
use result::{Error, Result};
use service::Service;
use std::result;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

const MAX_COALESCED_TXS: usize = 512;

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

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        signal_sender: &Sender<Signal>,
        packet_recycler: &PacketRecycler,
    ) -> Result<()> {
        // Coalesce upto 512 transactions before sending it to the next stage
        let max_recv_tries = 10;
        let recv_start = Instant::now();
        let mms = recv_multiple_packets(verified_receiver, 20, max_recv_tries)?;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            mms.len(),
        );
        let bank_starting_tx_count = bank.transaction_count();
        let proc_start = Instant::now();
        let count = mms.iter().map(|x| x.1.len()).sum();
        let txs: Vec<Transaction> = mms
            .iter()
            .flat_map(|(data, vers)| {
                let p = data.read().unwrap();
                let res: Vec<_> = p
                    .packets
                    .par_iter()
                    .enumerate()
                    .filter_map(|(i, x)| {
                        if 0 == vers[i] {
                            None
                        } else {
                            deserialize(&x.data[0..x.meta.size]).ok()
                        }
                    })
                    .collect(); //TODO: so many allocs
                res
            })
            .collect(); //TODO: so many allocs
        for txs in txs.chunks(MAX_COALESCED_TXS) {
            debug!("process_transactions");
            let transactions: Vec<_> = txs.to_vec(); //TODO: so many allocs
            let results = bank.process_transactions(transactions);
            debug!("done process_transactions");
            let output = results.into_iter().filter_map(|x| x.ok()).collect(); //TODO: so many allocs
            signal_sender.send(Signal::Transactions(output))?;
        }
        mms.into_iter()
            .for_each(|(msgs, _)| packet_recycler.recycle(msgs));

        let total_time_s = timing::duration_as_s(&proc_start.elapsed());
        let total_time_ms = timing::duration_as_ms(&proc_start.elapsed());
        info!(
            "@{:?} done processing transaction batches: {} time: {:?}ms reqs: {} reqs/s: {}",
            timing::timestamp(),
            mms_len,
            total_time_ms,
            count,
            (count as f32) / (total_time_s)
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
