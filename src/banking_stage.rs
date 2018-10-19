//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use bank::{Bank, NUM_TICKS_PER_SECOND};
use bincode::deserialize;
use counter::Counter;
use entry::Entry;
use hash::Hash;
use log::Level;
use packet::Packets;
use poh_recorder::{PohRecorder, PohRecorderError};
use rayon::prelude::*;
use result::{Error, Result};
use service::Service;
use sigverify_stage::VerifiedPackets;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use timing;
use transaction::Transaction;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BankingStageReturnType {
    LeaderRotation,
    ChannelDisconnected,
}

// number of threads is 1 until mt bank is ready
pub const NUM_THREADS: usize = 10;

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    /// Handle to the stage's thread.
    bank_thread_hdls: Vec<JoinHandle<Option<BankingStageReturnType>>>,
    tick_producer: JoinHandle<Option<BankingStageReturnType>>,
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
        Config::Sleep(Duration::from_millis(1000 / NUM_TICKS_PER_SECOND as u64))
    }
}
impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    pub fn new(
        bank: &Arc<Bank>,
        verified_receiver: Receiver<VerifiedPackets>,
        config: Config,
        last_entry_id: &Hash,
        tick_height: u64,
        max_tick_height: Option<u64>,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (entry_sender, entry_receiver) = channel();
        let shared_verified_receiver = Arc::new(Mutex::new(verified_receiver));
        let poh = PohRecorder::new(
            bank.clone(),
            entry_sender,
            *last_entry_id,
            tick_height,
            max_tick_height,
        );
        let tick_poh = poh.clone();
        // Tick producer is a headless producer, so when it exits it should notify the banking stage.
        // Since channel are not used to talk between these threads an AtomicBool is used as a
        // signal.
        let poh_exit = Arc::new(AtomicBool::new(false));
        let banking_exit = poh_exit.clone();
        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its last_id is registered with the bank.
        let tick_producer = Builder::new()
            .name("solana-banking-stage-tick_producer".to_string())
            .spawn(move || {
                let mut tick_poh_ = tick_poh;
                let return_value = match Self::tick_producer(&mut tick_poh_, &config, &poh_exit) {
                    Err(Error::SendError) => Some(BankingStageReturnType::ChannelDisconnected),
                    Err(e) => {
                        error!(
                            "solana-banking-stage-tick_producer unexpected error {:?}",
                            e
                        );
                        None
                    }
                    Ok(x) => x,
                };
                debug!("tick producer exiting");
                poh_exit.store(true, Ordering::Relaxed);
                return_value
            }).unwrap();

        // Many banks that process transactions in parallel.
        let bank_thread_hdls: Vec<JoinHandle<Option<BankingStageReturnType>>> = (0..NUM_THREADS)
            .map(|_| {
                let thread_bank = bank.clone();
                let thread_verified_receiver = shared_verified_receiver.clone();
                let thread_poh = poh.clone();
                let thread_banking_exit = banking_exit.clone();
                Builder::new()
                    .name("solana-banking-stage-tx".to_string())
                    .spawn(move || {
                        let return_result = loop {
                            if let Err(e) = Self::process_packets(
                                &thread_bank,
                                &thread_verified_receiver,
                                &thread_poh,
                            ) {
                                debug!("got error {:?}", e);
                                match e {
                                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                        break Some(BankingStageReturnType::ChannelDisconnected);
                                    }
                                    Error::RecvError(_) => {
                                        break Some(BankingStageReturnType::ChannelDisconnected);
                                    }
                                    Error::SendError => {
                                        break Some(BankingStageReturnType::ChannelDisconnected);
                                    }
                                    Error::PohRecorderError(PohRecorderError::MaxHeightReached) => {
                                        break Some(BankingStageReturnType::LeaderRotation);
                                    }
                                    _ => error!("solana-banking-stage-tx {:?}", e),
                                }
                            }
                            if thread_banking_exit.load(Ordering::Relaxed) {
                                debug!("tick service exited");
                                break None;
                            }
                        };
                        thread_banking_exit.store(true, Ordering::Relaxed);
                        return_result
                    }).unwrap()
            }).collect();

        (
            BankingStage {
                bank_thread_hdls,
                tick_producer,
            },
            entry_receiver,
        )
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

    fn tick_producer(
        poh: &mut PohRecorder,
        config: &Config,
        poh_exit: &AtomicBool,
    ) -> Result<Option<BankingStageReturnType>> {
        loop {
            match *config {
                Config::Tick(num) => {
                    for _ in 0..num {
                        match poh.hash() {
                            Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)) => {
                                return Ok(Some(BankingStageReturnType::LeaderRotation));
                            }
                            Err(e) => {
                                return Err(e);
                            }
                            _ => (),
                        }
                    }
                }
                Config::Sleep(duration) => {
                    sleep(duration);
                }
            }
            match poh.tick() {
                Ok(height) if Some(height) == poh.max_tick_height => {
                    // CASE 1: We were successful in recording the last tick, so exit
                    return Ok(Some(BankingStageReturnType::LeaderRotation));
                }
                Ok(_) => (),
                Err(e) => {
                    return Err(e);
                }
            };
            if poh_exit.load(Ordering::Relaxed) {
                debug!("tick service exited");
                return Ok(None);
            }
        }
    }

    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        poh: &PohRecorder,
    ) -> Result<()> {
        debug!("transactions: {}", transactions.len());
        let mut chunk_start = 0;
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start + Entry::num_will_fit(&transactions[chunk_start..]);

            bank.process_and_record_transactions(&transactions[chunk_start..chunk_end], poh)?;

            chunk_start = chunk_end;
        }
        debug!("done process_transactions");
        Ok(())
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Arc<Mutex<Receiver<VerifiedPackets>>>,
        poh: &PohRecorder,
    ) -> Result<()> {
        let recv_start = Instant::now();
        let mms = verified_receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))?;
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
            let transactions = Self::deserialize_transactions(&msgs.read().unwrap());
            reqs_len += transactions.len();

            debug!("transactions received {}", transactions.len());

            let transactions: Vec<_> = transactions
                .into_iter()
                .zip(vers)
                .filter_map(|(tx, ver)| match tx {
                    None => None,
                    Some((tx, _addr)) => if tx.verify_refs() && ver != 0 {
                        Some(tx)
                    } else {
                        None
                    },
                }).collect();
            debug!("verified transactions {}", transactions.len());
            Self::process_transactions(bank, &transactions, poh)?;
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
    type JoinReturnType = Option<BankingStageReturnType>;

    fn join(self) -> thread::Result<Option<BankingStageReturnType>> {
        let mut return_value = None;

        for bank_thread_hdl in self.bank_thread_hdls {
            let thread_return_value = bank_thread_hdl.join()?;
            if thread_return_value.is_some() {
                return_value = thread_return_value;
            }
        }

        let tick_return_value = self.tick_producer.join()?;
        if tick_return_value.is_some() {
            return_value = tick_return_value;
        }

        Ok(return_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use banking_stage::BankingStageReturnType;
    use ledger::Block;
    use mint::Mint;
    use packet::to_packets;
    use signature::{Keypair, KeypairUtil};
    use std::thread::sleep;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    #[test]
    fn test_banking_stage_shutdown1() {
        let bank = Arc::new(Bank::new(&Mint::new(2)));
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, _entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Default::default(),
            &bank.last_id(),
            0,
            None,
        );
        drop(verified_sender);
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::ChannelDisconnected)
        );
    }

    #[test]
    fn test_banking_stage_shutdown2() {
        let bank = Arc::new(Bank::new(&Mint::new(2)));
        let (_verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Default::default(),
            &bank.last_id(),
            0,
            None,
        );
        drop(entry_receiver);
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::ChannelDisconnected)
        );
    }

    #[test]
    fn test_banking_stage_tick() {
        let bank = Arc::new(Bank::new(&Mint::new(2)));
        let start_hash = bank.last_id();
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Config::Sleep(Duration::from_millis(1)),
            &bank.last_id(),
            0,
            None,
        );
        sleep(Duration::from_millis(500));
        drop(verified_sender);

        let entries: Vec<_> = entry_receiver.iter().flat_map(|x| x).collect();
        assert!(entries.len() != 0);
        assert!(entries.verify(&start_hash));
        assert_eq!(entries[entries.len() - 1].id, bank.last_id());
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::ChannelDisconnected)
        );
    }

    #[test]
    fn test_banking_stage_entries_only() {
        let mint = Mint::new(2);
        let bank = Arc::new(Bank::new(&mint));
        let start_hash = bank.last_id();
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Default::default(),
            &bank.last_id(),
            0,
            None,
        );

        // good tx
        let keypair = mint.keypair();
        let tx = Transaction::system_new(&keypair, keypair.pubkey(), 1, start_hash);

        // good tx, but no verify
        let tx_no_ver = Transaction::system_new(&keypair, keypair.pubkey(), 1, start_hash);

        // bad tx, AccountNotFound
        let keypair = Keypair::new();
        let tx_anf = Transaction::system_new(&keypair, keypair.pubkey(), 1, start_hash);

        // send 'em over
        let packets = to_packets(&[tx, tx_no_ver, tx_anf]);

        // glad they all fit
        assert_eq!(packets.len(), 1);
        verified_sender                       // tx, no_ver, anf
            .send(vec![(packets[0].clone(), vec![1u8, 0u8, 1u8])])
            .unwrap();

        drop(verified_sender);

        //receive entries + ticks
        let entries: Vec<_> = entry_receiver.iter().map(|x| x).collect();
        assert!(entries.len() >= 1);

        let mut last_id = start_hash;
        entries.iter().for_each(|entries| {
            assert_eq!(entries.len(), 1);
            assert!(entries.verify(&last_id));
            last_id = entries.last().unwrap().id;
        });
        drop(entry_receiver);
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::ChannelDisconnected)
        );
    }
    #[test]
    fn test_banking_stage_entryfication() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let mint = Mint::new(2);
        let bank = Arc::new(Bank::new(&mint));
        let (verified_sender, verified_receiver) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Default::default(),
            &bank.last_id(),
            0,
            None,
        );

        // Process a batch that includes a transaction that receives two tokens.
        let alice = Keypair::new();
        let tx = Transaction::system_new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());

        let packets = to_packets(&[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();

        // Process a second batch that spends one of those tokens.
        let tx = Transaction::system_new(&alice, mint.pubkey(), 1, mint.last_id());
        let packets = to_packets(&[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();
        drop(verified_sender);
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::ChannelDisconnected)
        );

        // Collect the ledger and feed it to a new bank.
        let entries: Vec<_> = entry_receiver.iter().flat_map(|x| x).collect();
        // same assertion as running through the bank, really...
        assert!(entries.len() >= 2);

        // Assert the user holds one token, not two. If the stage only outputs one
        // entry, then the second transaction will be rejected, because it drives
        // the account balance below zero before the credit is added.
        let bank = Bank::new(&mint);
        for entry in entries {
            bank.process_transactions(&entry.transactions)
                .iter()
                .for_each(|x| assert_eq!(*x, Ok(())));
        }
        assert_eq!(bank.get_balance(&alice.pubkey()), 1);
    }

    // Test that when the max_tick_height is reached, the banking stage exits
    // with reason BankingStageReturnType::LeaderRotation
    #[test]
    fn test_max_tick_height_shutdown() {
        let bank = Arc::new(Bank::new(&Mint::new(2)));
        let (_verified_sender_, verified_receiver) = channel();
        let max_tick_height = 10;
        let (banking_stage, _entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            Default::default(),
            &bank.last_id(),
            0,
            Some(max_tick_height),
        );
        assert_eq!(
            banking_stage.join().unwrap(),
            Some(BankingStageReturnType::LeaderRotation)
        );
    }
}
