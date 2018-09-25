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
                let poh_service = PohService::new(bank.last_id(), tick_duration.is_some());

                let mut last_tick = Instant::now();

                loop {
                    let timeout =
                        tick_duration.map(|duration| duration - (Instant::now() - last_tick));

                    if let Err(e) = Self::process_packets(
                        timeout,
                        &bank,
                        &poh_service,
                        &verified_receiver,
                        &entry_sender,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            Error::RecvError(_) => break,
                            Error::SendError => break,
                            _ => {
                                error!("process_packets() {:?}", e);
                                break;
                            }
                        }
                    }
                    if tick_duration.is_some() && last_tick.elapsed() > tick_duration.unwrap() {
                        if let Err(e) = Self::tick(&poh_service, &entry_sender) {
                            error!("tick() {:?}", e);
                        }
                        last_tick = Instant::now();
                    }
                }
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

    fn tick(poh_service: &PohService, entry_sender: &Sender<Vec<Entry>>) -> Result<()> {
        let poh = poh_service.tick();
        let entry = Entry {
            num_hashes: poh.num_hashes,
            id: poh.id,
            transactions: vec![],
        };
        entry_sender.send(vec![entry])?;
        Ok(())
    }

    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        poh_service: &PohService,
    ) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();

        debug!("processing: {}", transactions.len());

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

            debug!("processed: {}", processed_transactions.len());

            chunk_start = chunk_end;

            let hash = hasher.result();

            if !processed_transactions.is_empty() {
                let poh = poh_service.record(hash);

                bank.register_entry_id(&poh.id);
                entries.push(Entry {
                    num_hashes: poh.num_hashes,
                    id: poh.id,
                    transactions: processed_transactions,
                });
            }
        }

        debug!("done process_transactions, {} entries", entries.len());

        Ok(entries)
    }

    /// Process the incoming packets and send output `Signal` messages to `signal_sender`.
    /// Discard packets via `packet_recycler`.
    pub fn process_packets(
        timeout: Option<Duration>,
        bank: &Arc<Bank>,
        poh_service: &PohService,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        entry_sender: &Sender<Vec<Entry>>,
    ) -> Result<()> {
        let recv_start = Instant::now();
        // TODO pass deadline to recv_deadline() when/if it becomes available?
        let mms = if let Some(timeout) = timeout {
            verified_receiver.recv_timeout(timeout)?
        } else {
            verified_receiver.recv()?
        };
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

            let entries = Self::process_transactions(bank, &transactions, poh_service)?;
            entry_sender.send(entries)?;
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
