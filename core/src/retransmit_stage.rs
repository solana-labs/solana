//! The `retransmit_stage` retransmits shreds between validators
#![allow(clippy::rc_buffer)]

use {
    crate::{
        ancestor_hashes_service::AncestorHashesReplayUpdateReceiver,
        cluster_info_vote_listener::VerifiedVoteReceiver,
        cluster_nodes::ClusterNodesCache,
        cluster_slots::ClusterSlots,
        cluster_slots_service::{ClusterSlotsService, ClusterSlotsUpdateReceiver},
        completed_data_sets_service::CompletedDataSetsSender,
        packet_hasher::PacketHasher,
        repair_service::{DuplicateSlotsResetSender, RepairInfo},
        window_service::{should_retransmit_and_persist, WindowService},
    },
    crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender},
    lru::LruCache,
    rayon::{prelude::*, ThreadPool, ThreadPoolBuilder},
    solana_client::rpc_response::SlotUpdate,
    solana_gossip::{
        cluster_info::{ClusterInfo, DATA_PLANE_FANOUT},
        contact_info::ContactInfo,
    },
    solana_ledger::{
        blockstore::Blockstore,
        leader_schedule_cache::LeaderScheduleCache,
        shred::{Shred, ShredId},
    },
    solana_measure::measure::Measure,
    solana_perf::packet::PacketBatch,
    solana_rayon_threadlimit::get_thread_count,
    solana_rpc::{max_slots::MaxSlots, rpc_subscriptions::RpcSubscriptions},
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{clock::Slot, epoch_schedule::EpochSchedule, pubkey::Pubkey, timing::timestamp},
    solana_streamer::sendmmsg::{multi_target_send, SendPktsError},
    std::{
        collections::{HashMap, HashSet},
        net::UdpSocket,
        ops::AddAssign,
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const MAX_DUPLICATE_COUNT: usize = 2;
const DEFAULT_LRU_SIZE: usize = 10_000;

const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;
const CLUSTER_NODES_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RetransmitSlotStats {
    asof: u64,   // Latest timestamp struct was updated.
    outset: u64, // 1st shred retransmit timestamp.
    // Number of shreds sent and received at different
    // distances from the turbine broadcast root.
    num_shreds_received: [usize; 3],
    num_shreds_sent: [usize; 3],
}

struct RetransmitStats {
    since: Instant,
    num_nodes: AtomicUsize,
    num_addrs_failed: AtomicUsize,
    num_shreds: usize,
    num_shreds_skipped: AtomicUsize,
    total_batches: usize,
    total_time: u64,
    epoch_fetch: u64,
    epoch_cache_update: u64,
    retransmit_total: AtomicU64,
    compute_turbine_peers_total: AtomicU64,
    slot_stats: LruCache<Slot, RetransmitSlotStats>,
    unknown_shred_slot_leader: AtomicUsize,
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
        let num_peers = cluster_nodes_cache
            .get(root_bank.slot(), root_bank, working_bank, cluster_info)
            .num_peers();
        let stats = std::mem::replace(self, Self::new(Instant::now()));
        datapoint_info!("retransmit-num_nodes", ("count", num_peers, i64));
        datapoint_info!(
            "retransmit-stage",
            ("total_time", stats.total_time, i64),
            ("epoch_fetch", stats.epoch_fetch, i64),
            ("epoch_cache_update", stats.epoch_cache_update, i64),
            ("total_batches", stats.total_batches, i64),
            ("num_nodes", stats.num_nodes.into_inner(), i64),
            ("num_addrs_failed", stats.num_addrs_failed.into_inner(), i64),
            ("num_shreds", stats.num_shreds, i64),
            (
                "num_shreds_skipped",
                stats.num_shreds_skipped.into_inner(),
                i64
            ),
            ("retransmit_total", stats.retransmit_total.into_inner(), i64),
            (
                "compute_turbine",
                stats.compute_turbine_peers_total.into_inner(),
                i64
            ),
            (
                "unknown_shred_slot_leader",
                stats.unknown_shred_slot_leader.into_inner(),
                i64
            ),
        );
    }
}

// Map of shred (slot, index, type) => list of hash values seen for that key.
type ShredFilter = LruCache<ShredId, Vec<u64>>;

// Returns true if shred is already received and should skip retransmit.
fn should_skip_retransmit(
    shred: &Shred,
    shreds_received: &Mutex<ShredFilter>,
    packet_hasher: &PacketHasher,
) -> bool {
    let key = shred.id();
    let mut shreds_received = shreds_received.lock().unwrap();
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
    shreds_received: &Mutex<ShredFilter>,
    packet_hasher: &mut PacketHasher,
    hasher_reset_ts: &mut Instant,
) {
    const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
    if hasher_reset_ts.elapsed() >= UPDATE_INTERVAL {
        *hasher_reset_ts = Instant::now();
        shreds_received.lock().unwrap().clear();
        packet_hasher.reset();
    }
}

#[allow(clippy::too_many_arguments)]
fn retransmit(
    thread_pool: &ThreadPool,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    cluster_info: &ClusterInfo,
    shreds_receiver: &Receiver<Vec<Shred>>,
    sockets: &[UdpSocket],
    stats: &mut RetransmitStats,
    cluster_nodes_cache: &ClusterNodesCache<RetransmitStage>,
    hasher_reset_ts: &mut Instant,
    shreds_received: &Mutex<ShredFilter>,
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

    let socket_addr_space = cluster_info.socket_addr_space();
    let retransmit_shred = |shred: &Shred, socket: &UdpSocket| {
        if should_skip_retransmit(shred, shreds_received, packet_hasher) {
            stats.num_shreds_skipped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let shred_slot = shred.slot();
        max_slots
            .retransmit
            .fetch_max(shred_slot, Ordering::Relaxed);

        let mut compute_turbine_peers = Measure::start("turbine_start");
        // TODO: consider using root-bank here for leader lookup!
        // Shreds' signatures should be verified before they reach here, and if
        // the leader is unknown they should fail signature check. So here we
        // should expect to know the slot leader and otherwise skip the shred.
        let slot_leader =
            match leader_schedule_cache.slot_leader_at(shred_slot, Some(&working_bank)) {
                Some(pubkey) => pubkey,
                None => {
                    stats
                        .unknown_shred_slot_leader
                        .fetch_add(1, Ordering::Relaxed);
                    return None;
                }
            };
        let cluster_nodes =
            cluster_nodes_cache.get(shred_slot, &root_bank, &working_bank, cluster_info);
        let (root_distance, addrs) =
            cluster_nodes.get_retransmit_addrs(slot_leader, shred, &root_bank, DATA_PLANE_FANOUT);
        let addrs: Vec<_> = addrs
            .into_iter()
            .filter(|addr| ContactInfo::is_valid_address(addr, socket_addr_space))
            .collect();
        compute_turbine_peers.stop();
        stats
            .compute_turbine_peers_total
            .fetch_add(compute_turbine_peers.as_us(), Ordering::Relaxed);

        let mut retransmit_time = Measure::start("retransmit_to");
        let num_nodes = match multi_target_send(socket, shred.payload(), &addrs) {
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
        Some((root_distance, num_nodes))
    };
    let slot_stats = thread_pool.install(|| {
        shreds
            .into_par_iter()
            .with_min_len(4)
            .filter_map(|shred| {
                let index = thread_pool.current_thread_index().unwrap();
                let socket = &sockets[index % sockets.len()];
                Some((shred.slot(), retransmit_shred(&shred, socket)?))
            })
            .fold(
                HashMap::<Slot, RetransmitSlotStats>::new,
                |mut acc, (slot, (root_distance, num_nodes))| {
                    let now = timestamp();
                    let slot_stats = acc.entry(slot).or_default();
                    slot_stats.record(now, root_distance, num_nodes);
                    acc
                },
            )
            .reduce(HashMap::new, RetransmitSlotStats::merge)
    });
    stats.upsert_slot_stats(slot_stats, root_bank.slot(), rpc_subscriptions);
    timer_start.stop();
    stats.total_time += timer_start.as_us();
    stats.maybe_submit(&root_bank, &working_bank, cluster_info, cluster_nodes_cache);
    Ok(())
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
    shreds_receiver: Receiver<Vec<Shred>>,
    max_slots: Arc<MaxSlots>,
    rpc_subscriptions: Option<Arc<RpcSubscriptions>>,
) -> JoinHandle<()> {
    let cluster_nodes_cache = ClusterNodesCache::<RetransmitStage>::new(
        CLUSTER_NODES_CACHE_NUM_EPOCH_CAP,
        CLUSTER_NODES_CACHE_TTL,
    );
    let mut hasher_reset_ts = Instant::now();
    let mut stats = RetransmitStats::new(Instant::now());
    let shreds_received = Mutex::new(LruCache::new(DEFAULT_LRU_SIZE));
    let mut packet_hasher = PacketHasher::default();
    let num_threads = get_thread_count().min(8).max(sockets.len());
    let thread_pool = ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .thread_name(|i| format!("retransmit-{}", i))
        .build()
        .unwrap();
    Builder::new()
        .name("solana-retransmitter".to_string())
        .spawn(move || {
            trace!("retransmitter started");
            loop {
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
                    &shreds_received,
                    &mut packet_hasher,
                    &max_slots,
                    rpc_subscriptions.as_deref(),
                ) {
                    Ok(()) => (),
                    Err(RecvTimeoutError::Timeout) => (),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            trace!("exiting retransmitter");
        })
        .unwrap()
}

pub struct RetransmitStage {
    retransmit_thread_handle: JoinHandle<()>,
    window_service: WindowService,
    cluster_slots_service: ClusterSlotsService,
}

impl RetransmitStage {
    #[allow(clippy::new_ret_no_self)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        blockstore: Arc<Blockstore>,
        cluster_info: Arc<ClusterInfo>,
        retransmit_sockets: Arc<Vec<UdpSocket>>,
        repair_socket: Arc<UdpSocket>,
        ancestor_hashes_socket: Arc<UdpSocket>,
        verified_receiver: Receiver<Vec<PacketBatch>>,
        exit: Arc<AtomicBool>,
        cluster_slots_update_receiver: ClusterSlotsUpdateReceiver,
        epoch_schedule: EpochSchedule,
        turbine_disabled: Option<Arc<AtomicBool>>,
        cluster_slots: Arc<ClusterSlots>,
        duplicate_slots_reset_sender: DuplicateSlotsResetSender,
        verified_vote_receiver: VerifiedVoteReceiver,
        repair_validators: Option<HashSet<Pubkey>>,
        completed_data_sets_sender: CompletedDataSetsSender,
        max_slots: Arc<MaxSlots>,
        rpc_subscriptions: Option<Arc<RpcSubscriptions>>,
        duplicate_slots_sender: Sender<Slot>,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
    ) -> Self {
        let (retransmit_sender, retransmit_receiver) = unbounded();

        let retransmit_thread_handle = retransmitter(
            retransmit_sockets,
            bank_forks.clone(),
            leader_schedule_cache.clone(),
            cluster_info.clone(),
            retransmit_receiver,
            max_slots,
            rpc_subscriptions,
        );

        let cluster_slots_service = ClusterSlotsService::new(
            blockstore.clone(),
            cluster_slots.clone(),
            bank_forks.clone(),
            cluster_info.clone(),
            cluster_slots_update_receiver,
            exit.clone(),
        );

        let leader_schedule_cache_clone = leader_schedule_cache.clone();
        let repair_info = RepairInfo {
            bank_forks,
            epoch_schedule,
            duplicate_slots_reset_sender,
            repair_validators,
            cluster_info,
            cluster_slots,
        };
        let window_service = WindowService::new(
            blockstore,
            verified_receiver,
            retransmit_sender,
            repair_socket,
            ancestor_hashes_socket,
            exit,
            repair_info,
            leader_schedule_cache,
            move |id, shred, working_bank, last_root| {
                let turbine_disabled = turbine_disabled
                    .as_ref()
                    .map(|x| x.load(Ordering::Relaxed))
                    .unwrap_or(false);
                let rv = should_retransmit_and_persist(
                    shred,
                    working_bank,
                    &leader_schedule_cache_clone,
                    id,
                    last_root,
                );
                rv && !turbine_disabled
            },
            verified_vote_receiver,
            completed_data_sets_sender,
            duplicate_slots_sender,
            ancestor_hashes_replay_update_receiver,
        );

        Self {
            retransmit_thread_handle,
            window_service,
            cluster_slots_service,
        }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.retransmit_thread_handle.join()?;
        self.window_service.join()?;
        self.cluster_slots_service.join()
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
        for k in 0..3 {
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
            num_shreds_skipped: AtomicUsize::default(),
            total_batches: 0usize,
            total_time: 0u64,
            epoch_fetch: 0u64,
            epoch_cache_update: 0u64,
            retransmit_total: AtomicU64::default(),
            compute_turbine_peers_total: AtomicU64::default(),
            // Cache capacity is manually enforced.
            slot_stats: LruCache::<Slot, RetransmitSlotStats>::unbounded(),
            unknown_shred_slot_leader: AtomicUsize::default(),
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
            ("num_shreds_sent_root", self.num_shreds_sent[0], i64),
            ("num_shreds_sent_1st_layer", self.num_shreds_sent[1], i64),
            ("num_shreds_sent_2nd_layer", self.num_shreds_sent[2], i64),
        );
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_ledger::shred::ShredFlags};

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
        let shreds_received = Mutex::new(LruCache::new(100));
        let packet_hasher = PacketHasher::default();
        // unique shred for (1, 5) should pass
        assert!(!should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        // duplicate shred for (1, 5) blocked
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
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
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        // then blocked
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
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
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 0, 1, 1, 0, version);
        // Coding at (1, 5) passes
        assert!(!should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        // then blocked
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 2, 1, 1, 0, version);
        // 2nd unique coding at (1, 5) passes
        assert!(!should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        // same again is blocked
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));

        let shred = Shred::new_from_parity_shard(slot, index, &[], 3, 1, 1, 0, version);
        // Another unique coding at (1, 5) always blocked
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));
        assert!(should_skip_retransmit(
            &shred,
            &shreds_received,
            &packet_hasher
        ));
    }
}
