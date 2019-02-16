//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.

use crate::bank::{Bank, BankError};
use crate::compute_leader_confirmation_service::ComputeLeaderConfirmationService;
use crate::counter::Counter;
use crate::entry::Entry;
use crate::packet::Packets;
use crate::packet::SharedPackets;
use crate::poh_recorder::{PohRecorder, PohRecorderError};
use crate::poh_service::{PohService, PohServiceConfig};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify_stage::VerifiedPackets;
use crate::tpu::TpuRotationSender;
use bincode::deserialize;
use log::Level;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing;
use solana_sdk::transaction::Transaction;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use sys_info;

pub type UnprocessedPackets = Vec<(SharedPackets, usize)>; // `usize` is the index of the first unprocessed packet in `SharedPackets`

// number of threads is 1 until mt bank is ready
pub const NUM_THREADS: u32 = 10;

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    bank_thread_hdls: Vec<JoinHandle<UnprocessedPackets>>,
    poh_waiter_hdl: JoinHandle<Result<()>>,
    compute_confirmation_service: ComputeLeaderConfirmationService,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        bank: &Arc<Bank>,
        verified_receiver: Receiver<VerifiedPackets>,
        config: PohServiceConfig,
        last_entry_id: &Hash,
        max_tick_height: u64,
        leader_id: Pubkey,
        to_validator_sender: &TpuRotationSender,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (entry_sender, entry_receiver) = channel();
        let shared_verified_receiver = Arc::new(Mutex::new(verified_receiver));
        let poh_recorder =
            PohRecorder::new(bank.clone(), entry_sender, *last_entry_id, max_tick_height);

        // TODO: please pass me current slot
        let current_slot = bank
            .leader_scheduler
            .read()
            .unwrap()
            .tick_height_to_slot(max_tick_height);

        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its last_id is registered with the bank.
        let poh_service = PohService::new(poh_recorder.clone(), config);

        let poh_exit = poh_service.poh_exit.clone();

        // once poh_service finishes, we freeze the current slot and merge it into the root
        let poh_waiter_hdl: JoinHandle<Result<()>> = {
            let bank = bank.clone();
            let to_validator_sender = to_validator_sender.clone();

            Builder::new()
                .name("solana-poh-waiter".to_string())
                .spawn(move || {
                    let poh_return_value = poh_service.join()?;

                    match poh_return_value {
                        Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)) => {
                            trace!("leader for slot {} done", current_slot);
                            if let Some(bank_fork) = bank.fork(current_slot) {
                                trace!("freezing slot {}", current_slot);
                                bank_fork.head().freeze();
                                bank.merge_into_root(current_slot);
                            } else {
                                trace!("current slot not found! {}", current_slot);
                            }
                            to_validator_sender.send(max_tick_height)?
                        }
                        _ => (),
                    }

                    poh_return_value
                })
                .unwrap()
        };

        // Single thread to compute confirmation
        let compute_confirmation_service =
            ComputeLeaderConfirmationService::new(bank.clone(), leader_id, poh_exit.clone());

        // Many banks that process transactions in parallel.
        let bank_thread_hdls: Vec<JoinHandle<UnprocessedPackets>> = (0..Self::num_threads())
            .map(|_| {
                let thread_bank = bank.clone();
                let thread_verified_receiver = shared_verified_receiver.clone();
                let thread_poh_recorder = poh_recorder.clone();

                Builder::new()
                    .name("solana-banking-stage-tx".to_string())
                    .spawn(move || {
                        let mut unprocessed_packets: UnprocessedPackets = vec![];
                        loop {
                            match Self::process_packets(
                                &thread_bank,
                                &thread_verified_receiver,
                                &thread_poh_recorder,
                            ) {
                                Err(Error::RecvTimeoutError(RecvTimeoutError::Timeout)) => (),
                                Ok(more_unprocessed_packets) => {
                                    unprocessed_packets.extend(more_unprocessed_packets);
                                }
                                Err(err) => {
                                    debug!("solana-banking-stage-tx: exit due to {:?}", err);
                                    break;
                                }
                            }
                        }
                        unprocessed_packets
                    })
                    .unwrap()
            })
            .collect();

        (
            Self {
                bank_thread_hdls,
                poh_waiter_hdl,
                compute_confirmation_service,
            },
            entry_receiver,
        )
    }

    pub fn num_threads() -> u32 {
        sys_info::cpu_num().unwrap_or(NUM_THREADS)
    }

    /// Convert the transactions from a blob of binary data to a vector of transactions
    fn deserialize_transactions(p: &Packets) -> Vec<Option<Transaction>> {
        p.packets
            .iter()
            .map(|x| deserialize(&x.data[0..x.meta.size]).ok())
            .collect()
    }

    /// Sends transactions to the bank.
    ///
    /// Returns the number of transactions successfully processed by the bank, which may be less
    /// than the total number if max PoH height was reached and the bank halted
    fn process_transactions(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        poh: &PohRecorder,
    ) -> Result<(usize)> {
        let mut chunk_start = 0;
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start + Entry::num_will_fit(&transactions[chunk_start..]);

            let result = bank
                .process_and_record_transactions(&transactions[chunk_start..chunk_end], Some(poh));
            if Err(BankError::MaxHeightReached) == result {
                break;
            }
            result?;
            chunk_start = chunk_end;
        }
        Ok(chunk_start)
    }

    /// Process the incoming packets
    pub fn process_packets(
        bank: &Arc<Bank>,
        verified_receiver: &Arc<Mutex<Receiver<VerifiedPackets>>>,
        poh: &PohRecorder,
    ) -> Result<UnprocessedPackets> {
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
        let count = mms.iter().map(|x| x.1.len()).sum();
        let proc_start = Instant::now();
        let mut new_tx_count = 0;

        let mut unprocessed_packets = vec![];
        let mut bank_shutdown = false;
        for (msgs, vers) in mms {
            if bank_shutdown {
                unprocessed_packets.push((msgs, 0));
                continue;
            }

            let transactions = Self::deserialize_transactions(&msgs.read().unwrap());
            reqs_len += transactions.len();

            debug!("transactions received {}", transactions.len());
            let (verified_transactions, verified_transaction_index): (Vec<_>, Vec<_>) =
                transactions
                    .into_iter()
                    .zip(vers)
                    .zip(0..)
                    .filter_map(|((tx, ver), index)| match tx {
                        None => None,
                        Some(tx) => {
                            if tx.verify_refs() && ver != 0 {
                                Some((tx, index))
                            } else {
                                None
                            }
                        }
                    })
                    .unzip();

            debug!("verified transactions {}", verified_transactions.len());

            let processed = Self::process_transactions(bank, &verified_transactions, poh)?;
            if processed < verified_transactions.len() {
                bank_shutdown = true;
                // Collect any unprocessed transactions in this batch for forwarding
                unprocessed_packets.push((msgs, verified_transaction_index[processed]));
            }
            new_tx_count += processed;
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
        inc_new_counter_info!("banking_stage-process_transactions", new_tx_count);

        Ok(unprocessed_packets)
    }

    pub fn join_and_collect_unprocessed_packets(&mut self) -> UnprocessedPackets {
        let mut unprocessed_packets: UnprocessedPackets = vec![];
        for bank_thread_hdl in self.bank_thread_hdls.drain(..) {
            match bank_thread_hdl.join() {
                Ok(more_unprocessed_packets) => {
                    unprocessed_packets.extend(more_unprocessed_packets)
                }
                err => warn!("bank_thread_hdl join failed: {:?}", err),
            }
        }
        unprocessed_packets
    }
}

impl Service for BankingStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for bank_thread_hdl in self.bank_thread_hdls {
            bank_thread_hdl.join()?;
        }
        self.compute_confirmation_service.join()?;
        let _ = self.poh_waiter_hdl.join()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntrySlice;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::{LeaderSchedulerConfig, DEFAULT_TICKS_PER_SLOT};
    use crate::packet::to_packets;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use std::thread::sleep;

    #[test]
    fn test_banking_stage_shutdown1() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, _) = channel();
        let (banking_stage, _entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::default(),
            &bank.active_fork().last_id(),
            DEFAULT_TICKS_PER_SLOT,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );
        drop(verified_sender);
        banking_stage.join().unwrap();
    }

    #[test]
    fn test_banking_stage_tick() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let start_hash = bank.active_fork().last_id();
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, _) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::Sleep(Duration::from_millis(1)),
            &bank.active_fork().last_id(),
            DEFAULT_TICKS_PER_SLOT,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );
        sleep(Duration::from_millis(500));
        drop(verified_sender);

        let entries: Vec<_> = entry_receiver.iter().flat_map(|x| x).collect();
        assert!(entries.len() != 0);
        assert!(entries.verify(&start_hash));
        assert_eq!(entries[entries.len() - 1].id, bank.active_fork().last_id());
        banking_stage.join().unwrap();
    }

    #[test]
    fn test_banking_stage_entries_only() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let start_hash = bank.active_fork().last_id();
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, _) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::default(),
            &bank.active_fork().last_id(),
            DEFAULT_TICKS_PER_SLOT,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );

        // good tx
        let keypair = mint_keypair;
        let tx = SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, start_hash, 0);

        // good tx, but no verify
        let tx_no_ver =
            SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, start_hash, 0);

        // bad tx, AccountNotFound
        let keypair = Keypair::new();
        let tx_anf = SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, start_hash, 0);

        // send 'em over
        let packets = to_packets(&[tx, tx_no_ver, tx_anf]);

        // glad they all fit
        assert_eq!(packets.len(), 1);
        verified_sender // tx, no_ver, anf
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
        banking_stage.join().unwrap();
    }
    #[test]
    fn test_banking_stage_entryfication() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, _) = channel();
        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::default(),
            &bank.active_fork().last_id(),
            DEFAULT_TICKS_PER_SLOT,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );

        // Process a batch that includes a transaction that receives two tokens.
        let alice = Keypair::new();
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            alice.pubkey(),
            2,
            genesis_block.last_id(),
            0,
        );

        let packets = to_packets(&[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();

        // Process a second batch that spends one of those tokens.
        let tx = SystemTransaction::new_account(
            &alice,
            mint_keypair.pubkey(),
            1,
            genesis_block.last_id(),
            0,
        );
        let packets = to_packets(&[tx]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();
        drop(verified_sender);
        banking_stage.join().unwrap();

        // Collect the ledger and feed it to a new bank.
        let entries: Vec<_> = entry_receiver.iter().flat_map(|x| x).collect();
        // same assertion as running through the bank, really...
        assert!(entries.len() >= 2);

        // Assert the user holds one token, not two. If the stage only outputs one
        // entry, then the second transaction will be rejected, because it drives
        // the account balance below zero before the credit is added.
        let bank = Bank::new(&genesis_block);
        for entry in entries {
            bank.process_transactions(&entry.transactions)
                .iter()
                .for_each(|x| assert_eq!(*x, Ok(())));
        }
        assert_eq!(bank.active_fork().get_balance_slow(&alice.pubkey()), 1);
    }

    // Test that when the max_tick_height is reached, the banking stage exits
    #[test]
    fn test_max_tick_height_shutdown() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(2);
        let bank = Arc::new(Bank::new(&genesis_block));
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, to_validator_receiver) = channel();
        let max_tick_height = 10;
        let (banking_stage, _entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::default(),
            &bank.active_fork().last_id(),
            max_tick_height,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );
        assert_eq!(to_validator_receiver.recv().unwrap(), max_tick_height);
        drop(verified_sender);
        banking_stage.join().unwrap();
    }

    #[test]
    fn test_returns_unprocessed_packet() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let leader_scheduler_config = LeaderSchedulerConfig::new(1, 1, 1);
        let bank = Arc::new(Bank::new_with_leader_scheduler_config(
            &genesis_block,
            &leader_scheduler_config,
        ));
        let (verified_sender, verified_receiver) = channel();
        let (to_validator_sender, to_validator_receiver) = channel();
        let (mut banking_stage, _entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            PohServiceConfig::default(),
            &bank.active_fork().last_id(),
            leader_scheduler_config.ticks_per_slot,
            genesis_block.bootstrap_leader_id,
            &to_validator_sender,
        );

        // Wait for Poh recorder to hit max height
        assert_eq!(
            to_validator_receiver.recv().unwrap(),
            leader_scheduler_config.ticks_per_slot
        );

        // Now send a transaction to the banking stage
        let transaction = SystemTransaction::new_account(
            &mint_keypair,
            Keypair::new().pubkey(),
            2,
            genesis_block.last_id(),
            0,
        );

        let packets = to_packets(&[transaction]);
        verified_sender
            .send(vec![(packets[0].clone(), vec![1u8])])
            .unwrap();

        // Shut down the banking stage, it should give back the transaction
        drop(verified_sender);
        let unprocessed_packets = banking_stage.join_and_collect_unprocessed_packets();
        assert_eq!(unprocessed_packets.len(), 1);
        let (packets, start_index) = &unprocessed_packets[0];
        assert_eq!(packets.read().unwrap().packets.len(), 1); // TODO: maybe compare actual packet contents too
        assert_eq!(*start_index, 0);
    }
}
