//! The `banking_stage` processes Transaction messages. It is intended to be used
//! to contruct a software pipeline. The stage uses all available CPU cores and
//! can do its processing in parallel with signature verification on the GPU.
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::entry;
use crate::entry::{hash_transactions, Entry};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::packet;
use crate::packet::PACKETS_PER_BLOB;
use crate::packet::{Packet, Packets};
use crate::poh_recorder::{PohRecorder, PohRecorderError, WorkingBankEntries};
use crate::poh_service::PohService;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::sigverify_stage::VerifiedPackets;
use bincode::deserialize;
use crossbeam_channel::{Receiver as CrossbeamReceiver, RecvTimeoutError};
use itertools::Itertools;
use solana_measure::measure::Measure;
use solana_metrics::{inc_new_counter_debug, inc_new_counter_info, inc_new_counter_warn};
use solana_runtime::accounts_db::ErrorCounters;
use solana_runtime::bank::Bank;
use solana_runtime::locked_accounts_results::LockedAccountsResults;
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::{
    self, DEFAULT_NUM_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE,
    MAX_TRANSACTION_FORWARDING_DELAY,
};
use solana_sdk::transaction::{self, Transaction, TransactionError};
use std::cmp;
use std::env;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;

type PacketsAndOffsets = (Packets, Vec<usize>);
pub type UnprocessedPackets = Vec<PacketsAndOffsets>;

/// Transaction forwarding
pub const FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET: u64 = 4;

// Fixed thread size seems to be fastest on GCP setup
pub const NUM_THREADS: u32 = 4;

const TOTAL_BUFFERED_PACKETS: usize = 500_000;

/// Stores the stage's thread handle and output receiver.
pub struct BankingStage {
    bank_thread_hdls: Vec<JoinHandle<()>>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BufferedPacketsDecision {
    Consume,
    Forward,
    Hold,
}

impl BankingStage {
    /// Create the stage using `bank`. Exit when `verified_receiver` is dropped.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        verified_receiver: CrossbeamReceiver<VerifiedPackets>,
        verified_vote_receiver: CrossbeamReceiver<VerifiedPackets>,
    ) -> Self {
        Self::new_num_threads(
            cluster_info,
            poh_recorder,
            verified_receiver,
            verified_vote_receiver,
            Self::num_threads(),
        )
    }

    fn new_num_threads(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        verified_receiver: CrossbeamReceiver<VerifiedPackets>,
        verified_vote_receiver: CrossbeamReceiver<VerifiedPackets>,
        num_threads: u32,
    ) -> Self {
        let batch_limit = TOTAL_BUFFERED_PACKETS / ((num_threads - 1) as usize * PACKETS_PER_BLOB);
        // Single thread to generate entries from many banks.
        // This thread talks to poh_service and broadcasts the entries once they have been recorded.
        // Once an entry has been recorded, its blockhash is registered with the bank.
        let my_pubkey = cluster_info.read().unwrap().id();
        // Many banks that process transactions in parallel.
        let bank_thread_hdls: Vec<JoinHandle<()>> = (0..num_threads)
            .map(|i| {
                let (verified_receiver, enable_forwarding) = if i < num_threads - 1 {
                    (verified_receiver.clone(), true)
                } else {
                    // Disable forwarding of vote transactions, as votes are gossiped
                    (verified_vote_receiver.clone(), false)
                };

                let poh_recorder = poh_recorder.clone();
                let cluster_info = cluster_info.clone();
                let mut recv_start = Instant::now();
                Builder::new()
                    .name("solana-banking-stage-tx".to_string())
                    .spawn(move || {
                        Self::process_loop(
                            my_pubkey,
                            &verified_receiver,
                            &poh_recorder,
                            &cluster_info,
                            &mut recv_start,
                            enable_forwarding,
                            i,
                            batch_limit,
                        );
                    })
                    .unwrap()
            })
            .collect();
        Self { bank_thread_hdls }
    }

    fn filter_valid_packets_for_forwarding(all_packets: &[PacketsAndOffsets]) -> Vec<&Packet> {
        all_packets
            .iter()
            .flat_map(|(p, valid_indexes)| valid_indexes.iter().map(move |x| &p.packets[*x]))
            .collect()
    }

    fn forward_buffered_packets(
        socket: &std::net::UdpSocket,
        tpu_forwards: &std::net::SocketAddr,
        unprocessed_packets: &[PacketsAndOffsets],
    ) -> std::io::Result<()> {
        let packets = Self::filter_valid_packets_for_forwarding(unprocessed_packets);
        inc_new_counter_info!("banking_stage-forwarded_packets", packets.len());
        for p in packets {
            socket.send_to(&p.data[..p.meta.size], &tpu_forwards)?;
        }

        Ok(())
    }

    pub fn consume_buffered_packets(
        my_pubkey: &Pubkey,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        buffered_packets: &mut Vec<PacketsAndOffsets>,
        batch_limit: usize,
    ) -> Result<UnprocessedPackets> {
        let mut unprocessed_packets = vec![];
        let mut rebuffered_packets = 0;
        let mut new_tx_count = 0;
        let buffered_len = buffered_packets.len();
        let mut buffered_packets_iter = buffered_packets.drain(..);
        let mut dropped_batches_count = 0;

        let mut proc_start = Measure::start("consume_buffered_process");
        while let Some((msgs, unprocessed_indexes)) = buffered_packets_iter.next() {
            let bank = poh_recorder.lock().unwrap().bank();
            if bank.is_none() {
                rebuffered_packets += unprocessed_indexes.len();
                Self::push_unprocessed(
                    &mut unprocessed_packets,
                    msgs,
                    unprocessed_indexes,
                    &mut dropped_batches_count,
                    batch_limit,
                );
                continue;
            }
            let bank = bank.unwrap();

            let (processed, verified_txs_len, new_unprocessed_indexes) =
                Self::process_received_packets(
                    &bank,
                    &poh_recorder,
                    &msgs,
                    unprocessed_indexes.to_owned(),
                );

            new_tx_count += processed;

            // Collect any unprocessed transactions in this batch for forwarding
            rebuffered_packets += new_unprocessed_indexes.len();
            Self::push_unprocessed(
                &mut unprocessed_packets,
                msgs,
                new_unprocessed_indexes,
                &mut dropped_batches_count,
                batch_limit,
            );

            if processed < verified_txs_len {
                let next_leader = poh_recorder.lock().unwrap().next_slot_leader();
                // Walk thru rest of the transactions and filter out the invalid (e.g. too old) ones
                while let Some((msgs, unprocessed_indexes)) = buffered_packets_iter.next() {
                    let unprocessed_indexes = Self::filter_unprocessed_packets(
                        &bank,
                        &msgs,
                        &unprocessed_indexes,
                        my_pubkey,
                        next_leader,
                    );
                    Self::push_unprocessed(
                        &mut unprocessed_packets,
                        msgs,
                        unprocessed_indexes,
                        &mut dropped_batches_count,
                        batch_limit,
                    );
                }
            }
        }

        proc_start.stop();

        debug!(
            "@{:?} done processing buffered batches: {} time: {:?}ms tx count: {} tx/s: {}",
            timing::timestamp(),
            buffered_len,
            proc_start.as_ms(),
            new_tx_count,
            (new_tx_count as f32) / (proc_start.as_s())
        );

        inc_new_high_rate_counter_info!("banking_stage-rebuffered_packets", rebuffered_packets);
        inc_new_high_rate_counter_info!("banking_stage-consumed_buffered_packets", new_tx_count);
        inc_new_high_rate_counter_debug!("banking_stage-process_transactions", new_tx_count);
        inc_new_high_rate_counter_debug!(
            "banking_stage-dropped_batches_count",
            dropped_batches_count
        );

        Ok(unprocessed_packets)
    }

    fn consume_or_forward_packets(
        my_pubkey: &Pubkey,
        leader_pubkey: Option<Pubkey>,
        bank_is_available: bool,
        would_be_leader: bool,
    ) -> BufferedPacketsDecision {
        leader_pubkey.map_or(
            // If leader is not known, return the buffered packets as is
            BufferedPacketsDecision::Hold,
            // else process the packets
            |x| {
                if bank_is_available {
                    // If the bank is available, this node is the leader
                    BufferedPacketsDecision::Consume
                } else if would_be_leader {
                    // If the node will be the leader soon, hold the packets for now
                    BufferedPacketsDecision::Hold
                } else if x != *my_pubkey {
                    // If the current node is not the leader, forward the buffered packets
                    BufferedPacketsDecision::Forward
                } else {
                    // We don't know the leader. Hold the packets for now
                    BufferedPacketsDecision::Hold
                }
            },
        )
    }

    fn process_buffered_packets(
        my_pubkey: &Pubkey,
        socket: &std::net::UdpSocket,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        buffered_packets: &mut Vec<PacketsAndOffsets>,
        enable_forwarding: bool,
        batch_limit: usize,
    ) -> Result<()> {
        let (poh_next_slot_leader, poh_has_bank, would_be_leader) = {
            let poh = poh_recorder.lock().unwrap();
            (
                poh.next_slot_leader(),
                poh.has_bank(),
                poh.would_be_leader(
                    (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1) * DEFAULT_TICKS_PER_SLOT,
                ),
            )
        };

        let decision = Self::consume_or_forward_packets(
            my_pubkey,
            poh_next_slot_leader,
            poh_has_bank,
            would_be_leader,
        );

        match decision {
            BufferedPacketsDecision::Consume => {
                let mut unprocessed = Self::consume_buffered_packets(
                    my_pubkey,
                    poh_recorder,
                    buffered_packets,
                    batch_limit,
                )?;
                buffered_packets.append(&mut unprocessed);
                Ok(())
            }
            BufferedPacketsDecision::Forward => {
                if enable_forwarding {
                    let next_leader = poh_recorder
                        .lock()
                        .unwrap()
                        .leader_after_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET);
                    next_leader.map_or(Ok(()), |leader_pubkey| {
                        let leader_addr = {
                            cluster_info
                                .read()
                                .unwrap()
                                .lookup(&leader_pubkey)
                                .map(|leader| leader.tpu_forwards)
                        };

                        leader_addr.map_or(Ok(()), |leader_addr| {
                            let _ = Self::forward_buffered_packets(
                                &socket,
                                &leader_addr,
                                &buffered_packets,
                            );
                            buffered_packets.clear();
                            Ok(())
                        })
                    })
                } else {
                    buffered_packets.clear();
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    pub fn process_loop(
        my_pubkey: Pubkey,
        verified_receiver: &CrossbeamReceiver<VerifiedPackets>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        recv_start: &mut Instant,
        enable_forwarding: bool,
        id: u32,
        batch_limit: usize,
    ) {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut buffered_packets = vec![];
        loop {
            if !buffered_packets.is_empty() {
                Self::process_buffered_packets(
                    &my_pubkey,
                    &socket,
                    poh_recorder,
                    cluster_info,
                    &mut buffered_packets,
                    enable_forwarding,
                    batch_limit,
                )
                .unwrap_or_else(|_| buffered_packets.clear());
            }

            let recv_timeout = if !buffered_packets.is_empty() {
                // If packets are buffered, let's wait for less time on recv from the channel.
                // This helps detect the next leader faster, and processing the buffered
                // packets quickly
                Duration::from_millis(10)
            } else {
                // Default wait time
                Duration::from_millis(100)
            };

            match Self::process_packets(
                &my_pubkey,
                &verified_receiver,
                &poh_recorder,
                recv_start,
                recv_timeout,
                id,
                batch_limit,
            ) {
                Err(Error::CrossbeamRecvTimeoutError(RecvTimeoutError::Timeout)) => (),
                Err(Error::CrossbeamRecvTimeoutError(RecvTimeoutError::Disconnected)) => break,
                Ok(mut unprocessed_packets) => {
                    if unprocessed_packets.is_empty() {
                        continue;
                    }
                    let num = unprocessed_packets
                        .iter()
                        .map(|(_, unprocessed)| unprocessed.len())
                        .sum();
                    inc_new_counter_info!("banking_stage-buffered_packets", num);
                    buffered_packets.append(&mut unprocessed_packets);
                }
                Err(err) => {
                    debug!("solana-banking-stage-tx error: {:?}", err);
                }
            }
        }
    }

    pub fn num_threads() -> u32 {
        const MIN_THREADS_VOTES: u32 = 1;
        const MIN_THREADS_BANKING: u32 = 1;
        cmp::max(
            env::var("SOLANA_BANKING_THREADS")
                .map(|x| x.parse().unwrap_or(NUM_THREADS))
                .unwrap_or(NUM_THREADS),
            MIN_THREADS_VOTES + MIN_THREADS_BANKING,
        )
    }

    /// Convert the transactions from a blob of binary data to a vector of transactions
    fn deserialize_transactions(p: &Packets) -> Vec<Option<Transaction>> {
        p.packets
            .iter()
            .map(|x| deserialize(&x.data[0..x.meta.size]).ok())
            .collect()
    }

    #[allow(clippy::match_wild_err_arm)]
    fn record_transactions(
        bank_slot: u64,
        txs: &[Transaction],
        results: &[transaction::Result<()>],
        poh: &Arc<Mutex<PohRecorder>>,
    ) -> (Result<usize>, Vec<usize>) {
        let mut processed_generation = Measure::start("record::process_generation");
        let (processed_transactions, processed_transactions_indexes): (Vec<_>, Vec<_>) = results
            .iter()
            .zip(txs.iter())
            .enumerate()
            .filter_map(|(i, (r, x))| {
                if Bank::can_commit(r) {
                    Some((x.clone(), i))
                } else {
                    None
                }
            })
            .unzip();

        processed_generation.stop();
        let num_to_commit = processed_transactions.len();
        debug!("num_to_commit: {} ", num_to_commit);
        // unlock all the accounts with errors which are filtered by the above `filter_map`
        if !processed_transactions.is_empty() {
            inc_new_counter_warn!("banking_stage-record_transactions", num_to_commit);

            let mut hash_time = Measure::start("record::hash");
            let hash = hash_transactions(&processed_transactions[..]);
            hash_time.stop();

            let mut poh_record = Measure::start("record::poh_record");
            // record and unlock will unlock all the successful transactions
            let res = poh
                .lock()
                .unwrap()
                .record(bank_slot, hash, processed_transactions);

            match res {
                Ok(()) => (),
                Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)) => {
                    // If record errors, add all the committable transactions (the ones
                    // we just attempted to record) as retryable
                    return (
                        Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)),
                        processed_transactions_indexes,
                    );
                }
                Err(e) => panic!(format!("Poh recorder returned unexpected error: {:?}", e)),
            }
            poh_record.stop();
        }
        (Ok(num_to_commit), vec![])
    }

    fn process_and_record_transactions_locked(
        bank: &Bank,
        txs: &[Transaction],
        poh: &Arc<Mutex<PohRecorder>>,
        lock_results: &LockedAccountsResults,
    ) -> (Result<usize>, Vec<usize>) {
        let mut load_execute_time = Measure::start("load_execute_time");
        // Use a shorter maximum age when adding transactions into the pipeline.  This will reduce
        // the likelihood of any single thread getting starved and processing old ids.
        // TODO: Banking stage threads should be prioritized to complete faster then this queue
        // expires.
        let (mut loaded_accounts, results, mut retryable_txs, tx_count, signature_count) =
            bank.load_and_execute_transactions(txs, lock_results, MAX_PROCESSING_AGE);
        load_execute_time.stop();

        let freeze_lock = bank.freeze_lock();

        let mut record_time = Measure::start("record_time");
        let (num_to_commit, retryable_record_txs) =
            Self::record_transactions(bank.slot(), txs, &results, poh);
        retryable_txs.extend(retryable_record_txs);
        if num_to_commit.is_err() {
            return (num_to_commit, retryable_txs);
        }
        record_time.stop();

        let mut commit_time = Measure::start("commit_time");

        let num_to_commit = num_to_commit.unwrap();

        if num_to_commit != 0 {
            bank.commit_transactions(
                txs,
                &mut loaded_accounts,
                &results,
                tx_count,
                signature_count,
            );
        }
        commit_time.stop();

        drop(freeze_lock);

        debug!(
            "bank: {} process_and_record_locked: {}us record: {}us commit: {}us txs_len: {}",
            bank.slot(),
            load_execute_time.as_us(),
            record_time.as_us(),
            commit_time.as_us(),
            txs.len(),
        );

        (Ok(num_to_commit), retryable_txs)
    }

    pub fn process_and_record_transactions(
        bank: &Bank,
        txs: &[Transaction],
        poh: &Arc<Mutex<PohRecorder>>,
        chunk_offset: usize,
    ) -> (Result<usize>, Vec<usize>) {
        let mut lock_time = Measure::start("lock_time");
        // Once accounts are locked, other threads cannot encode transactions that will modify the
        // same account state
        let lock_results = bank.lock_accounts(txs);
        lock_time.stop();

        let (result, mut retryable_txs) =
            Self::process_and_record_transactions_locked(bank, txs, poh, &lock_results);
        retryable_txs.iter_mut().for_each(|x| *x += chunk_offset);

        let mut unlock_time = Measure::start("unlock_time");
        // Once the accounts are new transactions can enter the pipeline to process them
        drop(lock_results);
        unlock_time.stop();

        debug!(
            "bank: {} lock: {}us unlock: {}us txs_len: {}",
            bank.slot(),
            lock_time.as_us(),
            unlock_time.as_us(),
            txs.len(),
        );

        (result, retryable_txs)
    }

    /// Sends transactions to the bank.
    ///
    /// Returns the number of transactions successfully processed by the bank, which may be less
    /// than the total number if max PoH height was reached and the bank halted
    fn process_transactions(
        bank: &Bank,
        transactions: &[Transaction],
        poh: &Arc<Mutex<PohRecorder>>,
    ) -> (usize, Vec<usize>) {
        let mut chunk_start = 0;
        let mut unprocessed_txs = vec![];
        while chunk_start != transactions.len() {
            let chunk_end = chunk_start
                + entry::num_will_fit(
                    &transactions[chunk_start..],
                    packet::BLOB_DATA_SIZE as u64,
                    &Entry::serialized_to_blob_size,
                );

            let (result, retryable_txs_in_chunk) = Self::process_and_record_transactions(
                bank,
                &transactions[chunk_start..chunk_end],
                poh,
                chunk_start,
            );
            trace!("process_transactions result: {:?}", result);

            // Add the retryable txs (transactions that errored in a way that warrants a retry)
            // to the list of unprocessed txs.
            unprocessed_txs.extend_from_slice(&retryable_txs_in_chunk);
            if let Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached)) = result {
                info!(
                    "process transactions: max height reached slot: {} height: {}",
                    bank.slot(),
                    bank.tick_height()
                );
                // process_and_record_transactions has returned all retryable errors in
                // transactions[chunk_start..chunk_end], so we just need to push the remaining
                // transactions into the unprocessed queue.
                unprocessed_txs.extend(chunk_end..transactions.len());
                break;
            }
            // Don't exit early on any other type of error, continue processing...
            chunk_start = chunk_end;
        }

        (chunk_start, unprocessed_txs)
    }

    // This function returns a vector of transactions that are not None. It also returns a vector
    // with position of the transaction in the input list
    fn filter_transaction_indexes(
        transactions: Vec<Option<Transaction>>,
        indexes: &[usize],
    ) -> (Vec<Transaction>, Vec<usize>) {
        transactions
            .into_iter()
            .zip(indexes)
            .filter_map(|(tx, index)| match tx {
                None => None,
                Some(tx) => Some((tx, index)),
            })
            .unzip()
    }

    // This function creates a filter of transaction results with Ok() for every pending
    // transaction. The non-pending transactions are marked with TransactionError
    fn prepare_filter_for_pending_transactions(
        transactions: &[Transaction],
        pending_tx_indexes: &[usize],
    ) -> Vec<transaction::Result<()>> {
        let mut mask = vec![Err(TransactionError::BlockhashNotFound); transactions.len()];
        pending_tx_indexes.iter().for_each(|x| mask[*x] = Ok(()));
        mask
    }

    // This function returns a vector containing index of all valid transactions. A valid
    // transaction has result Ok() as the value
    fn filter_valid_transaction_indexes(
        valid_txs: &[transaction::Result<()>],
        transaction_indexes: &[usize],
    ) -> Vec<usize> {
        let valid_transactions = valid_txs
            .iter()
            .enumerate()
            .filter_map(|(index, x)| if x.is_ok() { Some(index) } else { None })
            .collect_vec();

        valid_transactions
            .iter()
            .map(|x| transaction_indexes[*x])
            .collect()
    }

    // This function deserializes packets into transactions and returns non-None transactions
    fn transactions_from_packets(
        msgs: &Packets,
        transaction_indexes: &[usize],
    ) -> (Vec<Transaction>, Vec<usize>) {
        let packets = Packets::new(
            transaction_indexes
                .iter()
                .map(|x| msgs.packets[*x].to_owned())
                .collect_vec(),
        );

        let transactions = Self::deserialize_transactions(&packets);

        Self::filter_transaction_indexes(transactions, &transaction_indexes)
    }

    /// This function filters pending packets that are still valid
    /// # Arguments
    /// * `transactions` - a batch of transactions deserialized from packets
    /// * `transaction_to_packet_indexes` - maps each transaction to a packet index
    /// * `pending_indexes` - identifies which indexes in the `transactions` list are still pending
    fn filter_pending_packets_from_pending_txs(
        bank: &Arc<Bank>,
        transactions: &[Transaction],
        transaction_to_packet_indexes: &[usize],
        pending_indexes: &[usize],
    ) -> Vec<usize> {
        let filter = Self::prepare_filter_for_pending_transactions(transactions, pending_indexes);

        let mut error_counters = ErrorCounters::default();
        // The following code also checks if the blockhash for a transaction is too old
        // The check accounts for
        //  1. Transaction forwarding delay
        //  2. The slot at which the next leader will actually process the transaction
        // Drop the transaction if it will expire by the time the next node receives and processes it
        let result = bank.check_transactions(
            transactions,
            &filter,
            (MAX_PROCESSING_AGE)
                .saturating_sub(MAX_TRANSACTION_FORWARDING_DELAY)
                .saturating_sub(
                    (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET * bank.ticks_per_slot()
                        / DEFAULT_NUM_TICKS_PER_SECOND) as usize,
                ),
            &mut error_counters,
        );

        Self::filter_valid_transaction_indexes(&result, transaction_to_packet_indexes)
    }

    fn process_received_packets(
        bank: &Arc<Bank>,
        poh: &Arc<Mutex<PohRecorder>>,
        msgs: &Packets,
        packet_indexes: Vec<usize>,
    ) -> (usize, usize, Vec<usize>) {
        let (transactions, transaction_to_packet_indexes) =
            Self::transactions_from_packets(msgs, &packet_indexes);
        debug!(
            "bank: {} filtered transactions {}",
            bank.slot(),
            transactions.len()
        );

        let tx_len = transactions.len();

        let (processed, unprocessed_tx_indexes) =
            Self::process_transactions(bank, &transactions, poh);

        let unprocessed_tx_count = unprocessed_tx_indexes.len();

        let filtered_unprocessed_packet_indexes = Self::filter_pending_packets_from_pending_txs(
            bank,
            &transactions,
            &transaction_to_packet_indexes,
            &unprocessed_tx_indexes,
        );
        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            unprocessed_tx_count.saturating_sub(filtered_unprocessed_packet_indexes.len())
        );

        (processed, tx_len, filtered_unprocessed_packet_indexes)
    }

    fn filter_unprocessed_packets(
        bank: &Arc<Bank>,
        msgs: &Packets,
        transaction_indexes: &[usize],
        my_pubkey: &Pubkey,
        next_leader: Option<Pubkey>,
    ) -> Vec<usize> {
        // Check if we are the next leader. If so, let's not filter the packets
        // as we'll filter it again while processing the packets.
        // Filtering helps if we were going to forward the packets to some other node
        if let Some(leader) = next_leader {
            if leader == *my_pubkey {
                return transaction_indexes.to_vec();
            }
        }

        let (transactions, transaction_to_packet_indexes) =
            Self::transactions_from_packets(msgs, &transaction_indexes);

        let tx_count = transaction_to_packet_indexes.len();

        let unprocessed_tx_indexes = (0..transactions.len()).collect_vec();
        let filtered_unprocessed_packet_indexes = Self::filter_pending_packets_from_pending_txs(
            bank,
            &transactions,
            &transaction_to_packet_indexes,
            &unprocessed_tx_indexes,
        );

        inc_new_counter_info!(
            "banking_stage-dropped_tx_before_forwarding",
            tx_count.saturating_sub(filtered_unprocessed_packet_indexes.len())
        );

        filtered_unprocessed_packet_indexes
    }

    fn generate_packet_indexes(vers: Vec<u8>) -> Vec<usize> {
        vers.iter()
            .enumerate()
            .filter_map(|(index, ver)| if *ver != 0 { Some(index) } else { None })
            .collect()
    }

    /// Process the incoming packets
    pub fn process_packets(
        my_pubkey: &Pubkey,
        verified_receiver: &CrossbeamReceiver<VerifiedPackets>,
        poh: &Arc<Mutex<PohRecorder>>,
        recv_start: &mut Instant,
        recv_timeout: Duration,
        id: u32,
        batch_limit: usize,
    ) -> Result<UnprocessedPackets> {
        let mut recv_time = Measure::start("process_packets_recv");
        let mms = verified_receiver.recv_timeout(recv_timeout)?;
        recv_time.stop();

        let mms_len = mms.len();
        let count: usize = mms.iter().map(|x| x.1.len()).sum();
        debug!(
            "@{:?} process start stalled for: {:?}ms txs: {} id: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            count,
            id,
        );
        inc_new_high_rate_counter_debug!("banking_stage-transactions_received", count);
        let mut proc_start = Measure::start("process_received_packets_process");
        let mut new_tx_count = 0;

        let mut mms_iter = mms.into_iter();
        let mut unprocessed_packets = vec![];
        let mut dropped_batches_count = 0;
        while let Some((msgs, vers)) = mms_iter.next() {
            let packet_indexes = Self::generate_packet_indexes(vers);
            let bank = poh.lock().unwrap().bank();
            if bank.is_none() {
                Self::push_unprocessed(
                    &mut unprocessed_packets,
                    msgs,
                    packet_indexes,
                    &mut dropped_batches_count,
                    batch_limit,
                );
                continue;
            }
            let bank = bank.unwrap();

            let (processed, verified_txs_len, unprocessed_indexes) =
                Self::process_received_packets(&bank, &poh, &msgs, packet_indexes);

            new_tx_count += processed;

            // Collect any unprocessed transactions in this batch for forwarding
            Self::push_unprocessed(
                &mut unprocessed_packets,
                msgs,
                unprocessed_indexes,
                &mut dropped_batches_count,
                batch_limit,
            );

            if processed < verified_txs_len {
                let next_leader = poh.lock().unwrap().next_slot_leader();
                // Walk thru rest of the transactions and filter out the invalid (e.g. too old) ones
                while let Some((msgs, vers)) = mms_iter.next() {
                    let packet_indexes = Self::generate_packet_indexes(vers);
                    let unprocessed_indexes = Self::filter_unprocessed_packets(
                        &bank,
                        &msgs,
                        &packet_indexes,
                        &my_pubkey,
                        next_leader,
                    );
                    Self::push_unprocessed(
                        &mut unprocessed_packets,
                        msgs,
                        unprocessed_indexes,
                        &mut dropped_batches_count,
                        batch_limit,
                    );
                }
            }
        }

        proc_start.stop();

        inc_new_counter_debug!("banking_stage-time_ms", proc_start.as_ms() as usize);
        debug!(
            "@{:?} done processing transaction batches: {} time: {:?}ms tx count: {} tx/s: {} total count: {} id: {}",
            timing::timestamp(),
            mms_len,
            proc_start.as_ms(),
            new_tx_count,
            (new_tx_count as f32) / (proc_start.as_s()),
            count,
            id,
        );
        inc_new_high_rate_counter_debug!("banking_stage-process_packets", count);
        inc_new_high_rate_counter_debug!("banking_stage-process_transactions", new_tx_count);
        inc_new_high_rate_counter_debug!(
            "banking_stage-dropped_batches_count",
            dropped_batches_count
        );

        *recv_start = Instant::now();

        Ok(unprocessed_packets)
    }

    fn push_unprocessed(
        unprocessed_packets: &mut UnprocessedPackets,
        packets: Packets,
        packet_indexes: Vec<usize>,
        dropped_batches_count: &mut usize,
        batch_limit: usize,
    ) {
        if !packet_indexes.is_empty() {
            if unprocessed_packets.len() >= batch_limit {
                unprocessed_packets.remove(0);
                *dropped_batches_count += 1;
            }
            unprocessed_packets.push((packets, packet_indexes));
        }
    }
}

impl Service for BankingStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for bank_thread_hdl in self.bank_thread_hdls {
            bank_thread_hdl.join()?;
        }
        Ok(())
    }
}

pub fn create_test_recorder(
    bank: &Arc<Bank>,
    blocktree: &Arc<Blocktree>,
) -> (
    Arc<AtomicBool>,
    Arc<Mutex<PohRecorder>>,
    PohService,
    Receiver<WorkingBankEntries>,
) {
    let exit = Arc::new(AtomicBool::new(false));
    let poh_config = Arc::new(PohConfig::default());
    let (mut poh_recorder, entry_receiver) = PohRecorder::new(
        bank.tick_height(),
        bank.last_blockhash(),
        bank.slot(),
        Some((4, 4)),
        bank.ticks_per_slot(),
        &Pubkey::default(),
        blocktree,
        &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
        &poh_config,
    );
    poh_recorder.set_bank(&bank);

    let poh_recorder = Arc::new(Mutex::new(poh_recorder));
    let poh_service = PohService::new(poh_recorder.clone(), &poh_config, &exit);

    (exit, poh_recorder, poh_service, entry_receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::cluster_info::Node;
    use crate::entry::EntrySlice;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::packet::to_packets;
    use crate::poh_recorder::WorkingBank;
    use crate::{get_tmp_ledger_path, tmp_ledger_name};
    use crossbeam_channel::unbounded;
    use itertools::Itertools;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::TransactionError;
    use std::sync::atomic::Ordering;
    use std::thread::sleep;

    #[test]
    fn test_banking_stage_shutdown1() {
        let genesis_block = create_genesis_block(2).genesis_block;
        let bank = Arc::new(Bank::new(&genesis_block));
        let (verified_sender, verified_receiver) = unbounded();
        let (vote_sender, vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );
            let (exit, poh_recorder, poh_service, _entry_receiever) =
                create_test_recorder(&bank, &blocktree);
            let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
            let cluster_info = Arc::new(RwLock::new(cluster_info));
            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                vote_receiver,
            );
            drop(verified_sender);
            drop(vote_sender);
            exit.store(true, Ordering::Relaxed);
            banking_stage.join().unwrap();
            poh_service.join().unwrap();
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_banking_stage_tick() {
        solana_logger::setup();
        let GenesisBlockInfo {
            mut genesis_block, ..
        } = create_genesis_block(2);
        genesis_block.ticks_per_slot = 4;
        let bank = Arc::new(Bank::new(&genesis_block));
        let start_hash = bank.last_blockhash();
        let (verified_sender, verified_receiver) = unbounded();
        let (vote_sender, vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );
            let (exit, poh_recorder, poh_service, entry_receiver) =
                create_test_recorder(&bank, &blocktree);
            let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
            let cluster_info = Arc::new(RwLock::new(cluster_info));
            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                vote_receiver,
            );
            trace!("sending bank");
            sleep(Duration::from_millis(600));
            drop(verified_sender);
            drop(vote_sender);
            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
            drop(poh_recorder);

            trace!("getting entries");
            let entries: Vec<_> = entry_receiver
                .iter()
                .flat_map(|x| x.1.into_iter().map(|e| e.0))
                .collect();
            trace!("done");
            assert_eq!(entries.len(), genesis_block.ticks_per_slot as usize - 1);
            assert!(entries.verify(&start_hash));
            assert_eq!(entries[entries.len() - 1].hash, bank.last_blockhash());
            banking_stage.join().unwrap();
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_banking_stage_entries_only() {
        solana_logger::setup();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10);
        let bank = Arc::new(Bank::new(&genesis_block));
        let start_hash = bank.last_blockhash();
        let (verified_sender, verified_receiver) = unbounded();
        let (vote_sender, vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );
            let (exit, poh_recorder, poh_service, entry_receiver) =
                create_test_recorder(&bank, &blocktree);
            let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
            let cluster_info = Arc::new(RwLock::new(cluster_info));
            let banking_stage = BankingStage::new(
                &cluster_info,
                &poh_recorder,
                verified_receiver,
                vote_receiver,
            );

            // fund another account so we can send 2 good transactions in a single batch.
            let keypair = Keypair::new();
            let fund_tx = system_transaction::create_user_account(
                &mint_keypair,
                &keypair.pubkey(),
                2,
                start_hash,
            );
            bank.process_transaction(&fund_tx).unwrap();

            // good tx
            let to = Pubkey::new_rand();
            let tx = system_transaction::create_user_account(&mint_keypair, &to, 1, start_hash);

            // good tx, but no verify
            let to2 = Pubkey::new_rand();
            let tx_no_ver = system_transaction::create_user_account(&keypair, &to2, 2, start_hash);

            // bad tx, AccountNotFound
            let keypair = Keypair::new();
            let to3 = Pubkey::new_rand();
            let tx_anf = system_transaction::create_user_account(&keypair, &to3, 1, start_hash);

            // send 'em over
            let packets = to_packets(&[tx_no_ver, tx_anf, tx]);

            // glad they all fit
            assert_eq!(packets.len(), 1);

            let packets = packets
                .into_iter()
                .map(|packets| (packets, vec![0u8, 1u8, 1u8]))
                .collect();

            verified_sender // no_ver, anf, tx
                .send(packets)
                .unwrap();

            drop(verified_sender);
            drop(vote_sender);
            exit.store(true, Ordering::Relaxed);
            poh_service.join().unwrap();
            drop(poh_recorder);

            let mut blockhash = start_hash;
            let bank = Bank::new(&genesis_block);
            bank.process_transaction(&fund_tx).unwrap();
            //receive entries + ticks
            for _ in 0..10 {
                let ventries: Vec<Vec<Entry>> = entry_receiver
                    .iter()
                    .map(|x| x.1.into_iter().map(|e| e.0).collect())
                    .collect();

                for entries in &ventries {
                    for entry in entries {
                        bank.process_transactions(&entry.transactions)
                            .iter()
                            .for_each(|x| assert_eq!(*x, Ok(())));
                    }
                    assert!(entries.verify(&blockhash));
                    blockhash = entries.last().unwrap().hash;
                }

                if bank.get_balance(&to) == 1 {
                    break;
                }

                sleep(Duration::from_millis(200));
            }

            assert_eq!(bank.get_balance(&to), 1);
            assert_eq!(bank.get_balance(&to2), 0);

            drop(entry_receiver);
            banking_stage.join().unwrap();
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_banking_stage_entryfication() {
        solana_logger::setup();
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(2);
        let (verified_sender, verified_receiver) = unbounded();

        // Process a batch that includes a transaction that receives two lamports.
        let alice = Keypair::new();
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &alice.pubkey(),
            2,
            genesis_block.hash(),
        );

        let packets = to_packets(&[tx]);
        let packets = packets
            .into_iter()
            .map(|packets| (packets, vec![1u8]))
            .collect();
        verified_sender.send(packets).unwrap();

        // Process a second batch that spends one of those lamports.
        let tx = system_transaction::create_user_account(
            &alice,
            &mint_keypair.pubkey(),
            1,
            genesis_block.hash(),
        );
        let packets = to_packets(&[tx]);
        let packets = packets
            .into_iter()
            .map(|packets| (packets, vec![1u8]))
            .collect();
        verified_sender.send(packets).unwrap();

        let (vote_sender, vote_receiver) = unbounded();
        let ledger_path = get_tmp_ledger_path!();
        {
            let entry_receiver = {
                // start a banking_stage to eat verified receiver
                let bank = Arc::new(Bank::new(&genesis_block));
                let blocktree = Arc::new(
                    Blocktree::open(&ledger_path)
                        .expect("Expected to be able to open database ledger"),
                );
                let (exit, poh_recorder, poh_service, entry_receiver) =
                    create_test_recorder(&bank, &blocktree);
                let cluster_info =
                    ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
                let cluster_info = Arc::new(RwLock::new(cluster_info));
                let _banking_stage = BankingStage::new_num_threads(
                    &cluster_info,
                    &poh_recorder,
                    verified_receiver,
                    vote_receiver,
                    2,
                );

                // wait for banking_stage to eat the packets
                while bank.get_balance(&alice.pubkey()) != 1 {
                    sleep(Duration::from_millis(100));
                }
                exit.store(true, Ordering::Relaxed);
                poh_service.join().unwrap();
                entry_receiver
            };
            drop(verified_sender);
            drop(vote_sender);

            // consume the entire entry_receiver, feed it into a new bank
            // check that the balance is what we expect.
            let entries: Vec<_> = entry_receiver
                .iter()
                .flat_map(|x| x.1.into_iter().map(|e| e.0))
                .collect();

            let bank = Bank::new(&genesis_block);
            for entry in &entries {
                bank.process_transactions(&entry.transactions)
                    .iter()
                    .for_each(|x| assert_eq!(*x, Ok(())));
            }

            // Assert the user holds one lamport, not two. If the stage only outputs one
            // entry, then the second transaction will be rejected, because it drives
            // the account balance below zero before the credit is added.
            assert_eq!(bank.get_balance(&alice.pubkey()), 1);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_bank_record_transactions() {
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let working_bank = WorkingBank {
            bank: bank.clone(),
            min_tick_height: bank.tick_height(),
            max_tick_height: std::u64::MAX,
        };
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (poh_recorder, entry_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.slot(),
                None,
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let poh_recorder = Arc::new(Mutex::new(poh_recorder));

            poh_recorder.lock().unwrap().set_working_bank(working_bank);
            let pubkey = Pubkey::new_rand();
            let keypair2 = Keypair::new();
            let pubkey2 = Pubkey::new_rand();

            let transactions = vec![
                system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
                system_transaction::transfer(&keypair2, &pubkey2, 1, genesis_block.hash()),
            ];

            let mut results = vec![Ok(()), Ok(())];
            BankingStage::record_transactions(bank.slot(), &transactions, &results, &poh_recorder);
            let (_, entries) = entry_receiver.recv().unwrap();
            assert_eq!(entries[0].0.transactions.len(), transactions.len());

            // InstructionErrors should still be recorded
            results[0] = Err(TransactionError::InstructionError(
                1,
                InstructionError::new_result_with_negative_lamports(),
            ));
            let (res, retryable) = BankingStage::record_transactions(
                bank.slot(),
                &transactions,
                &results,
                &poh_recorder,
            );
            res.unwrap();
            assert!(retryable.is_empty());
            let (_, entries) = entry_receiver.recv().unwrap();
            assert_eq!(entries[0].0.transactions.len(), transactions.len());

            // Other TransactionErrors should not be recorded
            results[0] = Err(TransactionError::AccountNotFound);
            let (res, retryable) = BankingStage::record_transactions(
                bank.slot(),
                &transactions,
                &results,
                &poh_recorder,
            );
            res.unwrap();
            assert!(retryable.is_empty());
            let (_, entries) = entry_receiver.recv().unwrap();
            assert_eq!(entries[0].0.transactions.len(), transactions.len() - 1);

            // Once bank is set to a new bank (setting bank.slot() + 1 in record_transactions),
            // record_transactions should throw MaxHeightReached and return the set of retryable
            // txs
            let (res, retryable) = BankingStage::record_transactions(
                bank.slot() + 1,
                &transactions,
                &results,
                &poh_recorder,
            );
            assert_matches!(
                res,
                Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
            );
            // The first result was an error so it's filtered out. The second result was Ok(),
            // so it should be marked as retryable
            assert_eq!(retryable, vec![1]);
            // Should receive nothing from PohRecorder b/c record failed
            assert!(entry_receiver.try_recv().is_err());
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_bank_filter_transaction_indexes() {
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let pubkey = Pubkey::new_rand();

        let transactions = vec![
            None,
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            None,
            None,
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            None,
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            None,
            Some(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            )),
            None,
            None,
        ];

        let filtered_transactions = vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
        ];

        assert_eq!(
            BankingStage::filter_transaction_indexes(
                transactions.clone(),
                &vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            ),
            (filtered_transactions.clone(), vec![1, 2, 3, 6, 8, 10])
        );

        assert_eq!(
            BankingStage::filter_transaction_indexes(
                transactions,
                &vec![1, 2, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15],
            ),
            (filtered_transactions, vec![2, 4, 5, 9, 11, 13])
        );
    }

    #[test]
    fn test_bank_prepare_filter_for_pending_transaction() {
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let pubkey = Pubkey::new_rand();

        let transactions = vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
        ];

        assert_eq!(
            BankingStage::prepare_filter_for_pending_transactions(&transactions, &vec![2, 4, 5],),
            vec![
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(())
            ]
        );

        assert_eq!(
            BankingStage::prepare_filter_for_pending_transactions(&transactions, &vec![0, 2, 3],),
            vec![
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Ok(()),
                Ok(()),
                Err(TransactionError::BlockhashNotFound),
                Err(TransactionError::BlockhashNotFound),
            ]
        );
    }

    #[test]
    fn test_bank_filter_valid_transaction_indexes() {
        assert_eq!(
            BankingStage::filter_valid_transaction_indexes(
                &vec![
                    Err(TransactionError::BlockhashNotFound),
                    Err(TransactionError::BlockhashNotFound),
                    Ok(()),
                    Err(TransactionError::BlockhashNotFound),
                    Ok(()),
                    Ok(())
                ],
                &vec![2, 4, 5, 9, 11, 13]
            ),
            vec![5, 11, 13]
        );

        assert_eq!(
            BankingStage::filter_valid_transaction_indexes(
                &vec![
                    Ok(()),
                    Err(TransactionError::BlockhashNotFound),
                    Err(TransactionError::BlockhashNotFound),
                    Ok(()),
                    Ok(()),
                    Ok(())
                ],
                &vec![1, 6, 7, 9, 31, 43]
            ),
            vec![1, 9, 31, 43]
        );
    }

    #[test]
    fn test_should_process_or_forward_packets() {
        let my_pubkey = Pubkey::new_rand();
        let my_pubkey1 = Pubkey::new_rand();

        assert_eq!(
            BankingStage::consume_or_forward_packets(&my_pubkey, None, true, false,),
            BufferedPacketsDecision::Hold
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(&my_pubkey, None, false, false),
            BufferedPacketsDecision::Hold
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(&my_pubkey1, None, false, false),
            BufferedPacketsDecision::Hold
        );

        assert_eq!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1.clone()),
                false,
                false,
            ),
            BufferedPacketsDecision::Forward
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1.clone()),
                false,
                true,
            ),
            BufferedPacketsDecision::Hold
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1.clone()),
                true,
                false,
            ),
            BufferedPacketsDecision::Consume
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1.clone()),
                false,
                false,
            ),
            BufferedPacketsDecision::Hold
        );
        assert_eq!(
            BankingStage::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1.clone()),
                true,
                false,
            ),
            BufferedPacketsDecision::Consume
        );
    }

    #[test]
    fn test_bank_process_and_record_transactions() {
        solana_logger::setup();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let pubkey = Pubkey::new_rand();

        let transactions = vec![system_transaction::create_user_account(
            &mint_keypair,
            &pubkey,
            1,
            genesis_block.hash(),
        )];

        let working_bank = WorkingBank {
            bank: bank.clone(),
            min_tick_height: bank.tick_height(),
            max_tick_height: bank.tick_height() + 1,
        };
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (poh_recorder, entry_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.slot(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let poh_recorder = Arc::new(Mutex::new(poh_recorder));

            poh_recorder.lock().unwrap().set_working_bank(working_bank);

            BankingStage::process_and_record_transactions(&bank, &transactions, &poh_recorder, 0)
                .0
                .unwrap();
            poh_recorder.lock().unwrap().tick();

            let mut done = false;
            // read entries until I find mine, might be ticks...
            while let Ok((_, entries)) = entry_receiver.recv() {
                for (entry, _) in entries {
                    if !entry.is_tick() {
                        trace!("got entry");
                        assert_eq!(entry.transactions.len(), transactions.len());
                        assert_eq!(bank.get_balance(&pubkey), 1);
                        done = true;
                    }
                }
                if done {
                    break;
                }
            }
            trace!("done ticking");

            assert_eq!(done, true);

            let transactions = vec![system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                2,
                genesis_block.hash(),
            )];

            assert_matches!(
                BankingStage::process_and_record_transactions(
                    &bank,
                    &transactions,
                    &poh_recorder,
                    0
                )
                .0,
                Err(Error::PohRecorderError(PohRecorderError::MaxHeightReached))
            );

            assert_eq!(bank.get_balance(&pubkey), 1);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_bank_process_and_record_transactions_account_in_use() {
        solana_logger::setup();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let pubkey = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();

        let transactions = vec![
            system_transaction::transfer(&mint_keypair, &pubkey, 1, genesis_block.hash()),
            system_transaction::transfer(&mint_keypair, &pubkey1, 1, genesis_block.hash()),
        ];

        let working_bank = WorkingBank {
            bank: bank.clone(),
            min_tick_height: bank.tick_height(),
            max_tick_height: bank.tick_height() + 1,
        };
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.slot(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &pubkey,
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );
            let poh_recorder = Arc::new(Mutex::new(poh_recorder));

            poh_recorder.lock().unwrap().set_working_bank(working_bank);

            let (result, unprocessed) = BankingStage::process_and_record_transactions(
                &bank,
                &transactions,
                &poh_recorder,
                0,
            );

            assert!(result.is_ok());
            assert_eq!(unprocessed.len(), 1);
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_filter_valid_packets() {
        solana_logger::setup();

        let all_packets = (0..16)
            .map(|packets_id| {
                let packets = Packets::new(
                    (0..32)
                        .map(|packet_id| {
                            let mut p = Packet::default();
                            p.meta.port = packets_id << 8 | packet_id;
                            p
                        })
                        .collect_vec(),
                );
                let valid_indexes = (0..32)
                    .filter_map(|x| if x % 2 != 0 { Some(x as usize) } else { None })
                    .collect_vec();
                (packets, valid_indexes)
            })
            .collect_vec();

        let result = BankingStage::filter_valid_packets_for_forwarding(&all_packets);

        assert_eq!(result.len(), 256);

        let _ = result
            .into_iter()
            .enumerate()
            .map(|(index, p)| {
                let packets_id = index / 16;
                let packet_id = (index % 16) * 2 + 1;
                assert_eq!(p.meta.port, (packets_id << 8 | packet_id) as u16);
            })
            .collect_vec();
    }

    #[test]
    fn test_process_transactions_returns_unprocessed_txs() {
        solana_logger::setup();
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let mut transactions = vec![];

        loop {
            let pubkey = Pubkey::new_rand();
            // Make enough transactions to span multiple entries
            transactions.push(system_transaction::transfer(
                &mint_keypair,
                &pubkey,
                1,
                genesis_block.hash(),
            ));

            if entry::num_will_fit(
                &transactions[0..],
                packet::BLOB_DATA_SIZE as u64,
                &Entry::serialized_to_blob_size,
            ) < transactions.len()
            {
                break;
            }
        }

        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree =
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger");
            let (poh_recorder, _entry_receiver) = PohRecorder::new(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.slot(),
                Some((4, 4)),
                bank.ticks_per_slot(),
                &Pubkey::new_rand(),
                &Arc::new(blocktree),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &Arc::new(PohConfig::default()),
            );

            // Poh Recorder has not working bank, so should throw MaxHeightReached error on
            // record
            let poh_recorder = Arc::new(Mutex::new(poh_recorder));

            let (processed_transactions_count, mut retryable_txs) =
                BankingStage::process_transactions(&bank, &transactions, &poh_recorder);

            assert_eq!(processed_transactions_count, 0,);

            retryable_txs.sort();
            let expected: Vec<usize> = (0..transactions.len()).collect();
            assert_eq!(retryable_txs, expected);
        }

        Blocktree::destroy(&ledger_path).unwrap();
    }
}
