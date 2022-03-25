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
        collections::{BTreeSet, HashMap, HashSet},
        net::UdpSocket,
        ops::{AddAssign, DerefMut},
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
    num_shreds: usize,
    num_nodes: usize,
}

impl AddAssign for RetransmitSlotStats {
    fn add_assign(&mut self, other: Self) {
        *self = Self {
            num_shreds: self.num_shreds + other.num_shreds,
            num_nodes: self.num_nodes + other.num_nodes,
        }
    }
}

#[derive(Default)]
struct RetransmitStats {
    since: Option<Instant>,
    num_nodes: AtomicUsize,
    num_shreds: usize,
    num_shreds_skipped: AtomicUsize,
    total_batches: usize,
    total_time: u64,
    epoch_fetch: u64,
    epoch_cache_update: u64,
    retransmit_total: AtomicU64,
    compute_turbine_peers_total: AtomicU64,
    slot_stats: HashMap<Slot, RetransmitSlotStats>,
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
        let elapsed = self.since.as_ref().map(Instant::elapsed);
        if elapsed.unwrap_or(Duration::MAX) < SUBMIT_CADENCE {
            return;
        }
        let num_peers = cluster_nodes_cache
            .get(root_bank.slot(), root_bank, working_bank, cluster_info)
            .num_peers();
        let stats = std::mem::replace(
            self,
            Self {
                since: Some(Instant::now()),
                ..Self::default()
            },
        );
        datapoint_info!("retransmit-num_nodes", ("count", num_peers, i64));
        datapoint_info!(
            "retransmit-stage",
            ("total_time", stats.total_time, i64),
            ("epoch_fetch", stats.epoch_fetch, i64),
            ("epoch_cache_update", stats.epoch_cache_update, i64),
            ("total_batches", stats.total_batches, i64),
            ("num_nodes", stats.num_nodes.into_inner(), i64),
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
        for (slot, stats) in stats.slot_stats {
            datapoint_info!(
                "retransmit-stage-slot-stats",
                ("slot", slot, i64),
                ("num_shreds", stats.num_shreds, i64),
                ("num_nodes", stats.num_nodes, i64),
            );
        }
    }
}

// Map of shred (slot, index, type) => list of hash values seen for that key.
type ShredFilter = LruCache<ShredId, Vec<u64>>;

type ShredFilterAndHasher = (ShredFilter, PacketHasher);

// Returns true if shred is already received and should skip retransmit.
fn should_skip_retransmit(shred: &Shred, shreds_received: &Mutex<ShredFilterAndHasher>) -> bool {
    let key = shred.id();
    let mut shreds_received = shreds_received.lock().unwrap();
    let (cache, hasher) = shreds_received.deref_mut();
    match cache.get_mut(&key) {
        Some(sent) if sent.len() >= MAX_DUPLICATE_COUNT => true,
        Some(sent) => {
            let hash = hasher.hash_shred(shred);
            if sent.contains(&hash) {
                true
            } else {
                sent.push(hash);
                false
            }
        }
        None => {
            let hash = hasher.hash_shred(shred);
            cache.put(key, vec![hash]);
            false
        }
    }
}

// Returns true if this is the first time receiving a shred for `shred_slot`.
fn check_if_first_shred_received(
    shred_slot: Slot,
    first_shreds_received: &Mutex<BTreeSet<Slot>>,
    root_bank: &Bank,
) -> bool {
    if shred_slot <= root_bank.slot() {
        return false;
    }

    let mut first_shreds_received_locked = first_shreds_received.lock().unwrap();
    if first_shreds_received_locked.insert(shred_slot) {
        datapoint_info!("retransmit-first-shred", ("slot", shred_slot, i64));
        if first_shreds_received_locked.len() > 100 {
            *first_shreds_received_locked =
                first_shreds_received_locked.split_off(&(root_bank.slot() + 1));
        }
        true
    } else {
        false
    }
}

fn maybe_reset_shreds_received_cache(
    shreds_received: &Mutex<ShredFilterAndHasher>,
    hasher_reset_ts: &mut Instant,
) {
    const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
    if hasher_reset_ts.elapsed() >= UPDATE_INTERVAL {
        *hasher_reset_ts = Instant::now();
        let mut shreds_received = shreds_received.lock().unwrap();
        let (cache, hasher) = shreds_received.deref_mut();
        cache.clear();
        hasher.reset();
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
    shreds_received: &Mutex<ShredFilterAndHasher>,
    max_slots: &MaxSlots,
    first_shreds_received: &Mutex<BTreeSet<Slot>>,
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
    maybe_reset_shreds_received_cache(shreds_received, hasher_reset_ts);
    epoch_cache_update.stop();
    stats.epoch_cache_update += epoch_cache_update.as_us();

    let socket_addr_space = cluster_info.socket_addr_space();
    let retransmit_shred = |shred: &Shred, socket: &UdpSocket| {
        if should_skip_retransmit(shred, shreds_received) {
            stats.num_shreds_skipped.fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        let shred_slot = shred.slot();
        max_slots
            .retransmit
            .fetch_max(shred_slot, Ordering::Relaxed);

        if let Some(rpc_subscriptions) = rpc_subscriptions {
            if check_if_first_shred_received(shred_slot, first_shreds_received, &root_bank) {
                rpc_subscriptions.notify_slot_update(SlotUpdate::FirstShredReceived {
                    slot: shred_slot,
                    timestamp: timestamp(),
                });
            }
        }

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
                    return 0;
                }
            };
        let cluster_nodes =
            cluster_nodes_cache.get(shred_slot, &root_bank, &working_bank, cluster_info);
        let addrs: Vec<_> = cluster_nodes
            .get_retransmit_addrs(slot_leader, shred, &root_bank, DATA_PLANE_FANOUT)
            .into_iter()
            .filter(|addr| ContactInfo::is_valid_address(addr, socket_addr_space))
            .collect();
        compute_turbine_peers.stop();
        stats
            .compute_turbine_peers_total
            .fetch_add(compute_turbine_peers.as_us(), Ordering::Relaxed);

        let mut retransmit_time = Measure::start("retransmit_to");
        let num_nodes = match multi_target_send(socket, &shred.payload, &addrs) {
            Ok(()) => addrs.len(),
            Err(SendPktsError::IoError(ioerr, num_failed)) => {
                inc_new_counter_info!("cluster_info-retransmit-packets", addrs.len(), 1);
                inc_new_counter_error!("cluster_info-retransmit-error", num_failed, 1);
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
        num_nodes
    };
    fn merge<K, V>(mut acc: HashMap<K, V>, other: HashMap<K, V>) -> HashMap<K, V>
    where
        K: Eq + std::hash::Hash,
        V: Default + AddAssign,
    {
        if acc.len() < other.len() {
            return merge(other, acc);
        }
        for (key, value) in other {
            *acc.entry(key).or_default() += value;
        }
        acc
    }
    let slot_stats = thread_pool.install(|| {
        shreds
            .into_par_iter()
            .with_min_len(4)
            .map(|shred| {
                let index = thread_pool.current_thread_index().unwrap();
                let socket = &sockets[index % sockets.len()];
                let num_nodes = retransmit_shred(&shred, socket);
                (shred.slot(), num_nodes)
            })
            .fold(
                HashMap::<Slot, RetransmitSlotStats>::new,
                |mut acc, (slot, num_nodes)| {
                    let stats = acc.entry(slot).or_default();
                    stats.num_nodes += num_nodes;
                    stats.num_shreds += 1;
                    acc
                },
            )
            .reduce(HashMap::new, merge)
    });
    stats.slot_stats = merge(std::mem::take(&mut stats.slot_stats), slot_stats);
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
    let mut stats = RetransmitStats::default();
    let shreds_received = Mutex::new((LruCache::new(DEFAULT_LRU_SIZE), PacketHasher::default()));
    let first_shreds_received = Mutex::<BTreeSet<Slot>>::default();
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
                    &max_slots,
                    &first_shreds_received,
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
        cfg: Option<Arc<AtomicBool>>,
        shred_version: u16,
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
                let is_connected = cfg
                    .as_ref()
                    .map(|x| x.load(Ordering::Relaxed))
                    .unwrap_or(true);
                let rv = should_retransmit_and_persist(
                    shred,
                    working_bank,
                    &leader_schedule_cache_clone,
                    id,
                    last_root,
                    shred_version,
                );
                rv && is_connected
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_gossip::contact_info::ContactInfo,
        solana_ledger::{
            blockstore_processor::{test_process_blockstore, ProcessOptions},
            create_new_tmp_ledger,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_net_utils::find_available_port_in_range,
        solana_sdk::signature::Keypair,
        solana_streamer::socket::SocketAddrSpace,
        std::net::{IpAddr, Ipv4Addr},
    };

    #[test]
    fn test_skip_repair() {
        solana_logger::setup();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            accounts_db_test_hash_calculation: true,
            full_leader_cache: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, leader_schedule_cache) =
            test_process_blockstore(&genesis_config, &blockstore, opts);
        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let bank_forks = Arc::new(RwLock::new(bank_forks));

        let mut me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), 0);
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let port = find_available_port_in_range(ip_addr, (8000, 10000)).unwrap();
        let me_retransmit = UdpSocket::bind(format!("127.0.0.1:{}", port)).unwrap();
        // need to make sure tvu and tpu are valid addresses
        me.tvu_forwards = me_retransmit.local_addr().unwrap();

        let port = find_available_port_in_range(ip_addr, (8000, 10000)).unwrap();
        me.tvu = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap()
            .local_addr()
            .unwrap();
        // This fixes the order of nodes returned by shuffle_peers_and_index,
        // and makes turbine retransmit tree deterministic for the purpose of
        // the test.
        let other = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .find(|pk| me.id < *pk)
            .unwrap();
        let other = ContactInfo::new_localhost(&other, 0);
        let cluster_info = ClusterInfo::new(
            other,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        );
        cluster_info.insert_info(me);

        let retransmit_socket = Arc::new(vec![UdpSocket::bind("0.0.0.0:0").unwrap()]);
        let cluster_info = Arc::new(cluster_info);

        let (retransmit_sender, retransmit_receiver) = unbounded();
        let _retransmit_sender = retransmit_sender.clone();
        let _t_retransmit = retransmitter(
            retransmit_socket,
            bank_forks,
            leader_schedule_cache,
            cluster_info,
            retransmit_receiver,
            Arc::default(), // MaxSlots
            None,
        );

        let shred = Shred::new_from_data(0, 0, 0, None, true, true, 0, 0x20, 0);
        // it should send this over the sockets.
        retransmit_sender.send(vec![shred]).unwrap();
        let mut packet_batch = PacketBatch::new(vec![]);
        solana_streamer::packet::recv_from(&mut packet_batch, &me_retransmit, 1).unwrap();
        assert_eq!(packet_batch.packets.len(), 1);
        assert!(!packet_batch.packets[0].meta.repair());
    }

    #[test]
    fn test_already_received() {
        let slot = 1;
        let index = 5;
        let version = 0x40;
        let shred = Shred::new_from_data(slot, index, 0, None, true, true, 0, version, 0);
        let shreds_received = Arc::new(Mutex::new((LruCache::new(100), PacketHasher::default())));
        // unique shred for (1, 5) should pass
        assert!(!should_skip_retransmit(&shred, &shreds_received));
        // duplicate shred for (1, 5) blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));

        let shred = Shred::new_from_data(slot, index, 2, None, true, true, 0, version, 0);
        // first duplicate shred for (1, 5) passed
        assert!(!should_skip_retransmit(&shred, &shreds_received));
        // then blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));

        let shred = Shred::new_from_data(slot, index, 8, None, true, true, 0, version, 0);
        // 2nd duplicate shred for (1, 5) blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));
        assert!(should_skip_retransmit(&shred, &shreds_received));

        let shred = Shred::new_empty_coding(slot, index, 0, 1, 1, 0, version);
        // Coding at (1, 5) passes
        assert!(!should_skip_retransmit(&shred, &shreds_received));
        // then blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));

        let shred = Shred::new_empty_coding(slot, index, 2, 1, 1, 0, version);
        // 2nd unique coding at (1, 5) passes
        assert!(!should_skip_retransmit(&shred, &shreds_received));
        // same again is blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));

        let shred = Shred::new_empty_coding(slot, index, 3, 1, 1, 0, version);
        // Another unique coding at (1, 5) always blocked
        assert!(should_skip_retransmit(&shred, &shreds_received));
        assert!(should_skip_retransmit(&shred, &shreds_received));
    }
}
