//! The `retransmit_stage` retransmits shreds between validators
#![allow(clippy::rc_buffer)]

use {
    crate::{
        cluster_nodes::{self, ClusterNodes, ClusterNodesCache, MAX_NUM_TURBINE_HOPS},
        packet_hasher::PacketHasher,
    },
    crossbeam_channel::{Receiver, RecvTimeoutError},
    itertools::{izip, Itertools},
    lru::LruCache,
    rayon::{prelude::*, ThreadPool, ThreadPoolBuilder},
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_ledger::{
        leader_schedule_cache::LeaderScheduleCache,
        shred::{self, ShredId},
    },
    solana_measure::measure::Measure,
    solana_rayon_threadlimit::get_thread_count,
    solana_rpc::{max_slots::MaxSlots, rpc_subscriptions::RpcSubscriptions},
    solana_rpc_client_api::response::SlotUpdate,
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{clock::Slot, pubkey::Pubkey, timing::timestamp},
    solana_streamer::{
        sendmmsg::{multi_target_send, SendPktsError},
        socket::SocketAddrSpace,
    },
    std::{
        collections::HashMap,
        iter::repeat,
        net::UdpSocket,
        ops::AddAssign,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const MAX_DUPLICATE_COUNT: usize = 2;
const DEFAULT_LRU_SIZE: usize = 10_000;
// Minimum number of shreds to use rayon parallel iterators.
const PAR_ITER_MIN_NUM_SHREDS: usize = 2;

const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;
const CLUSTER_NODES_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RetransmitSlotStats {
    asof: u64,   // Latest timestamp struct was updated.
    outset: u64, // 1st shred retransmit timestamp.
    // Number of shreds sent and received at different
    // distances from the turbine broadcast root.
    num_shreds_received: [usize; MAX_NUM_TURBINE_HOPS],
    num_shreds_sent: [usize; MAX_NUM_TURBINE_HOPS],
}

struct RetransmitStats {
    since: Instant,
    num_nodes: AtomicUsize,
    num_addrs_failed: AtomicUsize,
    num_shreds: usize,
    num_shreds_skipped: usize,
    num_small_batches: usize,
    total_batches: usize,
    total_time: u64,
    epoch_fetch: u64,
    epoch_cache_update: u64,
    retransmit_total: AtomicU64,
    compute_turbine_peers_total: AtomicU64,
    slot_stats: LruCache<Slot, RetransmitSlotStats>,
    unknown_shred_slot_leader: usize,
}

impl RetransmitStats {
    fn maybe_submit(
        &mut self,
        root_bank: &Bank,
        working_bank: &Bank,
        cluster_info: &ClusterInfo,
        cluster_nodes_cache: &ClusterNodesCache<RetransmitStage>,
    ) {
        const SUBMIT_CADENCE: Duration = Duration::from_secs(2);
        if self.since.elapsed() < SUBMIT_CADENCE {
            return;
        }
        cluster_nodes_cache
            .get(root_bank.slot(), root_bank, working_bank, cluster_info)
            .submit_metrics("cluster_nodes_retransmit", timestamp());
        datapoint_info!(
            "retransmit-stage",
            ("total_time", self.total_time, i64),
            ("epoch_fetch", self.epoch_fetch, i64),
            ("epoch_cache_update", self.epoch_cache_update, i64),
            ("total_batches", self.total_batches, i64),
            ("num_small_batches", self.num_small_batches, i64),
            ("num_nodes", *self.num_nodes.get_mut(), i64),
            ("num_addrs_failed", *self.num_addrs_failed.get_mut(), i64),
            ("num_shreds", self.num_shreds, i64),
            ("num_shreds_skipped", self.num_shreds_skipped, i64),
            ("retransmit_total", *self.retransmit_total.get_mut(), i64),
            (
                "compute_turbine",
                *self.compute_turbine_peers_total.get_mut(),
                i64
            ),
            (
                "unknown_shred_slot_leader",
                self.unknown_shred_slot_leader,
                i64
            ),
        );
        // slot_stats are submited at a different cadence.
        let old = std::mem::replace(self, Self::new(Instant::now()));
        self.slot_stats = old.slot_stats;
    }
}

// Map of shred (slot, index, type) => list of hash values seen for that key.
type ShredFilter = LruCache<ShredId, Vec<u64>>;

// Returns true if shred is already received and should skip retransmit.
fn should_skip_retransmit(
    key: ShredId,
    shred: &[u8],
    shreds_received: &mut ShredFilter,
    packet_hasher: &PacketHasher,
) -> bool {
    match shreds_received.get_mut(&key) {
        Some(sent) if sent.len() >= MAX_DUPLICATE_COUNT => true,
        Some(sent) => {
            let hash = packet_hasher.hash_shred(shred);
            if sent.contains(&hash) {
                true
            } else {
                sent.push(hash);
                false
            }
        }
        None => {
            let hash = packet_hasher.hash_shred(shred);
            shreds_received.put(key, vec![hash]);
            false
        }
    }
}

fn maybe_reset_shreds_received_cache(
    shreds_received: &mut ShredFilter,
    packet_hasher: &mut PacketHasher,
    hasher_reset_ts: &mut Instant,
) {
    const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
    if hasher_reset_ts.elapsed() >= UPDATE_INTERVAL {
        *hasher_reset_ts = Instant::now();
        shreds_received.clear();
        packet_hasher.reset();
    }
}

#[allow(clippy::too_many_arguments)]
fn retransmit(
    thread_pool: &ThreadPool,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    cluster_info: &ClusterInfo,
    shreds_receiver: &Receiver<Vec</*shred:*/ Vec<u8>>>,
    sockets: &[UdpSocket],
    stats: &mut RetransmitStats,
    cluster_nodes_cache: &ClusterNodesCache<RetransmitStage>,
    hasher_reset_ts: &mut Instant,
    shreds_received: &mut ShredFilter,
    packet_hasher: &mut PacketHasher,
    max_slots: &MaxSlots,
    rpc_subscriptions: Option<&RpcSubscriptions>,
) -> Result<(), RecvTimeoutError> {
    const RECV_TIMEOUT: Duration = Duration::from_secs(1);
    let mut shreds = shreds_receiver.recv_timeout(RECV_TIMEOUT)?;
    let mut timer_start = Measure::start("retransmit");
    shreds.extend(shreds_receiver.try_iter().flatten());
    stats.num_shreds += shreds.len();
    stats.total_batches += 1;

    let mut epoch_fetch = Measure::start("retransmit_epoch_fetch");
    let (working_bank, root_bank) = {
        let bank_forks = bank_forks.read().unwrap();
        (bank_forks.working_bank(), bank_forks.root_bank())
    };
    epoch_fetch.stop();
    stats.epoch_fetch += epoch_fetch.as_us();

    let mut epoch_cache_update = Measure::start("retransmit_epoch_cache_update");
    maybe_reset_shreds_received_cache(shreds_received, packet_hasher, hasher_reset_ts);
    epoch_cache_update.stop();
    stats.epoch_cache_update += epoch_cache_update.as_us();
    // Lookup slot leader and cluster nodes for each slot.
    let shreds: Vec<_> = shreds
        .into_iter()
        .filter_map(|shred| {
            let key = shred::layout::get_shred_id(&shred)?;
            if should_skip_retransmit(key, &shred, shreds_received, packet_hasher) {
                stats.num_shreds_skipped += 1;
                None
            } else {
                Some((key, shred))
            }
        })
        .into_group_map_by(|(key, _shred)| key.slot())
        .into_iter()
        .filter_map(|(slot, shreds)| {
            max_slots.retransmit.fetch_max(slot, Ordering::Relaxed);
            // TODO: consider using root-bank here for leader lookup!
            // Shreds' signatures should be verified before they reach here,
            // and if the leader is unknown they should fail signature check.
            // So here we should expect to know the slot leader and otherwise
            // skip the shred.
            let slot_leader = match leader_schedule_cache.slot_leader_at(slot, Some(&working_bank))
            {
                Some(pubkey) => pubkey,
                None => {
                    stats.unknown_shred_slot_leader += shreds.len();
                    return None;
                }
            };
            let cluster_nodes =
                cluster_nodes_cache.get(slot, &root_bank, &working_bank, cluster_info);
            Some(izip!(shreds, repeat(slot_leader), repeat(cluster_nodes)))
        })
        .flatten()
        .collect();
    let socket_addr_space = cluster_info.socket_addr_space();
    let record = |mut stats: HashMap<Slot, RetransmitSlotStats>,
                  (slot, root_distance, num_nodes)| {
        let now = timestamp();
        let entry = stats.entry(slot).or_default();
        entry.record(now, root_distance, num_nodes);
        stats
    };
    let slot_stats = if shreds.len() < PAR_ITER_MIN_NUM_SHREDS {
        stats.num_small_batches += 1;
        shreds
            .into_iter()
            .enumerate()
            .map(|(index, ((key, shred), slot_leader, cluster_nodes))| {
                let (root_distance, num_nodes) = retransmit_shred(
                    &key,
                    &shred,
                    &slot_leader,
                    &root_bank,
                    &cluster_nodes,
                    socket_addr_space,
                    &sockets[index % sockets.len()],
                    stats,
                );
                (key.slot(), root_distance, num_nodes)
            })
            .fold(HashMap::new(), record)
    } else {
        thread_pool.install(|| {
            shreds
                .into_par_iter()
                .map(|((key, shred), slot_leader, cluster_nodes)| {
                    let index = thread_pool.current_thread_index().unwrap();
                    let (root_distance, num_nodes) = retransmit_shred(
                        &key,
                        &shred,
                        &slot_leader,
                        &root_bank,
                        &cluster_nodes,
                        socket_addr_space,
                        &sockets[index % sockets.len()],
                        stats,
                    );
                    (key.slot(), root_distance, num_nodes)
                })
                .fold(HashMap::new, record)
                .reduce(HashMap::new, RetransmitSlotStats::merge)
        })
    };
    stats.upsert_slot_stats(slot_stats, root_bank.slot(), rpc_subscriptions);
    timer_start.stop();
    stats.total_time += timer_start.as_us();
    stats.maybe_submit(&root_bank, &working_bank, cluster_info, cluster_nodes_cache);
    Ok(())
}

fn retransmit_shred(
    key: &ShredId,
    shred: &[u8],
    slot_leader: &Pubkey,
    root_bank: &Bank,
    cluster_nodes: &ClusterNodes<RetransmitStage>,
    socket_addr_space: &SocketAddrSpace,
    socket: &UdpSocket,
    stats: &RetransmitStats,
) -> (/*root_distance:*/ usize, /*num_nodes:*/ usize) {
    let mut compute_turbine_peers = Measure::start("turbine_start");
    let data_plane_fanout = cluster_nodes::get_data_plane_fanout(key.slot(), root_bank);
    let (root_distance, addrs) =
        cluster_nodes.get_retransmit_addrs(slot_leader, key, root_bank, data_plane_fanout);
    let addrs: Vec<_> = addrs
        .into_iter()
        .filter(|addr| ContactInfo::is_valid_address(addr, socket_addr_space))
        .collect();
    compute_turbine_peers.stop();
    stats
        .compute_turbine_peers_total
        .fetch_add(compute_turbine_peers.as_us(), Ordering::Relaxed);

    let mut retransmit_time = Measure::start("retransmit_to");
    let num_nodes = match multi_target_send(socket, shred, &addrs) {
        Ok(()) => addrs.len(),
        Err(SendPktsError::IoError(ioerr, num_failed)) => {
            stats
                .num_addrs_failed
                .fetch_add(num_failed, Ordering::Relaxed);
            error!(
                "retransmit_to multi_target_send error: {:?}, {}/{} packets failed",
                ioerr,
                num_failed,
                addrs.len(),
            );
            addrs.len() - num_failed
        }
    };
    retransmit_time.stop();
    stats.num_nodes.fetch_add(num_nodes, Ordering::Relaxed);
    stats
        .retransmit_total
        .fetch_add(retransmit_time.as_us(), Ordering::Relaxed);
    (root_distance, num_nodes)
}

/// Service to retransmit messages from the leader or layer 1 to relevant peer nodes.
/// See `cluster_info` for network layer definitions.
/// # Arguments
/// * `sockets` - Sockets to read from.
/// * `bank_forks` - The BankForks structure
/// * `leader_schedule_cache` - The leader schedule to verify shreds
/// * `cluster_info` - This structure needs to be updated and populated by the bank and via gossip.
/// * `r` - Receive channel for shreds to be retransmitted to all the layer 1 nodes.
pub fn retransmitter(
    sockets: Arc<Vec<UdpSocket>>,
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    cluster_info: Arc<ClusterInfo>,
    shreds_receiver: Receiver<Vec</*shred:*/ Vec<u8>>>,
    max_slots: Arc<MaxSlots>,
    rpc_subscriptions: Option<Arc<RpcSubscriptions>>,
) -> JoinHandle<()> {
    let cluster_nodes_cache = ClusterNodesCache::<RetransmitStage>::new(
        CLUSTER_NODES_CACHE_NUM_EPOCH_CAP,
        CLUSTER_NODES_CACHE_TTL,
    );
    let mut hasher_reset_ts = Instant::now();
    let mut stats = RetransmitStats::new(Instant::now());
    let mut shreds_received = LruCache::<ShredId, _>::new(DEFAULT_LRU_SIZE);
    let mut packet_hasher = PacketHasher::default();
    #[allow(clippy::manual_clamp)]
    let num_threads = get_thread_count().min(8).max(sockets.len());
    let thread_pool = ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .thread_name(|i| format!("solRetransmit{i:02}"))
        .build()
        .unwrap();
    Builder::new()
        .name("solRetransmittr".to_string())
        .spawn(move || loop {
            match retransmit(
                &thread_pool,
                &bank_forks,
                &leader_schedule_cache,
                &cluster_info,
                &shreds_receiver,
                &sockets,
                &mut stats,
                &cluster_nodes_cache,
                &mut hasher_reset_ts,
                &mut shreds_received,
                &mut packet_hasher,
                &max_slots,
                rpc_subscriptions.as_deref(),
            ) {
                Ok(()) => (),
                Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        })
        .unwrap()
}

pub struct RetransmitStage {
    retransmit_thread_handle: JoinHandle<()>,
}

impl RetransmitStage {
    pub(crate) fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        cluster_info: Arc<ClusterInfo>,
        retransmit_sockets: Arc<Vec<UdpSocket>>,
        retransmit_receiver: Receiver<Vec</*shred:*/ Vec<u8>>>,
        max_slots: Arc<MaxSlots>,
        rpc_subscriptions: Option<Arc<RpcSubscriptions>>,
    ) -> Self {
        let retransmit_thread_handle = retransmitter(
            retransmit_sockets,
            bank_forks,
            leader_schedule_cache,
            cluster_info,
            retransmit_receiver,
            max_slots,
            rpc_subscriptions,
        );

        Self {
            retransmit_thread_handle,
        }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.retransmit_thread_handle.join()
    }
}

impl AddAssign for RetransmitSlotStats {
    fn add_assign(&mut self, other: Self) {
        let Self {
            asof,
            outset,
            num_shreds_received,
            num_shreds_sent,
        } = other;
        self.asof = self.asof.max(asof);
        self.outset = if self.outset == 0 {
            outset
        } else {
            self.outset.min(outset)
        };
        for k in 0..MAX_NUM_TURBINE_HOPS {
            self.num_shreds_received[k] += num_shreds_received[k];
            self.num_shreds_sent[k] += num_shreds_sent[k];
        }
    }
}

impl RetransmitStats {
    const SLOT_STATS_CACHE_CAPACITY: usize = 750;

    fn new(now: Instant) -> Self {
        Self {
            since: now,
            num_nodes: AtomicUsize::default(),
            num_addrs_failed: AtomicUsize::default(),
            num_shreds: 0usize,
            num_shreds_skipped: 0usize,
            total_batches: 0usize,
            num_small_batches: 0usize,
            total_time: 0u64,
            epoch_fetch: 0u64,
            epoch_cache_update: 0u64,
            retransmit_total: AtomicU64::default(),
            compute_turbine_peers_total: AtomicU64::default(),
            // Cache capacity is manually enforced.
            slot_stats: LruCache::<Slot, RetransmitSlotStats>::unbounded(),
            unknown_shred_slot_leader: 0usize,
        }
    }

    fn upsert_slot_stats<I>(
        &mut self,
        feed: I,
        root: Slot,
        rpc_subscriptions: Option<&RpcSubscriptions>,
    ) where
        I: IntoIterator<Item = (Slot, RetransmitSlotStats)>,
    {
        for (slot, slot_stats) in feed {
            match self.slot_stats.get_mut(&slot) {
                None => {
                    if let Some(rpc_subscriptions) = rpc_subscriptions {
                        if slot > root {
                            let slot_update = SlotUpdate::FirstShredReceived {
                                slot,
                                timestamp: slot_stats.outset,
                            };
                            rpc_subscriptions.notify_slot_update(slot_update);
                            datapoint_info!("retransmit-first-shred", ("slot", slot, i64));
                        }
                    }
                    self.slot_stats.put(slot, slot_stats);
                }
                Some(entry) => {
                    *entry += slot_stats;
                }
            }
        }
        while self.slot_stats.len() > Self::SLOT_STATS_CACHE_CAPACITY {
            // Pop and submit metrics for the slot which was updated least
            // recently. At this point the node most likely will not receive
            // and retransmit any more shreds for this slot.
            match self.slot_stats.pop_lru() {
                Some((slot, stats)) => stats.submit(slot),
                None => break,
            }
        }
    }
}

impl RetransmitSlotStats {
    fn record(&mut self, now: u64, root_distance: usize, num_nodes: usize) {
        self.outset = if self.outset == 0 {
            now
        } else {
            self.outset.min(now)
        };
        self.asof = self.asof.max(now);
        self.num_shreds_received[root_distance] += 1;
        self.num_shreds_sent[root_distance] += num_nodes;
    }

    fn merge(mut acc: HashMap<Slot, Self>, other: HashMap<Slot, Self>) -> HashMap<Slot, Self> {
        if acc.len() < other.len() {
            return Self::merge(other, acc);
        }
        for (key, value) in other {
            *acc.entry(key).or_default() += value;
        }
        acc
    }

    fn submit(&self, slot: Slot) {
        let num_shreds: usize = self.num_shreds_received.iter().sum();
        let num_nodes: usize = self.num_shreds_sent.iter().sum();
        let elapsed_millis = self.asof.saturating_sub(self.outset);
        datapoint_info!(
            "retransmit-stage-slot-stats",
            ("slot", slot, i64),
            ("outset_timestamp", self.outset, i64),
            ("elapsed_millis", elapsed_millis, i64),
            ("num_shreds", num_shreds, i64),
            ("num_nodes", num_nodes, i64),
            ("num_shreds_received_root", self.num_shreds_received[0], i64),
            (
                "num_shreds_received_1st_layer",
                self.num_shreds_received[1],
                i64
            ),
            (
                "num_shreds_received_2nd_layer",
                self.num_shreds_received[2],
                i64
            ),
            (
                "num_shreds_received_3rd_layer",
                self.num_shreds_received[3],
                i64
            ),
            ("num_shreds_sent_root", self.num_shreds_sent[0], i64),
            ("num_shreds_sent_1st_layer", self.num_shreds_sent[1], i64),
            ("num_shreds_sent_2nd_layer", self.num_shreds_sent[2], i64),
            ("num_shreds_sent_3rd_layer", self.num_shreds_sent[3], i64),
        );
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_ledger::shred::{Shred, ShredFlags},
    };

    #[test]
    fn test_already_received() {
        let slot = 1;
        let index = 5;
        let version = 0x40;
        let shred = Shred::new_from_data(
            slot,
            index,
            0,
            &[],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            version,
            0,
        );
        let mut shreds_received = LruCache::new(100);
        let packet_hasher = PacketHasher::default();
        // unique shred for (1, 5) should pass
        assert!(!should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        // duplicate shred for (1, 5) blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_data(
            slot,
            index,
            2,
            &[],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            version,
            0,
        );
        // first duplicate shred for (1, 5) passed
        assert!(!should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        // then blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_data(
            slot,
            index,
            8,
            &[],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            version,
            0,
        );
        // 2nd duplicate shred for (1, 5) blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 0, 1, 1, 0, version);
        // Coding at (1, 5) passes
        assert!(!should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        // then blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 2, 1, 1, 0, version);
        // 2nd unique coding at (1, 5) passes
        assert!(!should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        // same again is blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 3, 1, 1, 0, version);
        // Another unique coding at (1, 5) always blocked
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
        assert!(should_skip_retransmit(
            shred.id(),
            shred.payload(),
            &mut shreds_received,
            &packet_hasher
        ));
    }
}
