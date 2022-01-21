//! The `sigverify_stage` implements the signature verification stage of the TPU. It
//! receives a list of lists of packets and outputs the same list, but tags each
//! top-level list with a list of booleans, telling the next stage whether the
//! signature in that packet is valid. It assumes each packet contains one
//! transaction. All processing is done on the CPU by default and on a GPU
//! if perf-libs are available

use {
    crate::sigverify,
    core::time::Duration,
    crossbeam_channel::{Receiver, RecvTimeoutError, SendError, Sender},
    itertools::Itertools,
    solana_gossip::cluster_info::ClusterInfo,
    solana_measure::measure::Measure,
    solana_perf::data_budget::DataBudget,
    solana_perf::packet::PacketBatch,
    solana_perf::qos::{qos_packets, Qos, StakeMap},
    solana_perf::sigverify::Deduper,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::timing,
    solana_streamer::streamer::{self, PacketBatchReceiver, StreamerError},
    std::{
        collections::hash_map::Entry,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
        time::Instant,
    },
    thiserror::Error,
};

pub const MAX_SIGVERIFY_BATCH: usize = 10_000;

#[derive(Error, Debug)]
pub enum SigVerifyServiceError {
    #[error("send packets batch error")]
    Send(#[from] SendError<Vec<PacketBatch>>),

    #[error("streamer error")]
    Streamer(#[from] StreamerError),
}

type Result<T> = std::result::Result<T, SigVerifyServiceError>;

pub struct SigVerifyStage {
    thread_hdl: JoinHandle<()>,
}

pub trait SigVerifier {
    fn verify_batches(&self, batches: Vec<PacketBatch>) -> Vec<PacketBatch>;
}

#[derive(Default, Clone)]
pub struct DisabledSigVerifier {}

#[derive(Default)]
struct SigVerifierStats {
    recv_batches_us_hist: histogram::Histogram, // time to call recv_batch
    verify_batches_pp_us_hist: histogram::Histogram, // per-packet time to call verify_batch
    discard_packets_pp_us_hist: histogram::Histogram, // per-packet time to call verify_batch
    dedup_packets_pp_us_hist: histogram::Histogram, // per-packet time to call verify_batch
    batches_hist: histogram::Histogram,         // number of packet batches per verify call
    packets_hist: histogram::Histogram,         // number of packets per verify call
    total_batches: usize,
    total_packets: usize,
    total_dedup: usize,
    total_excess_fail: usize,
}

impl SigVerifierStats {
    fn report(&self, name: &'static str) {
        datapoint_info!(
            name,
            (
                "recv_batches_us_90pct",
                self.recv_batches_us_hist.percentile(90.0).unwrap_or(0),
                i64
            ),
            (
                "recv_batches_us_min",
                self.recv_batches_us_hist.minimum().unwrap_or(0),
                i64
            ),
            (
                "recv_batches_us_max",
                self.recv_batches_us_hist.maximum().unwrap_or(0),
                i64
            ),
            (
                "recv_batches_us_mean",
                self.recv_batches_us_hist.mean().unwrap_or(0),
                i64
            ),
            (
                "verify_batches_pp_us_90pct",
                self.verify_batches_pp_us_hist.percentile(90.0).unwrap_or(0),
                i64
            ),
            (
                "verify_batches_pp_us_min",
                self.verify_batches_pp_us_hist.minimum().unwrap_or(0),
                i64
            ),
            (
                "verify_batches_pp_us_max",
                self.verify_batches_pp_us_hist.maximum().unwrap_or(0),
                i64
            ),
            (
                "verify_batches_pp_us_mean",
                self.verify_batches_pp_us_hist.mean().unwrap_or(0),
                i64
            ),
            (
                "discard_packets_pp_us_90pct",
                self.discard_packets_pp_us_hist
                    .percentile(90.0)
                    .unwrap_or(0),
                i64
            ),
            (
                "discard_packets_pp_us_min",
                self.discard_packets_pp_us_hist.minimum().unwrap_or(0),
                i64
            ),
            (
                "discard_packets_pp_us_max",
                self.discard_packets_pp_us_hist.maximum().unwrap_or(0),
                i64
            ),
            (
                "discard_packets_pp_us_mean",
                self.discard_packets_pp_us_hist.mean().unwrap_or(0),
                i64
            ),
            (
                "dedup_packets_pp_us_90pct",
                self.dedup_packets_pp_us_hist.percentile(90.0).unwrap_or(0),
                i64
            ),
            (
                "dedup_packets_pp_us_min",
                self.dedup_packets_pp_us_hist.minimum().unwrap_or(0),
                i64
            ),
            (
                "dedup_packets_pp_us_max",
                self.dedup_packets_pp_us_hist.maximum().unwrap_or(0),
                i64
            ),
            (
                "dedup_packets_pp_us_mean",
                self.dedup_packets_pp_us_hist.mean().unwrap_or(0),
                i64
            ),
            (
                "batches_90pct",
                self.batches_hist.percentile(90.0).unwrap_or(0),
                i64
            ),
            ("batches_min", self.batches_hist.minimum().unwrap_or(0), i64),
            ("batches_max", self.batches_hist.maximum().unwrap_or(0), i64),
            ("batches_mean", self.batches_hist.mean().unwrap_or(0), i64),
            (
                "packets_90pct",
                self.packets_hist.percentile(90.0).unwrap_or(0),
                i64
            ),
            ("packets_min", self.packets_hist.minimum().unwrap_or(0), i64),
            ("packets_max", self.packets_hist.maximum().unwrap_or(0), i64),
            ("packets_mean", self.packets_hist.mean().unwrap_or(0), i64),
            ("total_batches", self.total_batches, i64),
            ("total_packets", self.total_packets, i64),
            ("total_dedup", self.total_dedup, i64),
            ("total_excess_fail", self.total_excess_fail, i64),
        );
    }
}

impl SigVerifier for DisabledSigVerifier {
    fn verify_batches(&self, mut batches: Vec<PacketBatch>) -> Vec<PacketBatch> {
        sigverify::ed25519_verify_disabled(&mut batches);
        batches
    }
}

impl SigVerifyStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: Receiver<PacketBatch>,
        verified_sender: Sender<Vec<PacketBatch>>,
        verifier: T,
        max_batch_size: usize,
        max_packet_throughput: Option<usize>,
        name: &'static str,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<ClusterInfo>,
    ) -> Self {
        let thread_hdl = Self::verifier_services(
            packet_receiver,
            verified_sender,
            verifier,
            max_batch_size,
            max_packet_throughput,
            name,
            bank_forks,
            cluster_info,
        );
        Self { thread_hdl }
    }

    pub fn discard_excess_packets(batches: &mut [PacketBatch], mut max_packets: usize) {
        // Group packets by their incoming IP address.
        let mut addrs = batches
            .iter_mut()
            .rev()
            .flat_map(|batch| batch.packets.iter_mut().rev())
            .filter(|packet| !packet.meta.discard())
            .map(|packet| (packet.meta.addr, packet))
            .into_group_map();
        // Allocate max_packets evenly across addresses.
        while max_packets > 0 && !addrs.is_empty() {
            let num_addrs = addrs.len();
            addrs.retain(|_, packets| {
                let cap = (max_packets + num_addrs - 1) / num_addrs;
                max_packets -= packets.len().min(cap);
                packets.truncate(packets.len().saturating_sub(cap));
                !packets.is_empty()
            });
        }
        // Discard excess packets from each address.
        for packet in addrs.into_values().flatten() {
            packet.meta.set_discard(true);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn verifier<T: SigVerifier>(
        deduper: &Deduper,
        recvr: &PacketBatchReceiver,
        sendr: &Sender<Vec<PacketBatch>>,
        verifier: &T,
        stats: &mut SigVerifierStats,
        max_batch_size: usize,
        max_packet_throughput: Option<usize>,
        stake_map: &StakeMap,
        total_stake: u64,
        qos: &Qos,
    ) -> Result<()> {
        let (mut batches, num_packets, recv_duration) = streamer::recv_packet_batches(recvr)?;

        let batches_len = batches.len();
        debug!(
            "@{:?} verifier: verifying: {}",
            timing::timestamp(),
            num_packets,
        );

        let mut valid_packets = num_packets;
        let mut discard_time = Measure::start("sigverify_discard_time");
        if max_packet_throughput.is_some() {
            let qos_fail = qos_packets(qos, total_stake, stake_map, &mut batches);
            valid_packets = valid_packets.saturating_sub(qos_fail);
        }

        let mut dedup_time = Measure::start("sigverify_dedup_time");
        let dedup_fail = deduper.dedup_packets(&mut batches) as usize;
        dedup_time.stop();
        valid_packets = valid_packets.saturating_sub(dedup_fail);

        if valid_packets > max_batch_size {
            Self::discard_excess_packets(&mut batches, max_batch_size);
        };
        let excess_fail = valid_packets.saturating_sub(max_batch_size);
        discard_time.stop();

        let mut verify_batch_time = Measure::start("sigverify_batch_time");
        let batches = verifier.verify_batches(batches);
        sendr.send(batches)?;
        verify_batch_time.stop();

        debug!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} verified: {} v/s {}",
            timing::timestamp(),
            batches_len,
            verify_batch_time.as_ms(),
            num_packets,
            (num_packets as f32 / verify_batch_time.as_s())
        );

        stats
            .recv_batches_us_hist
            .increment(recv_duration.as_micros() as u64)
            .unwrap();
        stats
            .verify_batches_pp_us_hist
            .increment(verify_batch_time.as_us() / (num_packets as u64))
            .unwrap();
        stats
            .discard_packets_pp_us_hist
            .increment(discard_time.as_us() / (num_packets as u64))
            .unwrap();
        stats
            .dedup_packets_pp_us_hist
            .increment(dedup_time.as_us() / (num_packets as u64))
            .unwrap();
        stats.batches_hist.increment(batches_len as u64).unwrap();
        stats.packets_hist.increment(num_packets as u64).unwrap();
        stats.total_batches += batches_len;
        stats.total_packets += num_packets;
        stats.total_dedup += dedup_fail;
        stats.total_excess_fail += excess_fail;

        Ok(())
    }

    fn verifier_service<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: PacketBatchReceiver,
        verified_sender: Sender<Vec<PacketBatch>>,
        verifier: &T,
        max_batch_size: usize,
        max_packet_throughput: Option<usize>,
        name: &'static str,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<ClusterInfo>,
    ) -> JoinHandle<()> {
        let verifier = verifier.clone();
        let mut stats = SigVerifierStats::default();
        let mut last_print = Instant::now();
        const MAX_DEDUPER_AGE: Duration = Duration::from_secs(2);
        const MAX_DEDUPER_ITEMS: u32 = 1_000_000;

        let bank_forks = bank_forks.clone();
        let cluster_info = cluster_info.clone();
        const MAX_QOS_ITEMS: usize = 1_000_000;
        const MAX_STAKES_AGE: Duration = Duration::from_secs(2);
        Builder::new()
            .name("solana-verifier".to_string())
            .spawn(move || {
                let mut deduper = Deduper::new(MAX_DEDUPER_ITEMS, MAX_DEDUPER_AGE);
                let mut last_stakes_update = Instant::now();
                let mut total_stake = 1;
                let mut stake_map = StakeMap::default();
                let qos = Qos::new(MAX_QOS_ITEMS, max_packet_throughput.unwrap_or(usize::MAX));
                loop {
                    deduper.reset();
                    let now = Instant::now();
                    if max_packet_throughput.is_some()
                        && now.duration_since(last_stakes_update) > MAX_STAKES_AGE
                    {
                        let root_bank = bank_forks.read().unwrap().root_bank();
                        let staked_nodes = root_bank.staked_nodes();
                        total_stake = staked_nodes.values().sum();
                        for node in cluster_info.tvu_peers() {
                            if let Some(stake) = staked_nodes.get(&node.id) {
                                match stake_map.entry(node.tvu.ip()) {
                                    Entry::Occupied(mut e) => {
                                        e.get_mut().0 = *stake;
                                    }
                                    Entry::Vacant(e) => {
                                        e.insert((*stake, DataBudget::default()));
                                    }
                                }
                            }
                        }
                        let now_ts = solana_sdk::timing::timestamp();
                        stake_map.retain(|_key, budget| {
                            now_ts.saturating_sub(budget.1.last_update()) > 10_000
                        });
                        last_stakes_update = now;
                    }
                    if let Err(e) = Self::verifier(
                        &deduper,
                        &packet_receiver,
                        &verified_sender,
                        &verifier,
                        &mut stats,
                        max_batch_size,
                        max_packet_throughput,
                        &stake_map,
                        total_stake,
                        &qos,
                    ) {
                        match e {
                            SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
                                RecvTimeoutError::Disconnected,
                            )) => break,
                            SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
                                RecvTimeoutError::Timeout,
                            )) => (),
                            SigVerifyServiceError::Send(_) => {
                                break;
                            }
                            _ => error!("{:?}", e),
                        }
                    }
                    if last_print.elapsed().as_secs() > 2 {
                        stats.report(name);
                        stats = SigVerifierStats::default();
                        last_print = Instant::now();
                    }
                }
            })
            .unwrap()
    }

    fn verifier_services<T: SigVerifier + 'static + Send + Clone>(
        packet_receiver: PacketBatchReceiver,
        verified_sender: Sender<Vec<PacketBatch>>,
        verifier: T,
        max_batch_size: usize,
        max_packet_throughput: Option<usize>,
        name: &'static str,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<ClusterInfo>,
    ) -> JoinHandle<()> {
        Self::verifier_service(
            packet_receiver,
            verified_sender,
            &verifier,
            max_batch_size,
            max_packet_throughput,
            name,
            bank_forks,
            cluster_info,
        )
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_perf::packet::Packet};

    fn count_non_discard(packet_batches: &[PacketBatch]) -> usize {
        packet_batches
            .iter()
            .map(|batch| {
                batch
                    .packets
                    .iter()
                    .map(|p| if p.meta.discard() { 0 } else { 1 })
                    .sum::<usize>()
            })
            .sum::<usize>()
    }

    #[test]
    fn test_packet_discard() {
        solana_logger::setup();
        let mut batch = PacketBatch::default();
        batch.packets.resize(10, Packet::default());
        batch.packets[3].meta.addr = std::net::IpAddr::from([1u16; 8]);
        batch.packets[3].meta.set_discard(true);
        batch.packets[4].meta.addr = std::net::IpAddr::from([2u16; 8]);
        let mut batches = vec![batch];
        let max = 3;
        SigVerifyStage::discard_excess_packets(&mut batches, max);
        assert_eq!(count_non_discard(&batches), max);
        assert!(!batches[0].packets[0].meta.discard());
        assert!(batches[0].packets[3].meta.discard());
        assert!(!batches[0].packets[4].meta.discard());
    }
}
