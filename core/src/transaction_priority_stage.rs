use crossbeam_channel::{Receiver as CrossbeamReceiver, RecvTimeoutError};
use rayon::*;
use solana_measure::measure::Measure;
use solana_perf::packet::limited_deserialize;
use solana_perf::packet::{Packet, Packets};
use solana_sdk::short_vec::decode_shortu16_len;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{
    feature_set, message::Message, signature::Signature, transaction::SanitizedTransaction,
};
use std::{
    collections::{BTreeMap, HashMap},
    mem::size_of,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::{self, Builder, JoinHandle},
};

pub struct PrioritizedPackets {
    packets: HashMap<usize, Vec<Packets>>,
    transactions: HashMap<usize, Vec<SanitizedTransaction>>,
    priorities: BTreeMap<usize, (usize, usize)>,
}

pub struct TransactionPriorityStage {
    thread: JoinHandle<()>,
}

impl TransactionPriorityStage {
    pub fn new(
        verified_receiver: CrossbeamReceiver<Vec<Packets>>,
        prioritized_packets: Arc<RwLock<PrioritizedPackets>>,
        exit: Arc<AtomicBool>,
        cost_tracker: Arc<RwLock<CostTracker>>,
    ) -> Self {
        let thread = Builder::new()
            .name("sol-tx-pri".to_string())
            .spawn(move || {
                let mut packets_id = 0;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(mmsgs) = verified_receiver.recv() {
                        let vec_of_transactions = mmsgs
                            .par_iter()
                            .map(|msgs| {
                                let (
                                    transactions,
                                    transaction_to_packet_indexes,
                                    retryable_packet_indexes,
                                ) = Self::transactions_from_packets(
                                    msgs,
                                    &packet_indexes,
                                    &bank.feature_set,
                                    &cost_tracker,
                                    banking_stage_stats,
                                    bank.demote_program_write_locks(),
                                    bank.vote_only_bank(),
                                );
                                transactions
                            })
                            .collect();
                        let mut prioritized_packets = prioritized_packets.write().unwrap();
                        for (transactions, packets) in
                            vec_of_transactions.into_iter().zip(mmsgs.into_iter())
                        {
                            packets_id += 1;
                            prioritized_packets
                                .transactions
                                .insert(packets_id, transactions);
                            prioritized_packets.packets.insert(packets_id, transactions);
                        }
                    }
                }
            })
            .unwrap();

        Self { thread }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }

    // This function deserializes packets into transactions, computes the blake3 hash of transaction messages,
    // and verifies secp256k1 instructions. A list of valid transactions are returned with their message hashes
    // and packet indexes.
    // Also returned is packet indexes for transaction should be retried due to cost limits.
    #[allow(clippy::needless_collect)]
    fn transactions_from_packets(
        msgs: &Packets,
        transaction_indexes: &[usize],
        feature_set: &Arc<feature_set::FeatureSet>,
        cost_tracker: &Arc<RwLock<CostTracker>>,
        banking_stage_stats: &BankingStageStats,
        demote_program_write_locks: bool,
        votes_only: bool,
    ) -> (Vec<SanitizedTransaction>, Vec<usize>, Vec<usize>) {
        let mut retryable_transaction_packet_indexes: Vec<usize> = vec![];

        let verified_transactions_with_packet_indexes: Vec<_> = transaction_indexes
            .iter()
            .filter_map(|tx_index| {
                let p = &msgs.packets[*tx_index];
                let tx: VersionedTransaction = limited_deserialize(&p.data[0..p.meta.size]).ok()?;
                let message_bytes = Self::packet_message(p)?;
                let message_hash = Message::hash_raw_message(message_bytes);
                let tx = SanitizedTransaction::try_create(tx, message_hash, |_| {
                    Err(TransactionError::UnsupportedVersion)
                })
                .ok()?;
                if votes_only && !solana_runtime::bank::is_simple_vote_transaction(&tx) {
                    return None;
                }
                tx.verify_precompiles(feature_set).ok()?;
                Some((tx, *tx_index))
            })
            .collect();
        banking_stage_stats.cost_tracker_check_count.fetch_add(
            verified_transactions_with_packet_indexes.len(),
            Ordering::Relaxed,
        );

        let mut cost_tracker_check_time = Measure::start("cost_tracker_check_time");
        let (filtered_transactions, filter_transaction_packet_indexes) = {
            let cost_tracker_readonly = cost_tracker.read().unwrap();
            verified_transactions_with_packet_indexes
                .into_iter()
                .filter_map(|(tx, tx_index)| {
                    let result = cost_tracker_readonly
                        .would_transaction_fit(&tx, demote_program_write_locks);
                    if result.is_err() {
                        debug!("transaction {:?} would exceed limit: {:?}", tx, result);
                        retryable_transaction_packet_indexes.push(tx_index);
                        return None;
                    }
                    Some((tx, tx_index))
                })
                .unzip()
        };
        cost_tracker_check_time.stop();

        banking_stage_stats
            .cost_tracker_check_elapsed
            .fetch_add(cost_tracker_check_time.as_us(), Ordering::Relaxed);

        (
            filtered_transactions,
            filter_transaction_packet_indexes,
            retryable_transaction_packet_indexes,
        )
    }

    /// Read the transaction message from packet data
    fn packet_message(packet: &Packet) -> Option<&[u8]> {
        let (sig_len, sig_size) = decode_shortu16_len(&packet.data).ok()?;
        let msg_start = sig_len
            .checked_mul(size_of::<Signature>())
            .and_then(|v| v.checked_add(sig_size))?;
        let msg_end = packet.meta.size;
        Some(&packet.data[msg_start..msg_end])
    }
}
