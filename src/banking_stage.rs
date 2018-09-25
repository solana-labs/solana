//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::Bank;
use bincode::deserialize;
use counter::Counter;
use entry::Entry;
use hash::Hasher;
use log::Level;
use packet::{Packets, SharedPackets};
use poh_service::PohService;
use rayon::prelude::*;
use result::{Error, Result};
use service::Service;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

pub const NUM_THREADS: usize = 10;
/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    /// Handle to the stage's thread.
    thread_hdls: Vec<JoinHandle<()>>,
}

pub enum Config {
    /// * `Tick` - Run full PoH thread.  Tick is a rough estimate of how many hashes to roll before transmitting a new entry.
    Tick(usize),
    /// * `Sleep`- Low power mode.  Sleep is a rough estimate of how long to sleep before rolling 1 poh once and producing 1
    /// tick.
    Sleep(Duration),
}

impl Default for Config {
    fn default() -> Config {
        // TODO: Change this to Tick to enable PoH
        Config::Sleep(Duration::from_millis(500))
    }
}
impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    /// Discard input packets using `packet_recycler` to minimize memory
    /// allocations in a previous stage such as the `fetch_stage`.
    pub fn new(
        bank: Arc<Bank>,
        verified_receiver: Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        config: Config,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (entry_sender, entry_receiver) = channel();
        let shared_verified_receiver = Arc::new(Mutex::new(verified_receiver));
        let poh = PohService::new(bank.clone(), entry_sender);
        let tick_poh = poh.clone();

        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its last_id is registered with the bank.
        let tick_producer = Builder::new()
            .name("solana-banking-stage-tick_producer".to_string())
            .spawn(move || {
                if let Err(e) = Self::tick_producer(tick_poh, config) {
                    match e {
                        Error::RecvError(_) => (),
                        Error::SendError => (),
                        _ => error!(
                            "solana-banking-stage-tick_producer unexpected error {:?}",
                            e
                        ),
                    }
                }
            }).unwrap();

        // Many banks that process transactions in parallel.
        let mut thread_hdls: Vec<JoinHandle<()>> = (0..NUM_THREADS)
            .into_iter()
            .map(|_| {
                let thread_bank = bank.clone();
                let thread_verified_receiver = shared_verified_receiver.clone();
                let thread_poh = poh.clone();
                Builder::new()
                    .name("solana-banking-stage-tx".to_string())
                    .spawn(move || loop {
                        if let Err(e) = Self::process_packets(
                            &thread_bank,
                            &thread_verified_receiver,
                            &thread_poh,
                        ) {
                            match e {
                                Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                                Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                Error::RecvError(_) => break,
                                Error::SendError => break,
                                _ => error!("solana-banking-stage-tx {:?}", e),
                            }
                        }
                    }).unwrap()
            }).collect();
        thread_hdls.push(tick_producer);
        (BankingStage { thread_hdls }, entry_receiver)
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

    fn tick_producer(poh: PohService, config: Config) -> Result<()> {
        loop {
            match config {
                Config::Tick(num) => {
                    for _ in 0..num {
                        poh.hash();
                    }
                }
                Config::Sleep(duration) => {
                    sleep(duration);
                }
            }
            poh.tick()?;
        }
    }

    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: Vec<Transaction>,
        poh: &PohService,
    ) -> Result<()> {
        debug!("transactions: {}", transactions.len());
        let mut chunk_start = 0;
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start + Entry::num_will_fit(&transactions[chunk_start..]);

            let results = bank.process_transactions(&transactions[chunk_start..chunk_end]);

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

            if !processed_transactions.is_empty() {
                let hash = hasher.result();
                debug!("processed ok: {} {}", processed_transactions.len(), hash);
                poh.record(hash, processed_transactions)?;
            }
            chunk_start = chunk_end;
        }
        debug!("done process_transactions");
        Ok(())
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Arc<Mutex<Receiver<Vec<(SharedPackets, Vec<u8>)>>>>,
        poh: &PohService,
    ) -> Result<()> {
        let recv_start = Instant::now();
        let mms = verified_receiver.lock().unwrap().recv()?;
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

            debug!("transactions received {}", transactions.len());

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
            debug!("verified transactions {}", transactions.len());
            Self::process_transactions(bank, transactions, poh)?;
        }

        inc_new_counter_info!(
            "banking_stage-time_ms",
            timing::duration_as_ms(&proc_start.elapsed()) as usize
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
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use ledger::Block;
    use mint::Mint;
    use packet::{to_packets, PacketRecycler};
    use signature::{Keypair, KeypairUtil};
    use std::thread::sleep;
    use transaction::Transaction;

    #[test]
    fn test_banking_stage_shutdown() {
        let bank = Bank::new(&Mint::new(2));
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, _entry_receiver) =
            BankingStage::new(Arc::new(bank), verified_receiver, None);
        drop(verified_sender);
        assert_eq!(banking_stage.join().unwrap(), ());
    }

    #[test]
    fn test_banking_stage_tick() {
        let bank = Bank::new(&Mint::new(2));
        let start_hash = bank.last_id();
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            Arc::new(bank),
            verified_receiver,
            Some(Duration::from_millis(1)),
        );
        sleep(Duration::from_millis(50));
        drop(verified_sender);

        let entries: Vec<_> = entry_receiver.iter().flat_map(|x| x).collect();
        assert!(entries.len() != 0);
        assert!(entries.verify(&start_hash));
        assert_eq!(banking_stage.join().unwrap(), ());
    }

    #[test]
    fn test_banking_stage_no_tick() {
        let bank = Bank::new(&Mint::new(2));
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) =
            BankingStage::new(Arc::new(bank), verified_receiver, None);
        sleep(Duration::from_millis(1000));
        drop(verified_sender);

        let entries: Vec<_> = entry_receiver.try_iter().map(|x| x).collect();
        assert!(entries.len() == 0);
        assert_eq!(banking_stage.join().unwrap(), ());
    }

    #[test]
    fn test_banking_stage() {
        let mint = Mint::new(2);
        let bank = Bank::new(&mint);
        let start_hash = bank.last_id();
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) =
            BankingStage::new(Arc::new(bank), verified_receiver, None);

        // good tx
        let keypair = mint.keypair();
        let tx = Transaction::new(&keypair, keypair.pubkey(), 1, start_hash);

        // good tx, but no verify
        let tx_no_ver = Transaction::new(&keypair, keypair.pubkey(), 1, start_hash);

        // bad tx, AccountNotFound
        let keypair = Keypair::new();
        let tx_anf = Transaction::new(&keypair, keypair.pubkey(), 1, start_hash);

        // send 'em over
        let recycler = PacketRecycler::default();
        let packets = to_packets(&recycler, &[tx, tx_no_ver, tx_anf]);

        // glad they all fit
        assert_eq!(packets.len(), 1);
        verified_sender                       // tx, no_ver, anf
            .send(vec![(packets[0].clone(), vec![1u8, 0u8, 1u8])])
            .unwrap();

        drop(verified_sender);

        let entries: Vec<_> = entry_receiver.iter().map(|x| x).collect();
        assert_eq!(entries.len(), 1);
        let mut last_id = start_hash;
        entries.iter().for_each(|entries| {
            assert_eq!(entries.len(), 1);
            assert!(entries.verify(&last_id));
            last_id = entries.last().unwrap().id;
        });
        assert_eq!(banking_stage.join().unwrap(), ());
    }

    #[test]
    fn test_banking_stage_entryfication() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let mint = Mint::new(2);
        let bank = Bank::new(&mint);
        let recycler = PacketRecycler::default();
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) =
            BankingStage::new(Arc::new(bank), verified_receiver, None);

        // Process a batch that includes a transaction that receives two tokens.
        let alice = Keypair::new();
        let tx = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());

        let packets = to_packets(&recycler, &[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();

        // Process a second batch that spends one of those tokens.
        let tx = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
        let packets = to_packets(&recycler, &[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();
        drop(verified_sender);
        assert_eq!(banking_stage.join().unwrap(), ());

        // Collect the ledger and feed it to a new bank.
        let ventries: Vec<_> = entry_receiver.iter().collect();

        // same assertion as below, really...
        assert_eq!(ventries.len(), 2);

        // Assert the user holds one token, not two. If the stage only outputs one
        // entry, then the second transaction will be rejected, because it drives
        // the account balance below zero before the credit is added.
        let bank = Bank::new(&mint);
        for entries in ventries {
            for entry in entries {
                assert!(
                    bank.process_transactions(&entry.transactions)
                        .into_iter()
                        .all(|x| x.is_ok())
                );
            }
        }
        assert_eq!(bank.get_balance(&alice.pubkey()), 1);
    }
}
