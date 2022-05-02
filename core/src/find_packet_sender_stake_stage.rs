use {
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    itertools::Itertools,
    rayon::{prelude::*, ThreadPool},
    solana_gossip::cluster_info::ClusterInfo,
    solana_measure::measure::Measure,
    solana_perf::{
        packet::PacketBatch,
        sigverify::{shrink_batches, Deduper},
    },
    solana_rayon_threadlimit::get_thread_count,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::timing::timestamp,
    solana_streamer::streamer::{self, StreamerError},
    std::{
        cell::RefCell,
        collections::HashMap,
        net::IpAddr,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const IP_TO_STAKE_REFRESH_DURATION: Duration = Duration::from_secs(5);
const MAX_DEDUPER_AGE: Duration = Duration::from_secs(2);
const MAX_DEDUPER_ITEMS: u32 = 1_000_000;
const MAX_EMPTY_BATCH_RATIO: usize = 4;
const DISCARD_TARGET_PACKET_COUNT: usize = 10_000;

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .thread_name(|ix| format!("transaction_sender_stake_stage_{}", ix))
                    .build()
                    .unwrap()));

pub type FindPacketSenderStakeSender = Sender<Vec<PacketBatch>>;
pub type FindPacketSenderStakeReceiver = Receiver<Vec<PacketBatch>>;

#[derive(Debug, Default)]
struct FindPacketSenderStakeStats {
    last_print: u64,
    refresh_ip_to_stake_time: u64,
    apply_sender_stakes_time: u64,
    send_batches_time: u64,
    receive_batches_time: u64,
    dedup_time: u64,
    discard_time: u64,
    shrink_time: u64,
    total_batches: u64,
    total_packets: u64,
    total_packets_dedup: u64,
    total_packets_excess: u64,
    total_packets_sent: u64,
    total_shrinks: u64,
}

impl FindPacketSenderStakeStats {
    fn report(&mut self) {
        let now = timestamp();
        let elapsed_ms = now - self.last_print;
        if elapsed_ms > 2000 {
            datapoint_info!(
                "find_packet_sender_stake-services_stats",
                (
                    "refresh_ip_to_stake_time_us",
                    self.refresh_ip_to_stake_time as i64,
                    i64
                ),
                (
                    "apply_sender_stakes_time_us",
                    self.apply_sender_stakes_time as i64,
                    i64
                ),
                ("send_batches_time_us", self.send_batches_time as i64, i64),
                (
                    "receive_batches_time_ns",
                    self.receive_batches_time as i64,
                    i64
                ),
                ("dedup_time_us", self.dedup_time, i64),
                ("discard_time_us", self.discard_time, i64),
                ("shrink_time_us", self.shrink_time, i64),
                ("total_batches", self.total_batches as i64, i64),
                ("total_packets", self.total_packets as i64, i64),
                ("total_packets_dedup", self.total_packets_dedup, i64),
                ("total_packets_excess", self.total_packets_excess, i64),
                ("total_packets_sent", self.total_packets_sent, i64),
                ("total_shrinks", self.total_shrinks, i64),
            );
            *self = FindPacketSenderStakeStats::default();
            self.last_print = now;
        }
    }
}

struct FindPacketSenderStake {
    last_stakes: Instant,
    ip_to_stake: HashMap<IpAddr, u64>,
    bank_forks: Arc<RwLock<BankForks>>,
    cluster_info: Arc<ClusterInfo>,
    deduper: Deduper,
    stats: FindPacketSenderStakeStats,
}

impl FindPacketSenderStake {
    fn exec(
        &mut self,
        packet_receiver: &streamer::PacketBatchReceiver,
        sender: &FindPacketSenderStakeSender,
    ) -> Result<(), streamer::StreamerError> {
        self.deduper.reset();

        let mut refresh_ip_to_stake_time = Measure::start("refresh_ip_to_stake_time");
        self.try_refresh_ip_to_stake();
        refresh_ip_to_stake_time.stop();

        let (mut batches, num_packets, recv_duration) =
            streamer::recv_packet_batches(packet_receiver)?;
        let num_batches = batches.len();

        // Mark duplicate packets as discarded.
        let mut dedup_time = Measure::start("dedup_time");
        let dedup_fail = self.deduper.dedup_packets(&mut batches) as usize;
        dedup_time.stop();
        let num_unique = num_packets.saturating_sub(dedup_fail);

        // Fill packet.meta.sender_stake.
        let mut apply_sender_stakes_time = Measure::start("apply_sender_stakes_time");
        Self::apply_sender_stakes(&mut batches, &self.ip_to_stake);
        apply_sender_stakes_time.stop();

        // Discard packets early if more than the target amount remain.
        let mut discard_time = Measure::start("discard_time");
        let mut num_sent_packets = num_unique;
        if num_unique > DISCARD_TARGET_PACKET_COUNT {
            Self::discard_excess_packets(&mut batches, DISCARD_TARGET_PACKET_COUNT);
            num_sent_packets = DISCARD_TARGET_PACKET_COUNT;
        }
        let excess_fail = num_unique.saturating_sub(DISCARD_TARGET_PACKET_COUNT);
        discard_time.stop();

        // Shrink, if the previous stages significantly reduced the packet count.
        // This produces full batches at the cost of copying packet data. It also
        // releases memory.
        let mut shrink_time = Measure::start("shrink_time");
        let start_len = batches.len();
        if num_packets > num_sent_packets.saturating_mul(MAX_EMPTY_BATCH_RATIO) {
            let valid = shrink_batches(&mut batches);
            batches.truncate(valid);
        }
        let total_shrinks = start_len.saturating_sub(batches.len());
        shrink_time.stop();

        let mut send_batches_time = Measure::start("send_batches_time");
        if let Err(e) = sender.send(batches) {
            info!("Sender error: {:?}", e);
        }
        send_batches_time.stop();

        let mut stats = &mut self.stats;
        stats.refresh_ip_to_stake_time = stats
            .refresh_ip_to_stake_time
            .saturating_add(refresh_ip_to_stake_time.as_us());
        stats.apply_sender_stakes_time = stats
            .apply_sender_stakes_time
            .saturating_add(apply_sender_stakes_time.as_us());
        stats.send_batches_time = stats
            .send_batches_time
            .saturating_add(send_batches_time.as_us());
        stats.receive_batches_time = stats
            .receive_batches_time
            .saturating_add(recv_duration.as_nanos() as u64);
        stats.dedup_time = stats.dedup_time.saturating_add(dedup_time.as_us() as u64);
        stats.discard_time = stats
            .discard_time
            .saturating_add(discard_time.as_us() as u64);
        stats.shrink_time = stats.shrink_time.saturating_add(shrink_time.as_us() as u64);
        stats.total_batches = stats.total_batches.saturating_add(num_batches as u64);
        stats.total_packets = stats.total_packets.saturating_add(num_packets as u64);
        stats.total_packets_dedup = stats.total_packets_dedup.saturating_add(dedup_fail as u64);
        stats.total_packets_excess = stats
            .total_packets_excess
            .saturating_add(excess_fail as u64);
        stats.total_packets_sent = stats
            .total_packets_sent
            .saturating_add(num_sent_packets as u64);
        stats.total_shrinks = stats.total_shrinks.saturating_add(total_shrinks as u64);

        Ok(())
    }

    fn discard_excess_packets(batches: &mut [PacketBatch], mut max_packets: usize) {
        // TODO: let sender_stake influence the discard decision

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

    fn try_refresh_ip_to_stake(&mut self) {
        if self.last_stakes.elapsed() > IP_TO_STAKE_REFRESH_DURATION {
            let root_bank = self.bank_forks.read().unwrap().root_bank();
            let staked_nodes = root_bank.staked_nodes();
            self.ip_to_stake = self
                .cluster_info
                .tvu_peers()
                .into_iter()
                .filter_map(|node| {
                    let stake = staked_nodes.get(&node.id)?;
                    Some((node.tvu.ip(), *stake))
                })
                .collect();
            self.last_stakes = Instant::now();
        }
    }

    fn apply_sender_stakes(batches: &mut [PacketBatch], ip_to_stake: &HashMap<IpAddr, u64>) {
        PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                batches
                    .into_par_iter()
                    .flat_map(|batch| batch.packets.par_iter_mut())
                    .for_each(|packet| {
                        if !packet.meta.discard() {
                            packet.meta.sender_stake =
                                *ip_to_stake.get(&packet.meta.addr().ip()).unwrap_or(&0);
                        }
                    });
            })
        });
    }
}

pub struct FindPacketSenderStakeStage {
    thread_hdl: JoinHandle<()>,
}

impl FindPacketSenderStakeStage {
    pub fn new(
        packet_receiver: streamer::PacketBatchReceiver,
        sender: FindPacketSenderStakeSender,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("find-packet-sender-stake".to_string())
            .spawn(move || {
                let mut job = FindPacketSenderStake {
                    last_stakes: Instant::now(),
                    ip_to_stake: HashMap::new(),
                    bank_forks,
                    cluster_info,
                    deduper: Deduper::new(MAX_DEDUPER_ITEMS, MAX_DEDUPER_AGE),
                    stats: FindPacketSenderStakeStats::default(),
                };
                loop {
                    if let Err(e) = job.exec(&packet_receiver, &sender) {
                        match e {
                            StreamerError::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                            StreamerError::RecvTimeout(RecvTimeoutError::Timeout) => (),
                            _ => error!("error: {:?}", e),
                        };
                    }

                    job.stats.report();
                }
            })
            .unwrap();
        Self { thread_hdl }
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
        FindPacketSenderStake::discard_excess_packets(&mut batches, max);
        assert_eq!(count_non_discard(&batches), max);
        assert!(!batches[0].packets[0].meta.discard());
        assert!(batches[0].packets[3].meta.discard());
        assert!(!batches[0].packets[4].meta.discard());
    }
}
