//! The `cluster_info` module defines a data structure that is shared by all the nodes in the network over
//! a gossip control plane.  The goal is to share small bits of off-chain information and detect and
//! repair partitions.
//!
//! This CRDT only supports a very limited set of types.  A map of Pubkey -> Versioned Struct.
//! The last version is always picked during an update.
//!
//! The network is arranged in layers:
//!
//! * layer 0 - Leader.
//! * layer 1 - As many nodes as we can fit
//! * layer 2 - Everyone else, if layer 1 is `2^10`, layer 2 should be able to fit `2^20` number of nodes.
//!
//! Bank needs to provide an interface for us to query the stake weight
use crate::{
    contact_info::ContactInfo,
    crds_gossip::CrdsGossip,
    crds_gossip_error::CrdsGossipError,
    crds_gossip_pull::{CrdsFilter, ProcessPullStats, CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS},
    crds_value::{
        self, CrdsData, CrdsValue, CrdsValueLabel, EpochSlotsIndex, LowestSlot, NodeInstance,
        SnapshotHash, Version, Vote, MAX_WALLCLOCK,
    },
    data_budget::DataBudget,
    epoch_slots::EpochSlots,
    ping_pong::{self, PingCache, Pong},
    result::{Error, Result},
    weighted_shuffle::weighted_shuffle,
};

use rand::distributions::{Distribution, WeightedIndex};
use rand::{CryptoRng, Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_sdk::sanitize::{Sanitize, SanitizeError};

use bincode::{serialize, serialized_size};
use core::cmp;
use itertools::Itertools;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::ser::Serialize;
use solana_measure::measure::Measure;
use solana_measure::thread_mem_usage;
use solana_metrics::{inc_new_counter_debug, inc_new_counter_error};
use solana_net_utils::{
    bind_common, bind_common_in_range, bind_in_range, find_available_port_in_range,
    multi_bind_in_range, PortRange,
};
use solana_perf::packet::{
    limited_deserialize, to_packets_with_destination, Packet, Packets, PacketsRecycler,
    PACKET_DATA_SIZE,
};
use solana_rayon_threadlimit::get_thread_count;
use solana_runtime::bank_forks::BankForks;
use solana_sdk::{
    clock::{Slot, DEFAULT_MS_PER_SLOT, DEFAULT_SLOTS_PER_EPOCH},
    feature_set::{self, FeatureSet},
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signable, Signature, Signer},
    timing::timestamp,
    transaction::Transaction,
};
use solana_streamer::sendmmsg::multicast;
use solana_streamer::streamer::{PacketReceiver, PacketSender};
use std::{
    borrow::Cow,
    cmp::min,
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    fmt::{self, Debug},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    thread::{sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

pub const VALIDATOR_PORT_RANGE: PortRange = (8000, 10_000);
pub const MINIMUM_VALIDATOR_PORT_RANGE_WIDTH: u16 = 10; // VALIDATOR_PORT_RANGE must be at least this wide

/// The Data plane fanout size, also used as the neighborhood size
pub const DATA_PLANE_FANOUT: usize = 200;
/// milliseconds we sleep for between gossip requests
pub const GOSSIP_SLEEP_MILLIS: u64 = 100;
/// The maximum size of a bloom filter
pub const MAX_BLOOM_SIZE: usize = MAX_CRDS_OBJECT_SIZE;
pub const MAX_CRDS_OBJECT_SIZE: usize = 928;
/// A hard limit on incoming gossip messages
/// Chosen to be able to handle 1Gbps of pure gossip traffic
/// 128MB/PACKET_DATA_SIZE
const MAX_GOSSIP_TRAFFIC: usize = 128_000_000 / PACKET_DATA_SIZE;
/// Max size of serialized crds-values in a Protocol::PushMessage packet. This
/// is equal to PACKET_DATA_SIZE minus serialized size of an empty push
/// message: Protocol::PushMessage(Pubkey::default(), Vec::default())
const PUSH_MESSAGE_MAX_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - 44;
/// Maximum number of hashes in SnapshotHashes/AccountsHashes a node publishes
/// such that the serialized size of the push/pull message stays below
/// PACKET_DATA_SIZE.
// TODO: Update this to 26 once payload sizes are upgraded across fleet.
pub const MAX_SNAPSHOT_HASHES: usize = 16;
/// Maximum number of origin nodes that a PruneData may contain, such that the
/// serialized size of the PruneMessage stays below PACKET_DATA_SIZE.
const MAX_PRUNE_DATA_NODES: usize = 32;
/// Number of bytes in the randomly generated token sent with ping messages.
const GOSSIP_PING_TOKEN_SIZE: usize = 32;
const GOSSIP_PING_CACHE_CAPACITY: usize = 16384;
const GOSSIP_PING_CACHE_TTL: Duration = Duration::from_secs(640);
pub const DEFAULT_CONTACT_DEBUG_INTERVAL: u64 = 10_000;
/// Minimum serialized size of a Protocol::PullResponse packet.
const PULL_RESPONSE_MIN_SERIALIZED_SIZE: usize = 167;

#[derive(Debug, PartialEq, Eq)]
pub enum ClusterInfoError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadGossipAddress,
}

struct GossipWriteLock<'a> {
    gossip: RwLockWriteGuard<'a, CrdsGossip>,
    timer: Measure,
    counter: &'a Counter,
}

impl<'a> GossipWriteLock<'a> {
    fn new(
        gossip: RwLockWriteGuard<'a, CrdsGossip>,
        label: &'static str,
        counter: &'a Counter,
    ) -> Self {
        Self {
            gossip,
            timer: Measure::start(label),
            counter,
        }
    }
}

impl<'a> Deref for GossipWriteLock<'a> {
    type Target = RwLockWriteGuard<'a, CrdsGossip>;
    fn deref(&self) -> &Self::Target {
        &self.gossip
    }
}

impl<'a> DerefMut for GossipWriteLock<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.gossip
    }
}

impl<'a> Drop for GossipWriteLock<'a> {
    fn drop(&mut self) {
        self.counter.add_measure(&mut self.timer);
    }
}

struct GossipReadLock<'a> {
    gossip: RwLockReadGuard<'a, CrdsGossip>,
    timer: Measure,
    counter: &'a Counter,
}

impl<'a> GossipReadLock<'a> {
    fn new(
        gossip: RwLockReadGuard<'a, CrdsGossip>,
        label: &'static str,
        counter: &'a Counter,
    ) -> Self {
        Self {
            gossip,
            timer: Measure::start(label),
            counter,
        }
    }
}

impl<'a> Deref for GossipReadLock<'a> {
    type Target = RwLockReadGuard<'a, CrdsGossip>;
    fn deref(&self) -> &Self::Target {
        &self.gossip
    }
}

impl<'a> Drop for GossipReadLock<'a> {
    fn drop(&mut self) {
        self.counter.add_measure(&mut self.timer);
    }
}

#[derive(Default)]
struct Counter(AtomicU64);

impl Counter {
    fn add_measure(&self, x: &mut Measure) {
        x.stop();
        self.0.fetch_add(x.as_us(), Ordering::Relaxed);
    }
    fn add_relaxed(&self, x: u64) {
        self.0.fetch_add(x, Ordering::Relaxed);
    }
    fn clear(&self) -> u64 {
        self.0.swap(0, Ordering::Relaxed)
    }
}

struct ScopedTimer<'a> {
    clock: Instant,
    metric: &'a AtomicU64,
}

impl<'a> From<&'a Counter> for ScopedTimer<'a> {
    // Output should be assigned to a *named* variable,
    // otherwise it is immediately dropped.
    #[must_use]
    fn from(counter: &'a Counter) -> Self {
        Self {
            clock: Instant::now(),
            metric: &counter.0,
        }
    }
}

impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        let micros = self.clock.elapsed().as_micros();
        self.metric.fetch_add(micros as u64, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct GossipStats {
    entrypoint: Counter,
    entrypoint2: Counter,
    gossip_packets_dropped_count: Counter,
    push_vote_read: Counter,
    vote_process_push: Counter,
    get_votes: Counter,
    get_accounts_hash: Counter,
    get_snapshot_hash: Counter,
    all_tvu_peers: Counter,
    tvu_peers: Counter,
    retransmit_peers: Counter,
    repair_peers: Counter,
    new_push_requests: Counter,
    new_push_requests2: Counter,
    new_push_requests_num: Counter,
    filter_pull_response: Counter,
    handle_batch_ping_messages_time: Counter,
    handle_batch_pong_messages_time: Counter,
    handle_batch_prune_messages_time: Counter,
    handle_batch_pull_requests_time: Counter,
    handle_batch_pull_responses_time: Counter,
    handle_batch_push_messages_time: Counter,
    process_gossip_packets_time: Counter,
    process_pull_response: Counter,
    process_pull_response_count: Counter,
    process_pull_response_len: Counter,
    process_pull_response_timeout: Counter,
    process_pull_response_fail_insert: Counter,
    process_pull_response_fail_timeout: Counter,
    process_pull_response_success: Counter,
    process_pull_requests: Counter,
    generate_pull_responses: Counter,
    process_prune: Counter,
    process_push_message: Counter,
    prune_received_cache: Counter,
    prune_message_count: Counter,
    prune_message_len: Counter,
    pull_request_ping_pong_check_failed_count: Counter,
    purge: Counter,
    epoch_slots_lookup: Counter,
    epoch_slots_push: Counter,
    push_message: Counter,
    new_pull_requests: Counter,
    new_pull_requests_count: Counter,
    mark_pull_request: Counter,
    skip_pull_response_shred_version: Counter,
    skip_pull_shred_version: Counter,
    skip_push_message_shred_version: Counter,
    push_message_count: Counter,
    push_message_value_count: Counter,
    push_response_count: Counter,
    pull_requests_count: Counter,
}

pub struct ClusterInfo {
    /// The network
    pub gossip: RwLock<CrdsGossip>,
    /// set the keypair that will be used to sign crds values generated. It is unset only in tests.
    pub(crate) keypair: Arc<Keypair>,
    /// The network entrypoint
    entrypoint: RwLock<Option<ContactInfo>>,
    outbound_budget: DataBudget,
    my_contact_info: RwLock<ContactInfo>,
    ping_cache: RwLock<PingCache>,
    id: Pubkey,
    stats: GossipStats,
    socket: UdpSocket,
    local_message_pending_push_queue: RwLock<Vec<(CrdsValue, u64)>>,
    contact_debug_interval: u64,
    instance: NodeInstance,
}

impl Default for ClusterInfo {
    fn default() -> Self {
        Self::new_with_invalid_keypair(ContactInfo::default())
    }
}

#[derive(Default, Clone)]
pub struct Locality {
    /// The bounds of the neighborhood represented by this locality
    pub neighbor_bounds: (usize, usize),
    /// The `turbine` layer this locality is in
    pub layer_ix: usize,
    /// The bounds of the current layer
    pub layer_bounds: (usize, usize),
    /// The bounds of the next layer
    pub next_layer_bounds: Option<(usize, usize)>,
    /// The indices of the nodes that should be contacted in next layer
    pub next_layer_peers: Vec<usize>,
}

impl fmt::Debug for Locality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Locality {{ neighborhood_bounds: {:?}, current_layer: {:?}, child_layer_bounds: {:?} child_layer_peers: {:?} }}",
            self.neighbor_bounds, self.layer_ix, self.next_layer_bounds, self.next_layer_peers
        )
    }
}

#[derive(Debug, Default, Deserialize, Serialize, AbiExample)]
pub struct PruneData {
    /// Pubkey of the node that sent this prune data
    pub pubkey: Pubkey,
    /// Pubkeys of nodes that should be pruned
    pub prunes: Vec<Pubkey>,
    /// Signature of this Prune Message
    pub signature: Signature,
    /// The Pubkey of the intended node/destination for this message
    pub destination: Pubkey,
    /// Wallclock of the node that generated this message
    pub wallclock: u64,
}

impl PruneData {
    /// New random PruneData for tests and benchmarks.
    #[cfg(test)]
    fn new_rand<R: Rng>(rng: &mut R, self_keypair: &Keypair, num_nodes: Option<usize>) -> Self {
        let wallclock = crds_value::new_rand_timestamp(rng);
        let num_nodes = num_nodes.unwrap_or_else(|| rng.gen_range(0, MAX_PRUNE_DATA_NODES + 1));
        let prunes = std::iter::repeat_with(Pubkey::new_unique)
            .take(num_nodes)
            .collect();
        let mut prune_data = PruneData {
            pubkey: self_keypair.pubkey(),
            prunes,
            signature: Signature::default(),
            destination: Pubkey::new_unique(),
            wallclock,
        };
        prune_data.sign(&self_keypair);
        prune_data
    }
}

impl Sanitize for PruneData {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

impl Signable for PruneData {
    fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    fn signable_data(&self) -> Cow<[u8]> {
        #[derive(Serialize)]
        struct SignData {
            pubkey: Pubkey,
            prunes: Vec<Pubkey>,
            destination: Pubkey,
            wallclock: u64,
        }
        let data = SignData {
            pubkey: self.pubkey,
            prunes: self.prunes.clone(),
            destination: self.destination,
            wallclock: self.wallclock,
        };
        Cow::Owned(serialize(&data).expect("serialize PruneData"))
    }

    fn get_signature(&self) -> Signature {
        self.signature
    }

    fn set_signature(&mut self, signature: Signature) {
        self.signature = signature
    }
}

struct PullData {
    pub from_addr: SocketAddr,
    pub caller: CrdsValue,
    pub filter: CrdsFilter,
}

pub fn make_accounts_hashes_message(
    keypair: &Keypair,
    accounts_hashes: Vec<(Slot, Hash)>,
) -> Option<CrdsValue> {
    let message = CrdsData::AccountsHashes(SnapshotHash::new(keypair.pubkey(), accounts_hashes));
    Some(CrdsValue::new_signed(message, keypair))
}

type Ping = ping_pong::Ping<[u8; GOSSIP_PING_TOKEN_SIZE]>;

// TODO These messages should go through the gpu pipeline for spam filtering
#[frozen_abi(digest = "HAFjUDgiGthYTiAg6CYJxA8PqfwuhrC82NtHYYmee4vb")]
#[derive(Serialize, Deserialize, Debug, AbiEnumVisitor, AbiExample)]
#[allow(clippy::large_enum_variant)]
enum Protocol {
    /// Gossip protocol messages
    PullRequest(CrdsFilter, CrdsValue),
    PullResponse(Pubkey, Vec<CrdsValue>),
    PushMessage(Pubkey, Vec<CrdsValue>),
    PruneMessage(Pubkey, PruneData),
    PingMessage(Ping),
    PongMessage(Pong),
}

impl Protocol {
    fn par_verify(self) -> Option<Self> {
        match self {
            Protocol::PullRequest(_, ref caller) => {
                if caller.verify() {
                    Some(self)
                } else {
                    inc_new_counter_info!("cluster_info-gossip_pull_request_verify_fail", 1);
                    None
                }
            }
            Protocol::PullResponse(from, data) => {
                let size = data.len();
                let data: Vec<_> = data.into_par_iter().filter(Signable::verify).collect();
                if size != data.len() {
                    inc_new_counter_info!(
                        "cluster_info-gossip_pull_response_verify_fail",
                        size - data.len()
                    );
                }
                if data.is_empty() {
                    None
                } else {
                    Some(Protocol::PullResponse(from, data))
                }
            }
            Protocol::PushMessage(from, data) => {
                let size = data.len();
                let data: Vec<_> = data.into_par_iter().filter(Signable::verify).collect();
                if size != data.len() {
                    inc_new_counter_info!(
                        "cluster_info-gossip_push_msg_verify_fail",
                        size - data.len()
                    );
                }
                if data.is_empty() {
                    None
                } else {
                    Some(Protocol::PushMessage(from, data))
                }
            }
            Protocol::PruneMessage(_, ref data) => {
                if data.verify() {
                    Some(self)
                } else {
                    inc_new_counter_debug!("cluster_info-gossip_prune_msg_verify_fail", 1);
                    None
                }
            }
            Protocol::PingMessage(ref ping) => {
                if ping.verify() {
                    Some(self)
                } else {
                    inc_new_counter_info!("cluster_info-gossip_ping_msg_verify_fail", 1);
                    None
                }
            }
            Protocol::PongMessage(ref pong) => {
                if pong.verify() {
                    Some(self)
                } else {
                    inc_new_counter_info!("cluster_info-gossip_pong_msg_verify_fail", 1);
                    None
                }
            }
        }
    }
}

impl Sanitize for Protocol {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        match self {
            Protocol::PullRequest(filter, val) => {
                filter.sanitize()?;
                val.sanitize()
            }
            Protocol::PullResponse(_, val) => val.sanitize(),
            Protocol::PushMessage(_, val) => val.sanitize(),
            Protocol::PruneMessage(_, val) => val.sanitize(),
            Protocol::PingMessage(ping) => ping.sanitize(),
            Protocol::PongMessage(pong) => pong.sanitize(),
        }
    }
}

// Rating for pull requests
// A response table is generated as a
// 2-d table arranged by target nodes and a
// list of responses for that node,
// to/responses_index is a location in that table.
struct ResponseScore {
    to: usize,              // to, index of who the response is to
    responses_index: usize, // index into the list of responses for a given to
    score: u64,             // Relative score of the response
}

impl ClusterInfo {
    /// Without a valid keypair gossip will not function. Only useful for tests.
    pub fn new_with_invalid_keypair(contact_info: ContactInfo) -> Self {
        Self::new(contact_info, Arc::new(Keypair::new()))
    }

    pub fn new(contact_info: ContactInfo, keypair: Arc<Keypair>) -> Self {
        let id = contact_info.id;
        let me = Self {
            gossip: RwLock::new(CrdsGossip::default()),
            keypair,
            entrypoint: RwLock::new(None),
            outbound_budget: DataBudget::default(),
            my_contact_info: RwLock::new(contact_info),
            ping_cache: RwLock::new(PingCache::new(
                GOSSIP_PING_CACHE_TTL,
                GOSSIP_PING_CACHE_CAPACITY,
            )),
            id,
            stats: GossipStats::default(),
            socket: UdpSocket::bind("0.0.0.0:0").unwrap(),
            local_message_pending_push_queue: RwLock::new(vec![]),
            contact_debug_interval: DEFAULT_CONTACT_DEBUG_INTERVAL,
            instance: NodeInstance::new(&mut rand::thread_rng(), id, timestamp()),
        };
        {
            let mut gossip = me.gossip.write().unwrap();
            gossip.set_self(&id);
            gossip.set_shred_version(me.my_shred_version());
        }
        me.insert_self();
        me.push_self(&HashMap::new(), None);
        me
    }

    // Should only be used by tests and simulations
    pub fn clone_with_id(&self, new_id: &Pubkey) -> Self {
        let mut gossip = self.gossip.read().unwrap().mock_clone();
        gossip.id = *new_id;
        let mut my_contact_info = self.my_contact_info.read().unwrap().clone();
        my_contact_info.id = *new_id;
        ClusterInfo {
            gossip: RwLock::new(gossip),
            keypair: self.keypair.clone(),
            entrypoint: RwLock::new(self.entrypoint.read().unwrap().clone()),
            outbound_budget: self.outbound_budget.clone_non_atomic(),
            my_contact_info: RwLock::new(my_contact_info),
            ping_cache: RwLock::new(self.ping_cache.read().unwrap().mock_clone()),
            id: *new_id,
            stats: GossipStats::default(),
            socket: UdpSocket::bind("0.0.0.0:0").unwrap(),
            local_message_pending_push_queue: RwLock::new(
                self.local_message_pending_push_queue
                    .read()
                    .unwrap()
                    .clone(),
            ),
            contact_debug_interval: self.contact_debug_interval,
            instance: NodeInstance::new(&mut rand::thread_rng(), *new_id, timestamp()),
        }
    }

    pub fn set_contact_debug_interval(&mut self, new: u64) {
        self.contact_debug_interval = new;
    }

    pub fn update_contact_info<F>(&self, modify: F)
    where
        F: FnOnce(&mut ContactInfo),
    {
        let my_id = self.id();
        modify(&mut self.my_contact_info.write().unwrap());
        assert_eq!(self.my_contact_info.read().unwrap().id, my_id);
        self.insert_self()
    }

    fn push_self(
        &self,
        stakes: &HashMap<Pubkey, u64>,
        gossip_validators: Option<&HashSet<Pubkey>>,
    ) {
        let now = timestamp();
        self.my_contact_info.write().unwrap().wallclock = now;
        let entries: Vec<_> = vec![
            CrdsData::ContactInfo(self.my_contact_info()),
            CrdsData::NodeInstance(self.instance.with_wallclock(now)),
        ]
        .into_iter()
        .map(|v| CrdsValue::new_signed(v, &self.keypair))
        .collect();
        {
            let mut local_message_pending_push_queue =
                self.local_message_pending_push_queue.write().unwrap();
            for entry in entries {
                local_message_pending_push_queue.push((entry, now));
            }
        }
        self.gossip
            .write()
            .unwrap()
            .refresh_push_active_set(stakes, gossip_validators);
    }

    // TODO kill insert_info, only used by tests
    pub fn insert_info(&self, contact_info: ContactInfo) {
        let value = CrdsValue::new_signed(CrdsData::ContactInfo(contact_info), &self.keypair);
        let _ = self.gossip.write().unwrap().crds.insert(value, timestamp());
    }

    pub fn set_entrypoint(&self, entrypoint: ContactInfo) {
        *self.entrypoint.write().unwrap() = Some(entrypoint)
    }

    pub fn id(&self) -> Pubkey {
        self.id
    }

    pub fn lookup_contact_info<F, Y>(&self, id: &Pubkey, map: F) -> Option<Y>
    where
        F: FnOnce(&ContactInfo) -> Y,
    {
        let entry = CrdsValueLabel::ContactInfo(*id);
        self.gossip
            .read()
            .unwrap()
            .crds
            .lookup(&entry)
            .and_then(CrdsValue::contact_info)
            .map(map)
    }

    pub fn lookup_contact_info_by_gossip_addr(
        &self,
        gossip_addr: &SocketAddr,
    ) -> Option<ContactInfo> {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get_nodes_contact_info()
            .find(|peer| peer.gossip == *gossip_addr)
            .cloned()
    }

    pub fn my_contact_info(&self) -> ContactInfo {
        self.my_contact_info.read().unwrap().clone()
    }

    pub fn my_shred_version(&self) -> u16 {
        self.my_contact_info.read().unwrap().shred_version
    }

    pub fn lookup_epoch_slots(&self, ix: EpochSlotsIndex) -> EpochSlots {
        let entry = CrdsValueLabel::EpochSlots(ix, self.id());
        self.gossip
            .read()
            .unwrap()
            .crds
            .lookup(&entry)
            .and_then(CrdsValue::epoch_slots)
            .cloned()
            .unwrap_or_else(|| EpochSlots::new(self.id(), timestamp()))
    }

    pub fn rpc_info_trace(&self) -> String {
        let now = timestamp();
        let my_pubkey = self.id();
        let my_shred_version = self.my_shred_version();
        let nodes: Vec<_> = self
            .all_peers()
            .into_iter()
            .filter_map(|(node, last_updated)| {
                if !ContactInfo::is_valid_address(&node.rpc) {
                    return None;
                }

                let node_version = self.get_node_version(&node.id);
                if my_shred_version != 0
                    && (node.shred_version != 0 && node.shred_version != my_shred_version)
                {
                    return None;
                }

                fn addr_to_string(default_ip: &IpAddr, addr: &SocketAddr) -> String {
                    if ContactInfo::is_valid_address(addr) {
                        if &addr.ip() == default_ip {
                            addr.port().to_string()
                        } else {
                            addr.to_string()
                        }
                    } else {
                        "none".to_string()
                    }
                }

                let rpc_addr = node.rpc.ip();
                Some(format!(
                    "{:15} {:2}| {:5} | {:44} |{:^9}| {:5}| {:5}| {}\n",
                    rpc_addr.to_string(),
                    if node.id == my_pubkey { "me" } else { "" }.to_string(),
                    now.saturating_sub(last_updated),
                    node.id.to_string(),
                    if let Some(node_version) = node_version {
                        node_version.to_string()
                    } else {
                        "-".to_string()
                    },
                    addr_to_string(&rpc_addr, &node.rpc),
                    addr_to_string(&rpc_addr, &node.rpc_pubsub),
                    node.shred_version,
                ))
            })
            .collect();

        format!(
            "RPC Address       |Age(ms)| Node identifier                              \
             | Version | RPC  |PubSub|ShredVer\n\
             ------------------+-------+----------------------------------------------+---------+\
             ------+------+--------\n\
             {}\
             RPC Enabled Nodes: {}",
            nodes.join(""),
            nodes.len(),
        )
    }

    pub fn contact_info_trace(&self) -> String {
        let now = timestamp();
        let mut spy_nodes = 0;
        let mut different_shred_nodes = 0;
        let my_pubkey = self.id();
        let my_shred_version = self.my_shred_version();
        let nodes: Vec<_> = self
            .all_peers()
            .into_iter()
            .filter_map(|(node, last_updated)| {
                if Self::is_spy_node(&node) {
                    spy_nodes += 1;
                }

                let node_version = self.get_node_version(&node.id);
                if my_shred_version != 0 && (node.shred_version != 0 && node.shred_version != my_shred_version) {
                    different_shred_nodes += 1;
                    None
                } else {
                    fn addr_to_string(default_ip: &IpAddr, addr: &SocketAddr) -> String {
                        if ContactInfo::is_valid_address(addr) {
                            if &addr.ip() == default_ip {
                                addr.port().to_string()
                            } else {
                                addr.to_string()
                            }
                        } else {
                            "none".to_string()
                        }
                    }
                    let ip_addr = node.gossip.ip();
                    Some(format!(
                        "{:15} {:2}| {:5} | {:44} |{:^9}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {}\n",
                        if ContactInfo::is_valid_address(&node.gossip) {
                            ip_addr.to_string()
                        } else {
                            "none".to_string()
                        },
                        if node.id == my_pubkey { "me" } else { "" }.to_string(),
                        now.saturating_sub(last_updated),
                        node.id.to_string(),
                        if let Some(node_version) = node_version {
                            node_version.to_string()
                        } else {
                            "-".to_string()
                        },
                        addr_to_string(&ip_addr, &node.gossip),
                        addr_to_string(&ip_addr, &node.tpu),
                        addr_to_string(&ip_addr, &node.tpu_forwards),
                        addr_to_string(&ip_addr, &node.tvu),
                        addr_to_string(&ip_addr, &node.tvu_forwards),
                        addr_to_string(&ip_addr, &node.repair),
                        addr_to_string(&ip_addr, &node.serve_repair),
                        node.shred_version,
                    ))
                }
            })
            .collect();

        format!(
            "IP Address        |Age(ms)| Node identifier                              \
             | Version |Gossip| TPU  |TPUfwd| TVU  |TVUfwd|Repair|ServeR|ShredVer\n\
             ------------------+-------+----------------------------------------------+---------+\
             ------+------+------+------+------+------+------+--------\n\
             {}\
             Nodes: {}{}{}",
            nodes.join(""),
            nodes.len() - spy_nodes,
            if spy_nodes > 0 {
                format!("\nSpies: {}", spy_nodes)
            } else {
                "".to_string()
            },
            if different_shred_nodes > 0 {
                format!(
                    "\nNodes with different shred version: {}",
                    different_shred_nodes
                )
            } else {
                "".to_string()
            }
        )
    }

    pub fn push_lowest_slot(&self, id: Pubkey, min: Slot) {
        let now = timestamp();
        let last = self
            .gossip
            .read()
            .unwrap()
            .crds
            .lookup(&CrdsValueLabel::LowestSlot(self.id()))
            .and_then(|x| x.lowest_slot())
            .map(|x| x.lowest)
            .unwrap_or(0);
        if min > last {
            let entry = CrdsValue::new_signed(
                CrdsData::LowestSlot(0, LowestSlot::new(id, min, now)),
                &self.keypair,
            );
            self.local_message_pending_push_queue
                .write()
                .unwrap()
                .push((entry, now));
        }
    }

    pub fn push_epoch_slots(&self, update: &[Slot]) {
        let mut num = 0;
        let mut current_slots: Vec<_> = (0..crds_value::MAX_EPOCH_SLOTS)
            .filter_map(|ix| {
                Some((
                    self.time_gossip_read_lock(
                        "lookup_epoch_slots",
                        &self.stats.epoch_slots_lookup,
                    )
                    .crds
                    .lookup(&CrdsValueLabel::EpochSlots(ix, self.id()))
                    .and_then(CrdsValue::epoch_slots)
                    .and_then(|x| Some((x.wallclock, x.first_slot()?)))?,
                    ix,
                ))
            })
            .collect();
        current_slots.sort_unstable();
        let min_slot: Slot = current_slots
            .iter()
            .map(|((_, s), _)| *s)
            .min()
            .unwrap_or(0);
        let max_slot: Slot = update.iter().max().cloned().unwrap_or(0);
        let total_slots = max_slot as isize - min_slot as isize;
        // WARN if CRDS is not storing at least a full epoch worth of slots
        if DEFAULT_SLOTS_PER_EPOCH as isize > total_slots
            && crds_value::MAX_EPOCH_SLOTS as usize <= current_slots.len()
        {
            inc_new_counter_warn!("cluster_info-epoch_slots-filled", 1);
            warn!(
                "EPOCH_SLOTS are filling up FAST {}/{}",
                total_slots,
                current_slots.len()
            );
        }
        let mut reset = false;
        let mut epoch_slot_index = current_slots.last().map(|(_, x)| *x).unwrap_or(0);
        while num < update.len() {
            let ix = (epoch_slot_index % crds_value::MAX_EPOCH_SLOTS) as u8;
            let now = timestamp();
            let mut slots = if !reset {
                self.lookup_epoch_slots(ix)
            } else {
                EpochSlots::new(self.id(), now)
            };
            let n = slots.fill(&update[num..], now);
            if n > 0 {
                let entry = CrdsValue::new_signed(CrdsData::EpochSlots(ix, slots), &self.keypair);
                self.local_message_pending_push_queue
                    .write()
                    .unwrap()
                    .push((entry, now));
            }
            num += n;
            if num < update.len() {
                epoch_slot_index += 1;
                reset = true;
            }
        }
    }

    fn time_gossip_read_lock<'a>(
        &'a self,
        label: &'static str,
        counter: &'a Counter,
    ) -> GossipReadLock<'a> {
        GossipReadLock::new(self.gossip.read().unwrap(), label, counter)
    }

    fn time_gossip_write_lock<'a>(
        &'a self,
        label: &'static str,
        counter: &'a Counter,
    ) -> GossipWriteLock<'a> {
        GossipWriteLock::new(self.gossip.write().unwrap(), label, counter)
    }

    pub fn push_message(&self, message: CrdsValue) {
        let now = message.wallclock();
        self.local_message_pending_push_queue
            .write()
            .unwrap()
            .push((message, now));
    }

    pub fn push_accounts_hashes(&self, accounts_hashes: Vec<(Slot, Hash)>) {
        if accounts_hashes.len() > MAX_SNAPSHOT_HASHES {
            warn!(
                "accounts hashes too large, ignored: {}",
                accounts_hashes.len(),
            );
            return;
        }

        let message = CrdsData::AccountsHashes(SnapshotHash::new(self.id(), accounts_hashes));
        self.push_message(CrdsValue::new_signed(message, &self.keypair));
    }

    pub fn push_snapshot_hashes(&self, snapshot_hashes: Vec<(Slot, Hash)>) {
        if snapshot_hashes.len() > MAX_SNAPSHOT_HASHES {
            warn!(
                "snapshot hashes too large, ignored: {}",
                snapshot_hashes.len(),
            );
            return;
        }

        let message = CrdsData::SnapshotHashes(SnapshotHash::new(self.id(), snapshot_hashes));
        self.push_message(CrdsValue::new_signed(message, &self.keypair));
    }

    pub fn push_vote(&self, tower_index: usize, vote: Transaction) {
        let now = timestamp();
        let vote = Vote::new(&self.id(), vote, now);
        let vote_ix = {
            let r_gossip =
                self.time_gossip_read_lock("gossip_read_push_vote", &self.stats.push_vote_read);
            let current_votes: Vec<_> = (0..crds_value::MAX_VOTES)
                .filter_map(|ix| r_gossip.crds.lookup(&CrdsValueLabel::Vote(ix, self.id())))
                .collect();
            CrdsValue::compute_vote_index(tower_index, current_votes)
        };
        let entry = CrdsValue::new_signed(CrdsData::Vote(vote_ix, vote), &self.keypair);
        self.local_message_pending_push_queue
            .write()
            .unwrap()
            .push((entry, now));
    }

    pub fn send_vote(&self, vote: &Transaction) -> Result<()> {
        let tpu = self.my_contact_info().tpu;
        let buf = serialize(vote)?;
        self.socket.send_to(&buf, &tpu)?;
        Ok(())
    }

    /// Get votes in the crds
    /// * since - The timestamp of when the vote inserted must be greater than
    /// since. This allows the bank to query for new votes only.
    ///
    /// * return - The votes, and the max timestamp from the new set.
    pub fn get_votes(&self, since: u64) -> (Vec<CrdsValueLabel>, Vec<Transaction>, u64) {
        let mut max_ts = since;
        let (labels, txs): (Vec<CrdsValueLabel>, Vec<Transaction>) = self
            .time_gossip_read_lock("get_votes", &self.stats.get_votes)
            .crds
            .iter()
            .filter(|(_, x)| x.insert_timestamp > since)
            .filter_map(|(label, x)| {
                max_ts = std::cmp::max(x.insert_timestamp, max_ts);
                x.value
                    .vote()
                    .map(|v| (label.clone(), v.transaction.clone()))
            })
            .unzip();
        inc_new_counter_info!("cluster_info-get_votes-count", txs.len());
        (labels, txs, max_ts)
    }

    pub fn get_snapshot_hash(&self, slot: Slot) -> Vec<(Pubkey, Hash)> {
        self.time_gossip_read_lock("get_snapshot_hash", &self.stats.get_snapshot_hash)
            .crds
            .values()
            .filter_map(|x| x.value.snapshot_hash())
            .filter_map(|x| {
                for (table_slot, hash) in &x.hashes {
                    if *table_slot == slot {
                        return Some((x.from, *hash));
                    }
                }
                None
            })
            .collect()
    }

    pub fn get_accounts_hash_for_node<F, Y>(&self, pubkey: &Pubkey, map: F) -> Option<Y>
    where
        F: FnOnce(&Vec<(Slot, Hash)>) -> Y,
    {
        self.time_gossip_read_lock("get_accounts_hash", &self.stats.get_accounts_hash)
            .crds
            .get(&CrdsValueLabel::AccountsHashes(*pubkey))
            .map(|x| &x.value.accounts_hash().unwrap().hashes)
            .map(map)
    }

    pub fn get_snapshot_hash_for_node<F, Y>(&self, pubkey: &Pubkey, map: F) -> Option<Y>
    where
        F: FnOnce(&Vec<(Slot, Hash)>) -> Y,
    {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get(&CrdsValueLabel::SnapshotHashes(*pubkey))
            .map(|x| &x.value.snapshot_hash().unwrap().hashes)
            .map(map)
    }

    pub fn get_lowest_slot_for_node<F, Y>(
        &self,
        pubkey: &Pubkey,
        since: Option<u64>,
        map: F,
    ) -> Option<Y>
    where
        F: FnOnce(&LowestSlot, u64) -> Y,
    {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get(&CrdsValueLabel::LowestSlot(*pubkey))
            .filter(|x| {
                since
                    .map(|since| x.insert_timestamp > since)
                    .unwrap_or(true)
            })
            .map(|x| map(x.value.lowest_slot().unwrap(), x.insert_timestamp))
    }

    pub fn get_epoch_slots_since(&self, since: Option<u64>) -> (Vec<EpochSlots>, Option<u64>) {
        let vals: Vec<_> = self
            .gossip
            .read()
            .unwrap()
            .crds
            .values()
            .filter(|x| {
                since
                    .map(|since| x.insert_timestamp > since)
                    .unwrap_or(true)
            })
            .filter_map(|x| Some((x.value.epoch_slots()?.clone(), x.insert_timestamp)))
            .collect();
        let max = vals.iter().map(|x| x.1).max().or(since);
        let vec = vals.into_iter().map(|x| x.0).collect();
        (vec, max)
    }

    pub fn get_node_version(&self, pubkey: &Pubkey) -> Option<solana_version::Version> {
        let version = self
            .gossip
            .read()
            .unwrap()
            .crds
            .get(&CrdsValueLabel::Version(*pubkey))
            .map(|x| x.value.version())
            .flatten()
            .map(|version| version.version.clone());

        if version.is_none() {
            self.gossip
                .read()
                .unwrap()
                .crds
                .get(&CrdsValueLabel::LegacyVersion(*pubkey))
                .map(|x| x.value.legacy_version())
                .flatten()
                .map(|version| version.version.clone().into())
        } else {
            version
        }
    }

    /// all validators that have a valid rpc port regardless of `shred_version`.
    pub fn all_rpc_peers(&self) -> Vec<ContactInfo> {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get_nodes_contact_info()
            .filter(|x| x.id != self.id() && ContactInfo::is_valid_address(&x.rpc))
            .cloned()
            .collect()
    }

    // All nodes in gossip (including spy nodes) and the last time we heard about them
    pub(crate) fn all_peers(&self) -> Vec<(ContactInfo, u64)> {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get_nodes()
            .map(|x| (x.value.contact_info().unwrap().clone(), x.local_timestamp))
            .collect()
    }

    pub fn gossip_peers(&self) -> Vec<ContactInfo> {
        let me = self.id();
        self.gossip
            .read()
            .unwrap()
            .crds
            .get_nodes_contact_info()
            // shred_version not considered for gossip peers (ie, spy nodes do not set shred_version)
            .filter(|x| x.id != me && ContactInfo::is_valid_address(&x.gossip))
            .cloned()
            .collect()
    }

    /// all validators that have a valid tvu port regardless of `shred_version`.
    pub fn all_tvu_peers(&self) -> Vec<ContactInfo> {
        self.time_gossip_read_lock("all_tvu_peers", &self.stats.all_tvu_peers)
            .crds
            .get_nodes_contact_info()
            .filter(|x| ContactInfo::is_valid_address(&x.tvu) && x.id != self.id())
            .cloned()
            .collect()
    }

    /// all validators that have a valid tvu port and are on the same `shred_version`.
    pub fn tvu_peers(&self) -> Vec<ContactInfo> {
        let self_pubkey = self.id();
        let self_shred_version = self.my_shred_version();
        self.time_gossip_read_lock("tvu_peers", &self.stats.tvu_peers)
            .crds
            .get_nodes_contact_info()
            .filter(|node| {
                node.id != self_pubkey
                    && node.shred_version == self_shred_version
                    && ContactInfo::is_valid_address(&node.tvu)
            })
            .cloned()
            .collect()
    }

    /// all peers that have a valid tvu
    pub fn retransmit_peers(&self) -> Vec<ContactInfo> {
        self.time_gossip_read_lock("retransmit_peers", &self.stats.retransmit_peers)
            .crds
            .get_nodes_contact_info()
            .filter(|x| {
                x.id != self.id()
                    && x.shred_version == self.my_shred_version()
                    && ContactInfo::is_valid_address(&x.tvu)
                    && ContactInfo::is_valid_address(&x.tvu_forwards)
            })
            .cloned()
            .collect()
    }

    /// all tvu peers with valid gossip addrs that likely have the slot being requested
    pub fn repair_peers(&self, slot: Slot) -> Vec<ContactInfo> {
        let mut time = Measure::start("repair_peers");
        // self.tvu_peers() already filters on:
        //   node.id != self.id() &&
        //     node.shred_verion == self.my_shred_version()
        let nodes = self.tvu_peers();
        let nodes = {
            let gossip = self.gossip.read().unwrap();
            nodes
                .into_iter()
                .filter(|node| {
                    ContactInfo::is_valid_address(&node.serve_repair)
                        && match gossip.crds.get_lowest_slot(node.id) {
                            None => true, // fallback to legacy behavior
                            Some(lowest_slot) => lowest_slot.lowest <= slot,
                        }
                })
                .collect()
        };
        self.stats.repair_peers.add_measure(&mut time);
        nodes
    }

    fn is_spy_node(contact_info: &ContactInfo) -> bool {
        !ContactInfo::is_valid_address(&contact_info.tpu)
            || !ContactInfo::is_valid_address(&contact_info.gossip)
            || !ContactInfo::is_valid_address(&contact_info.tvu)
    }

    fn sorted_stakes_with_index<S: std::hash::BuildHasher>(
        peers: &[ContactInfo],
        stakes: Option<Arc<HashMap<Pubkey, u64, S>>>,
    ) -> Vec<(u64, usize)> {
        let stakes_and_index: Vec<_> = peers
            .iter()
            .enumerate()
            .map(|(i, c)| {
                // For stake weighted shuffle a valid weight is atleast 1. Weight 0 is
                // assumed to be missing entry. So let's make sure stake weights are atleast 1
                let stake = 1.max(
                    stakes
                        .as_ref()
                        .map_or(1, |stakes| *stakes.get(&c.id).unwrap_or(&1)),
                );
                (stake, i)
            })
            .sorted_by(|(l_stake, l_info), (r_stake, r_info)| {
                if r_stake == l_stake {
                    peers[*r_info].id.cmp(&peers[*l_info].id)
                } else {
                    r_stake.cmp(&l_stake)
                }
            })
            .collect();

        stakes_and_index
    }

    fn stake_weighted_shuffle(
        stakes_and_index: &[(u64, usize)],
        seed: [u8; 32],
    ) -> Vec<(u64, usize)> {
        let stake_weights = stakes_and_index.iter().map(|(w, _)| *w).collect();

        let shuffle = weighted_shuffle(stake_weights, seed);

        shuffle.iter().map(|x| stakes_and_index[*x]).collect()
    }

    // Return sorted_retransmit_peers(including self) and their stakes
    pub fn sorted_retransmit_peers_and_stakes(
        &self,
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    ) -> (Vec<ContactInfo>, Vec<(u64, usize)>) {
        let mut peers = self.retransmit_peers();
        // insert "self" into this list for the layer and neighborhood computation
        peers.push(self.my_contact_info());
        let stakes_and_index = ClusterInfo::sorted_stakes_with_index(&peers, stakes);
        (peers, stakes_and_index)
    }

    /// Return sorted Retransmit peers and index of `Self.id()` as if it were in that list
    pub fn shuffle_peers_and_index(
        id: &Pubkey,
        peers: &[ContactInfo],
        stakes_and_index: &[(u64, usize)],
        seed: [u8; 32],
    ) -> (usize, Vec<(u64, usize)>) {
        let shuffled_stakes_and_index = ClusterInfo::stake_weighted_shuffle(stakes_and_index, seed);
        let mut self_index = 0;
        shuffled_stakes_and_index
            .iter()
            .enumerate()
            .for_each(|(i, (_stake, index))| {
                if &peers[*index].id == id {
                    self_index = i;
                }
            });
        (self_index, shuffled_stakes_and_index)
    }

    /// compute broadcast table
    pub fn tpu_peers(&self) -> Vec<ContactInfo> {
        self.gossip
            .read()
            .unwrap()
            .crds
            .get_nodes_contact_info()
            .filter(|x| x.id != self.id() && ContactInfo::is_valid_address(&x.tpu))
            .cloned()
            .collect()
    }

    /// Given a node count and fanout, it calculates how many layers are needed and at what index each layer begins.
    pub fn describe_data_plane(nodes: usize, fanout: usize) -> (usize, Vec<usize>) {
        let mut layer_indices: Vec<usize> = vec![0];
        if nodes == 0 {
            (0, vec![])
        } else if nodes <= fanout {
            // single layer data plane
            (1, layer_indices)
        } else {
            //layer 1 is going to be the first num fanout nodes, so exclude those
            let mut remaining_nodes = nodes - fanout;
            layer_indices.push(fanout);
            let mut num_layers = 2;
            // fanout * num_nodes in a neighborhood, which is also fanout.
            let mut layer_capacity = fanout * fanout;
            while remaining_nodes > 0 {
                if remaining_nodes > layer_capacity {
                    // Needs more layers.
                    num_layers += 1;
                    remaining_nodes -= layer_capacity;
                    let end = *layer_indices.last().unwrap();
                    layer_indices.push(layer_capacity + end);

                    // Next layer's capacity
                    layer_capacity *= fanout;
                } else {
                    //everything will now fit in the layers we have
                    let end = *layer_indices.last().unwrap();
                    layer_indices.push(layer_capacity + end);
                    break;
                }
            }
            assert_eq!(num_layers, layer_indices.len() - 1);
            (num_layers, layer_indices)
        }
    }

    fn localize_item(
        layer_indices: &[usize],
        fanout: usize,
        select_index: usize,
        curr_index: usize,
    ) -> Option<Locality> {
        let end = layer_indices.len() - 1;
        let next = min(end, curr_index + 1);
        let layer_start = layer_indices[curr_index];
        // localized if selected index lies within the current layer's bounds
        let localized = select_index >= layer_start && select_index < layer_indices[next];
        if localized {
            let mut locality = Locality::default();
            let hood_ix = (select_index - layer_start) / fanout;
            match curr_index {
                _ if curr_index == 0 => {
                    locality.layer_ix = 0;
                    locality.layer_bounds = (0, fanout);
                    locality.neighbor_bounds = locality.layer_bounds;

                    if next == end {
                        locality.next_layer_bounds = None;
                        locality.next_layer_peers = vec![];
                    } else {
                        locality.next_layer_bounds =
                            Some((layer_indices[next], layer_indices[next + 1]));
                        locality.next_layer_peers = ClusterInfo::next_layer_peers(
                            select_index,
                            hood_ix,
                            layer_indices[next],
                            fanout,
                        );
                    }
                }
                _ if curr_index == end => {
                    locality.layer_ix = end;
                    locality.layer_bounds = (end - fanout, end);
                    locality.neighbor_bounds = locality.layer_bounds;
                    locality.next_layer_bounds = None;
                    locality.next_layer_peers = vec![];
                }
                ix => {
                    locality.layer_ix = ix;
                    locality.layer_bounds = (layer_start, layer_indices[next]);
                    locality.neighbor_bounds = (
                        ((hood_ix * fanout) + layer_start),
                        ((hood_ix + 1) * fanout + layer_start),
                    );

                    if next == end {
                        locality.next_layer_bounds = None;
                        locality.next_layer_peers = vec![];
                    } else {
                        locality.next_layer_bounds =
                            Some((layer_indices[next], layer_indices[next + 1]));
                        locality.next_layer_peers = ClusterInfo::next_layer_peers(
                            select_index,
                            hood_ix,
                            layer_indices[next],
                            fanout,
                        );
                    }
                }
            }
            Some(locality)
        } else {
            None
        }
    }

    /// Given a array of layer indices and an index of interest, returns (as a `Locality`) the layer,
    /// layer-bounds, and neighborhood-bounds in which the index resides
    fn localize(layer_indices: &[usize], fanout: usize, select_index: usize) -> Locality {
        (0..layer_indices.len())
            .find_map(|i| ClusterInfo::localize_item(layer_indices, fanout, select_index, i))
            .or_else(|| Some(Locality::default()))
            .unwrap()
    }

    /// Selects a range in the next layer and chooses nodes from that range as peers for the given index
    fn next_layer_peers(index: usize, hood_ix: usize, start: usize, fanout: usize) -> Vec<usize> {
        // Each neighborhood is only tasked with pushing to `fanout` neighborhoods where each neighborhood contains `fanout` nodes.
        let fanout_nodes = fanout * fanout;
        // Skip first N nodes, where N is hood_ix * (fanout_nodes)
        let start = start + (hood_ix * fanout_nodes);
        let end = start + fanout_nodes;
        (start..end)
            .step_by(fanout)
            .map(|x| x + index % fanout)
            .collect()
    }

    /// retransmit messages to a list of nodes
    /// # Remarks
    /// We need to avoid having obj locked while doing a io, such as the `send_to`
    pub fn retransmit_to(
        peers: &[&ContactInfo],
        packet: &mut Packet,
        slot_leader_pubkey: Option<Pubkey>,
        s: &UdpSocket,
        forwarded: bool,
    ) -> Result<()> {
        trace!("retransmit orders {}", peers.len());
        let dests: Vec<_> = peers
            .iter()
            .filter(|v| v.id != slot_leader_pubkey.unwrap_or_default())
            .map(|v| if forwarded { &v.tvu_forwards } else { &v.tvu })
            .collect();

        let mut sent = 0;
        while sent < dests.len() {
            match multicast(s, &mut packet.data[..packet.meta.size], &dests[sent..]) {
                Ok(n) => sent += n,
                Err(e) => {
                    inc_new_counter_error!(
                        "cluster_info-retransmit-send_to_error",
                        dests.len() - sent,
                        1
                    );
                    error!("retransmit result {:?}", e);
                    return Err(Error::IO(e));
                }
            }
        }
        Ok(())
    }

    fn insert_self(&self) {
        let value =
            CrdsValue::new_signed(CrdsData::ContactInfo(self.my_contact_info()), &self.keypair);
        let _ = self.gossip.write().unwrap().crds.insert(value, timestamp());
    }

    // If the network entrypoint hasn't been discovered yet, add it to the crds table
    fn append_entrypoint_to_pulls(
        &self,
        thread_pool: &ThreadPool,
        pulls: &mut Vec<(Pubkey, CrdsFilter, SocketAddr, CrdsValue)>,
    ) {
        let pull_from_entrypoint = {
            let mut w_entrypoint = self.entrypoint.write().unwrap();
            if let Some(ref mut entrypoint) = &mut *w_entrypoint {
                if pulls.is_empty() {
                    // Nobody else to pull from, try the entrypoint
                    true
                } else {
                    let now = timestamp();
                    // Only consider pulling from the entrypoint periodically to avoid spamming it
                    if timestamp() - entrypoint.wallclock <= CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 2 {
                        false
                    } else {
                        entrypoint.wallclock = now;
                        let found_entrypoint = self
                            .time_gossip_read_lock("entrypoint", &self.stats.entrypoint)
                            .crds
                            .get_nodes_contact_info()
                            .any(|node| node.gossip == entrypoint.gossip);
                        !found_entrypoint
                    }
                }
            } else {
                false
            }
        };

        if pull_from_entrypoint {
            let id_and_gossip = {
                self.entrypoint
                    .read()
                    .unwrap()
                    .as_ref()
                    .map(|e| (e.id, e.gossip))
            };
            if let Some((id, gossip)) = id_and_gossip {
                let r_gossip = self.time_gossip_read_lock("entrypoint", &self.stats.entrypoint2);
                let self_info = r_gossip
                    .crds
                    .lookup(&CrdsValueLabel::ContactInfo(self.id()))
                    .unwrap_or_else(|| panic!("self_id invalid {}", self.id()));
                r_gossip
                    .pull
                    .build_crds_filters(thread_pool, &r_gossip.crds, MAX_BLOOM_SIZE)
                    .into_iter()
                    .for_each(|filter| pulls.push((id, filter, gossip, self_info.clone())));
            }
        }
    }

    /// Splits an input feed of serializable data into chunks where the sum of
    /// serialized size of values within each chunk is no larger than
    /// max_chunk_size.
    /// Note: some messages cannot be contained within that size so in the worst case this returns
    /// N nested Vecs with 1 item each.
    fn split_gossip_messages<I, T>(
        max_chunk_size: usize,
        data_feed: I,
    ) -> impl Iterator<Item = Vec<T>>
    where
        T: Serialize + Debug,
        I: IntoIterator<Item = T>,
    {
        let mut data_feed = data_feed.into_iter().fuse();
        let mut buffer = vec![];
        let mut buffer_size = 0; // Serialized size of buffered values.
        std::iter::from_fn(move || loop {
            match data_feed.next() {
                None => {
                    return if buffer.is_empty() {
                        None
                    } else {
                        Some(std::mem::take(&mut buffer))
                    };
                }
                Some(data) => {
                    let data_size = match serialized_size(&data) {
                        Ok(size) => size as usize,
                        Err(err) => {
                            error!("serialized_size failed: {}", err);
                            continue;
                        }
                    };
                    if buffer_size + data_size <= max_chunk_size {
                        buffer_size += data_size;
                        buffer.push(data);
                    } else if data_size <= max_chunk_size {
                        buffer_size = data_size;
                        return Some(std::mem::replace(&mut buffer, vec![data]));
                    } else {
                        error!(
                            "dropping data larger than the maximum chunk size {:?}",
                            data
                        );
                    }
                }
            }
        })
    }

    fn new_pull_requests(
        &self,
        thread_pool: &ThreadPool,
        gossip_validators: Option<&HashSet<Pubkey>>,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Vec<(SocketAddr, Protocol)> {
        let now = timestamp();
        let mut pulls: Vec<_> = {
            let r_gossip =
                self.time_gossip_read_lock("new_pull_reqs", &self.stats.new_pull_requests);
            r_gossip
                .new_pull_request(thread_pool, now, gossip_validators, stakes, MAX_BLOOM_SIZE)
                .ok()
                .into_iter()
                .filter_map(|(peer, filters, me)| {
                    let peer_label = CrdsValueLabel::ContactInfo(peer);
                    r_gossip
                        .crds
                        .lookup(&peer_label)
                        .and_then(CrdsValue::contact_info)
                        .map(move |peer_info| {
                            filters
                                .into_iter()
                                .map(move |f| (peer, f, peer_info.gossip, me.clone()))
                        })
                })
                .flatten()
                .collect()
        };
        self.append_entrypoint_to_pulls(thread_pool, &mut pulls);
        self.stats
            .new_pull_requests_count
            .add_relaxed(pulls.len() as u64);
        // There are at most 2 unique peers here: The randomly
        // selected pull peer, and possibly also the entrypoint.
        let peers: Vec<Pubkey> = pulls.iter().map(|(peer, _, _, _)| *peer).dedup().collect();
        {
            let mut gossip =
                self.time_gossip_write_lock("mark_pull", &self.stats.mark_pull_request);
            for peer in peers {
                gossip.mark_pull_request_creation_time(&peer, now);
            }
        }
        pulls
            .into_iter()
            .map(|(_, filter, gossip, self_info)| {
                (gossip, Protocol::PullRequest(filter, self_info))
            })
            .collect()
    }
    fn drain_push_queue(&self) -> Vec<(CrdsValue, u64)> {
        let mut push_queue = self.local_message_pending_push_queue.write().unwrap();
        std::mem::take(&mut *push_queue)
    }
    #[cfg(test)]
    pub fn flush_push_queue(&self) {
        let pending_push_messages = self.drain_push_queue();
        let mut gossip = self.gossip.write().unwrap();
        gossip.process_push_messages(pending_push_messages);
    }
    fn new_push_requests(&self) -> Vec<(SocketAddr, Protocol)> {
        let self_id = self.id();
        let (_, push_messages) = self
            .time_gossip_write_lock("new_push_requests", &self.stats.new_push_requests)
            .new_push_messages(self.drain_push_queue(), timestamp());
        let push_messages: Vec<_> = {
            let gossip =
                self.time_gossip_read_lock("push_req_lookup", &self.stats.new_push_requests2);
            push_messages
                .into_iter()
                .filter_map(|(pubkey, messages)| {
                    let peer = gossip.crds.get_contact_info(pubkey)?;
                    Some((peer.gossip, messages))
                })
                .collect()
        };
        let messages: Vec<_> = push_messages
            .into_iter()
            .flat_map(|(peer, msgs)| {
                Self::split_gossip_messages(PUSH_MESSAGE_MAX_PAYLOAD_SIZE, msgs)
                    .map(move |payload| (peer, Protocol::PushMessage(self_id, payload)))
            })
            .collect();
        self.stats
            .new_push_requests_num
            .add_relaxed(messages.len() as u64);
        messages
    }

    // Generate new push and pull requests
    fn generate_new_gossip_requests(
        &self,
        thread_pool: &ThreadPool,
        gossip_validators: Option<&HashSet<Pubkey>>,
        stakes: &HashMap<Pubkey, u64>,
        generate_pull_requests: bool,
    ) -> Vec<(SocketAddr, Protocol)> {
        let mut pulls: Vec<_> = if generate_pull_requests {
            self.new_pull_requests(&thread_pool, gossip_validators, stakes)
        } else {
            vec![]
        };
        let mut pushes: Vec<_> = self.new_push_requests();

        pulls.append(&mut pushes);
        pulls
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(
        &self,
        thread_pool: &ThreadPool,
        gossip_validators: Option<&HashSet<Pubkey>>,
        recycler: &PacketsRecycler,
        stakes: &HashMap<Pubkey, u64>,
        sender: &PacketSender,
        generate_pull_requests: bool,
    ) -> Result<()> {
        let reqs = self.generate_new_gossip_requests(
            thread_pool,
            gossip_validators,
            &stakes,
            generate_pull_requests,
        );
        if !reqs.is_empty() {
            let packets = to_packets_with_destination(recycler.clone(), &reqs);
            sender.send(packets)?;
        }
        Ok(())
    }

    fn process_entrypoint(&self, entrypoint_processed: &mut bool) {
        if *entrypoint_processed {
            return;
        }

        let gossip_addr = self.entrypoint.read().unwrap().as_ref().map(|e| e.gossip);
        if let Some(gossip_addr) = gossip_addr {
            // If a pull from the entrypoint was successful it should exist in the CRDS table
            let entrypoint = self.lookup_contact_info_by_gossip_addr(&gossip_addr);

            if let Some(entrypoint) = entrypoint {
                // Adopt the entrypoint's `shred_version` if ours is unset
                if self.my_shred_version() == 0 {
                    if entrypoint.shred_version == 0 {
                        warn!("Unable to adopt entrypoint shred version of 0");
                    } else {
                        info!(
                            "Setting shred version to {:?} from entrypoint {:?}",
                            entrypoint.shred_version, entrypoint.id
                        );
                        self.my_contact_info.write().unwrap().shred_version =
                            entrypoint.shred_version;
                        self.gossip
                            .write()
                            .unwrap()
                            .set_shred_version(entrypoint.shred_version);
                        self.insert_self();
                        *entrypoint_processed = true;
                    }
                } else {
                    *entrypoint_processed = true;
                }

                // Update the entrypoint's id so future entrypoint pulls correctly reference it
                *self.entrypoint.write().unwrap() = Some(entrypoint);
            }
        } else {
            // No entrypoint specified.  Nothing more to process
            *entrypoint_processed = true;
        }
    }

    fn handle_purge(
        &self,
        thread_pool: &ThreadPool,
        bank_forks: &Option<Arc<RwLock<BankForks>>>,
        stakes: &HashMap<Pubkey, u64>,
    ) {
        let timeout = {
            if let Some(ref bank_forks) = bank_forks {
                let bank = bank_forks.read().unwrap().working_bank();
                let epoch = bank.epoch();
                let epoch_schedule = bank.epoch_schedule();
                epoch_schedule.get_slots_in_epoch(epoch) * DEFAULT_MS_PER_SLOT
            } else {
                inc_new_counter_info!("cluster_info-purge-no_working_bank", 1);
                CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS
            }
        };
        let timeouts = self.gossip.read().unwrap().make_timeouts(stakes, timeout);
        let num_purged = self
            .time_gossip_write_lock("purge", &self.stats.purge)
            .purge(thread_pool, timestamp(), &timeouts);
        inc_new_counter_info!("cluster_info-purge-count", num_purged);
    }

    /// randomly pick a node and ask them for updates asynchronously
    pub fn gossip(
        self: Arc<Self>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        sender: PacketSender,
        gossip_validators: Option<HashSet<Pubkey>>,
        exit: &Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        let thread_pool = ThreadPoolBuilder::new()
            .num_threads(std::cmp::min(get_thread_count(), 8))
            .thread_name(|i| format!("ClusterInfo::gossip-{}", i))
            .build()
            .unwrap();
        Builder::new()
            .name("solana-gossip".to_string())
            .spawn(move || {
                let mut last_push = timestamp();
                let mut last_contact_info_trace = timestamp();
                let mut entrypoint_processed = false;
                let recycler = PacketsRecycler::default();
                let crds_data = vec![
                    CrdsData::Version(Version::new(self.id())),
                    CrdsData::NodeInstance(self.instance.with_wallclock(timestamp())),
                ];
                for value in crds_data {
                    let value = CrdsValue::new_signed(value, &self.keypair);
                    self.push_message(value);
                }
                let mut generate_pull_requests = true;
                loop {
                    let start = timestamp();
                    thread_mem_usage::datapoint("solana-gossip");
                    if self.contact_debug_interval != 0
                        && start - last_contact_info_trace > self.contact_debug_interval
                    {
                        // Log contact info every 10 seconds
                        info!(
                            "\n{}\n\n{}",
                            self.contact_info_trace(),
                            self.rpc_info_trace()
                        );
                        last_contact_info_trace = start;
                    }

                    let stakes: HashMap<_, _> = match bank_forks {
                        Some(ref bank_forks) => {
                            bank_forks.read().unwrap().working_bank().staked_nodes()
                        }
                        None => HashMap::new(),
                    };

                    let _ = self.run_gossip(
                        &thread_pool,
                        gossip_validators.as_ref(),
                        &recycler,
                        &stakes,
                        &sender,
                        generate_pull_requests,
                    );
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }

                    self.handle_purge(&thread_pool, &bank_forks, &stakes);

                    self.process_entrypoint(&mut entrypoint_processed);

                    //TODO: possibly tune this parameter
                    //we saw a deadlock passing an self.read().unwrap().timeout into sleep
                    if start - last_push > CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 2 {
                        self.push_self(&stakes, gossip_validators.as_ref());
                        last_push = timestamp();
                    }
                    let elapsed = timestamp() - start;
                    if GOSSIP_SLEEP_MILLIS > elapsed {
                        let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                        sleep(Duration::from_millis(time_left));
                    }
                    generate_pull_requests = !generate_pull_requests;
                }
            })
            .unwrap()
    }

    fn handle_batch_prune_messages(&self, messages: Vec<(Pubkey, PruneData)>) {
        let _st = ScopedTimer::from(&self.stats.handle_batch_prune_messages_time);
        if messages.is_empty() {
            return;
        }
        self.stats
            .prune_message_count
            .add_relaxed(messages.len() as u64);
        self.stats.prune_message_len.add_relaxed(
            messages
                .iter()
                .map(|(_, data)| data.prunes.len() as u64)
                .sum(),
        );
        let mut prune_message_timeout = 0;
        let mut bad_prune_destination = 0;
        {
            let gossip = self.time_gossip_read_lock("process_prune", &self.stats.process_prune);
            let now = timestamp();
            for (from, data) in messages {
                match gossip.process_prune_msg(
                    &from,
                    &data.destination,
                    &data.prunes,
                    data.wallclock,
                    now,
                ) {
                    Err(CrdsGossipError::PruneMessageTimeout) => {
                        prune_message_timeout += 1;
                    }
                    Err(CrdsGossipError::BadPruneDestination) => {
                        bad_prune_destination += 1;
                    }
                    _ => (),
                }
            }
        }
        if prune_message_timeout != 0 {
            inc_new_counter_debug!("cluster_info-prune_message_timeout", prune_message_timeout);
        }
        if bad_prune_destination != 0 {
            inc_new_counter_debug!("cluster_info-bad_prune_destination", bad_prune_destination);
        }
    }

    fn handle_batch_pull_requests(
        &self,
        // from address, crds filter, caller contact info
        requests: Vec<(SocketAddr, CrdsFilter, CrdsValue)>,
        thread_pool: &ThreadPool,
        recycler: &PacketsRecycler,
        stakes: &HashMap<Pubkey, u64>,
        response_sender: &PacketSender,
        feature_set: Option<&FeatureSet>,
    ) {
        let _st = ScopedTimer::from(&self.stats.handle_batch_pull_requests_time);
        if requests.is_empty() {
            return;
        }
        let self_pubkey = self.id();
        let self_shred_version = self.my_shred_version();
        let requests: Vec<_> = thread_pool.install(|| {
            requests
                .into_par_iter()
                .with_min_len(1024)
                .filter(|(_, _, caller)| match caller.contact_info() {
                    None => false,
                    Some(caller) if caller.id == self_pubkey => {
                        warn!("PullRequest ignored, I'm talking to myself");
                        inc_new_counter_debug!("cluster_info-window-request-loopback", 1);
                        false
                    }
                    Some(caller) => {
                        if self_shred_version != 0
                            && caller.shred_version != 0
                            && caller.shred_version != self_shred_version
                        {
                            self.stats.skip_pull_shred_version.add_relaxed(1);
                            false
                        } else {
                            true
                        }
                    }
                })
                .map(|(from_addr, filter, caller)| PullData {
                    from_addr,
                    caller,
                    filter,
                })
                .collect()
        });
        if !requests.is_empty() {
            self.stats
                .pull_requests_count
                .add_relaxed(requests.len() as u64);
            let response = self.handle_pull_requests(recycler, requests, stakes, feature_set);
            if !response.is_empty() {
                let _ = response_sender.send(response);
            }
        }
    }

    fn update_data_budget(&self, num_staked: usize) -> usize {
        const INTERVAL_MS: u64 = 100;
        // allow 50kBps per staked validator, epoch slots + votes ~= 1.5kB/slot ~= 4kB/s
        const BYTES_PER_INTERVAL: usize = 5000;
        const MAX_BUDGET_MULTIPLE: usize = 5; // allow budget build-up to 5x the interval default
        let num_staked = num_staked.max(2);
        self.outbound_budget.update(INTERVAL_MS, |bytes| {
            std::cmp::min(
                bytes + num_staked * BYTES_PER_INTERVAL,
                MAX_BUDGET_MULTIPLE * num_staked * BYTES_PER_INTERVAL,
            )
        })
    }

    // Returns a predicate checking if the pull request is from a valid
    // address, and if the address have responded to a ping request. Also
    // appends ping packets for the addresses which need to be (re)verified.
    fn check_pull_request<'a, R>(
        &'a self,
        now: Instant,
        mut rng: &'a mut R,
        packets: &'a mut Packets,
        feature_set: Option<&FeatureSet>,
    ) -> impl FnMut(&PullData) -> bool + 'a
    where
        R: Rng + CryptoRng,
    {
        let check_enabled = matches!(feature_set, Some(feature_set) if
            feature_set.is_active(&feature_set::pull_request_ping_pong_check::id()));
        let mut cache = HashMap::<(Pubkey, SocketAddr), bool>::new();
        let mut pingf = move || Ping::new_rand(&mut rng, &self.keypair).ok();
        let mut ping_cache = self.ping_cache.write().unwrap();
        let mut hard_check = move |node| {
            let (check, ping) = ping_cache.check(now, node, &mut pingf);
            if let Some(ping) = ping {
                let ping = Protocol::PingMessage(ping);
                match Packet::from_data(&node.1, ping) {
                    Ok(packet) => packets.packets.push(packet),
                    Err(err) => error!("failed to write ping packet: {:?}", err),
                };
            }
            if !check {
                self.stats
                    .pull_request_ping_pong_check_failed_count
                    .add_relaxed(1)
            }
            check || !check_enabled
        };
        // Because pull-responses are sent back to packet.meta.addr() of
        // incoming pull-requests, pings are also sent to request.from_addr (as
        // opposed to caller.gossip address).
        move |request| {
            ContactInfo::is_valid_address(&request.from_addr) && {
                let node = (request.caller.pubkey(), request.from_addr);
                *cache.entry(node).or_insert_with(|| hard_check(node))
            }
        }
    }

    // Pull requests take an incoming bloom filter of contained entries from a node
    // and tries to send back to them the values it detects are missing.
    fn handle_pull_requests(
        &self,
        recycler: &PacketsRecycler,
        requests: Vec<PullData>,
        stakes: &HashMap<Pubkey, u64>,
        feature_set: Option<&FeatureSet>,
    ) -> Packets {
        let mut time = Measure::start("handle_pull_requests");
        let callers = crds_value::filter_current(requests.iter().map(|r| &r.caller));
        self.time_gossip_write_lock("process_pull_reqs", &self.stats.process_pull_requests)
            .process_pull_requests(callers.cloned(), timestamp());
        let output_size_limit =
            self.update_data_budget(stakes.len()) / PULL_RESPONSE_MIN_SERIALIZED_SIZE;
        let mut packets = Packets::new_with_recycler(recycler.clone(), 64, "handle_pull_requests");
        let (caller_and_filters, addrs): (Vec<_>, Vec<_>) = {
            let mut rng = rand::thread_rng();
            let check_pull_request =
                self.check_pull_request(Instant::now(), &mut rng, &mut packets, feature_set);
            requests
                .into_iter()
                .filter(check_pull_request)
                .map(|r| ((r.caller, r.filter), r.from_addr))
                .unzip()
        };
        let now = timestamp();
        let self_id = self.id();

        let pull_responses = self
            .time_gossip_read_lock(
                "generate_pull_responses",
                &self.stats.generate_pull_responses,
            )
            .generate_pull_responses(&caller_and_filters, output_size_limit, now);

        let pull_responses: Vec<_> = pull_responses
            .into_iter()
            .zip(addrs.into_iter())
            .filter(|(response, _)| !response.is_empty())
            .collect();

        if pull_responses.is_empty() {
            return packets;
        }

        let mut stats: Vec<_> = pull_responses
            .iter()
            .enumerate()
            .map(|(i, (responses, _from_addr))| {
                let score: u64 = if stakes.get(&responses[0].pubkey()).is_some() {
                    2
                } else {
                    1
                };
                responses
                    .iter()
                    .enumerate()
                    .map(|(j, _response)| ResponseScore {
                        to: i,
                        responses_index: j,
                        score,
                    })
                    .collect::<Vec<ResponseScore>>()
            })
            .flatten()
            .collect();

        stats.sort_by(|a, b| a.score.cmp(&b.score));
        let weights: Vec<_> = stats.iter().map(|stat| stat.score).collect();

        let seed = [48u8; 32];
        let rng = &mut ChaChaRng::from_seed(seed);
        let weighted_index = WeightedIndex::new(weights).unwrap();

        let mut total_bytes = 0;
        let mut sent = HashSet::new();
        while sent.len() < stats.len() {
            let index = weighted_index.sample(rng);
            if sent.contains(&index) {
                continue;
            }
            let stat = &stats[index];
            let from_addr = pull_responses[stat.to].1;
            let response = pull_responses[stat.to].0[stat.responses_index].clone();
            let protocol = Protocol::PullResponse(self_id, vec![response]);
            match Packet::from_data(&from_addr, protocol) {
                Err(err) => error!("failed to write pull-response packet: {:?}", err),
                Ok(packet) => {
                    if self.outbound_budget.take(packet.meta.size) {
                        sent.insert(index);
                        total_bytes += packet.meta.size;
                        packets.packets.push(packet)
                    } else {
                        inc_new_counter_info!("gossip_pull_request-no_budget", 1);
                        break;
                    }
                }
            }
        }
        time.stop();
        inc_new_counter_info!("gossip_pull_request-sent_requests", sent.len());
        inc_new_counter_info!(
            "gossip_pull_request-dropped_requests",
            stats.len() - sent.len()
        );
        debug!(
            "handle_pull_requests: {} sent: {} total: {} total_bytes: {}",
            time,
            sent.len(),
            stats.len(),
            total_bytes
        );
        packets
    }

    fn handle_batch_pull_responses(
        &self,
        responses: Vec<(Pubkey, Vec<CrdsValue>)>,
        thread_pool: &ThreadPool,
        stakes: &HashMap<Pubkey, u64>,
        epoch_time_ms: u64,
    ) {
        let _st = ScopedTimer::from(&self.stats.handle_batch_pull_responses_time);
        if responses.is_empty() {
            return;
        }
        fn extend<K, V>(hash_map: &mut HashMap<K, Vec<V>>, (key, mut value): (K, Vec<V>))
        where
            K: Eq + std::hash::Hash,
        {
            match hash_map.entry(key) {
                Entry::Occupied(mut entry) => {
                    let entry_value = entry.get_mut();
                    if entry_value.len() < value.len() {
                        std::mem::swap(entry_value, &mut value);
                    }
                    entry_value.extend(value);
                }
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }
        fn merge<K, V>(
            mut hash_map: HashMap<K, Vec<V>>,
            other: HashMap<K, Vec<V>>,
        ) -> HashMap<K, Vec<V>>
        where
            K: Eq + std::hash::Hash,
        {
            if hash_map.len() < other.len() {
                return merge(other, hash_map);
            }
            for kv in other {
                extend(&mut hash_map, kv);
            }
            hash_map
        }
        let responses = thread_pool.install(|| {
            responses
                .into_par_iter()
                .with_min_len(1024)
                .fold(HashMap::new, |mut hash_map, kv| {
                    extend(&mut hash_map, kv);
                    hash_map
                })
                .reduce(HashMap::new, merge)
        });
        if !responses.is_empty() {
            let timeouts = self
                .gossip
                .read()
                .unwrap()
                .make_timeouts(&stakes, epoch_time_ms);
            for (from, data) in responses {
                self.handle_pull_response(&from, data, &timeouts);
            }
        }
    }

    // Returns (failed, timeout, success)
    fn handle_pull_response(
        &self,
        from: &Pubkey,
        mut crds_values: Vec<CrdsValue>,
        timeouts: &HashMap<Pubkey, u64>,
    ) -> (usize, usize, usize) {
        let len = crds_values.len();
        trace!("PullResponse me: {} from: {} len={}", self.id, from, len);
        let shred_version = self
            .lookup_contact_info(from, |ci| ci.shred_version)
            .unwrap_or(0);
        Self::filter_by_shred_version(
            from,
            &mut crds_values,
            shred_version,
            self.my_shred_version(),
        );
        let filtered_len = crds_values.len();

        let mut pull_stats = ProcessPullStats::default();
        let (filtered_pulls, filtered_pulls_expired_timeout, failed_inserts) = self
            .time_gossip_read_lock("filter_pull_resp", &self.stats.filter_pull_response)
            .filter_pull_responses(timeouts, crds_values, timestamp(), &mut pull_stats);

        if !filtered_pulls.is_empty()
            || !filtered_pulls_expired_timeout.is_empty()
            || !failed_inserts.is_empty()
        {
            self.time_gossip_write_lock("process_pull_resp", &self.stats.process_pull_response)
                .process_pull_responses(
                    from,
                    filtered_pulls,
                    filtered_pulls_expired_timeout,
                    failed_inserts,
                    timestamp(),
                    &mut pull_stats,
                );
        }

        self.stats
            .skip_pull_response_shred_version
            .add_relaxed((len - filtered_len) as u64);
        self.stats.process_pull_response_count.add_relaxed(1);
        self.stats
            .process_pull_response_len
            .add_relaxed(filtered_len as u64);
        self.stats
            .process_pull_response_timeout
            .add_relaxed(pull_stats.timeout_count as u64);
        self.stats
            .process_pull_response_fail_insert
            .add_relaxed(pull_stats.failed_insert as u64);
        self.stats
            .process_pull_response_fail_timeout
            .add_relaxed(pull_stats.failed_timeout as u64);
        self.stats
            .process_pull_response_success
            .add_relaxed(pull_stats.success as u64);

        (
            pull_stats.failed_insert + pull_stats.failed_timeout,
            pull_stats.timeout_count,
            pull_stats.success,
        )
    }

    fn filter_by_shred_version(
        from: &Pubkey,
        crds_values: &mut Vec<CrdsValue>,
        shred_version: u16,
        my_shred_version: u16,
    ) {
        // Always run filter on spies
        if my_shred_version != 0 && shred_version != my_shred_version {
            // Allow someone to update their own ContactInfo so they
            // can change shred versions if needed.
            crds_values.retain(|crds_value| match &crds_value.data {
                CrdsData::ContactInfo(contact_info) => contact_info.id == *from,
                _ => false,
            });
        }
    }

    fn handle_batch_ping_messages<I>(
        &self,
        pings: I,
        recycler: &PacketsRecycler,
        response_sender: &PacketSender,
    ) where
        I: IntoIterator<Item = (SocketAddr, Ping)>,
    {
        let _st = ScopedTimer::from(&self.stats.handle_batch_ping_messages_time);
        if let Some(response) = self.handle_ping_messages(pings, recycler) {
            let _ = response_sender.send(response);
        }
    }

    fn handle_ping_messages<I>(&self, pings: I, recycler: &PacketsRecycler) -> Option<Packets>
    where
        I: IntoIterator<Item = (SocketAddr, Ping)>,
    {
        let packets: Vec<_> = pings
            .into_iter()
            .filter_map(|(addr, ping)| {
                let pong = Pong::new(&ping, &self.keypair).ok()?;
                let pong = Protocol::PongMessage(pong);
                match Packet::from_data(&addr, pong) {
                    Ok(packet) => Some(packet),
                    Err(err) => {
                        error!("failed to write pong packet: {:?}", err);
                        None
                    }
                }
            })
            .collect();
        if packets.is_empty() {
            None
        } else {
            let packets =
                Packets::new_with_recycler_data(recycler, "handle_ping_messages", packets);
            Some(packets)
        }
    }

    fn handle_batch_pong_messages<I>(&self, pongs: I, now: Instant)
    where
        I: IntoIterator<Item = (SocketAddr, Pong)>,
    {
        let _st = ScopedTimer::from(&self.stats.handle_batch_pong_messages_time);
        let mut pongs = pongs.into_iter().peekable();
        if pongs.peek().is_some() {
            let mut ping_cache = self.ping_cache.write().unwrap();
            for (addr, pong) in pongs {
                ping_cache.add(&pong, addr, now);
            }
        }
    }

    fn handle_batch_push_messages(
        &self,
        messages: Vec<(Pubkey, Vec<CrdsValue>)>,
        thread_pool: &ThreadPool,
        recycler: &PacketsRecycler,
        stakes: &HashMap<Pubkey, u64>,
        response_sender: &PacketSender,
    ) {
        let _st = ScopedTimer::from(&self.stats.handle_batch_push_messages_time);
        if messages.is_empty() {
            return;
        }
        self.stats
            .push_message_count
            .add_relaxed(messages.len() as u64);
        // Obtain shred versions of the origins.
        let shred_versions: Vec<_> = {
            let gossip = self.gossip.read().unwrap();
            messages
                .iter()
                .map(|(from, _)| match gossip.crds.get_contact_info(*from) {
                    None => 0,
                    Some(info) => info.shred_version,
                })
                .collect()
        };
        // Filter out data if the origin has different shred version.
        let self_shred_version = self.my_shred_version();
        let num_crds_values: u64 = messages.iter().map(|(_, data)| data.len() as u64).sum();
        let messages: Vec<_> = messages
            .into_iter()
            .zip(shred_versions)
            .filter_map(|((from, mut crds_values), shred_version)| {
                Self::filter_by_shred_version(
                    &from,
                    &mut crds_values,
                    shred_version,
                    self_shred_version,
                );
                if crds_values.is_empty() {
                    None
                } else {
                    Some((from, crds_values))
                }
            })
            .collect();
        let num_filtered_crds_values = messages.iter().map(|(_, data)| data.len() as u64).sum();
        self.stats
            .push_message_value_count
            .add_relaxed(num_filtered_crds_values);
        self.stats
            .skip_push_message_shred_version
            .add_relaxed(num_crds_values - num_filtered_crds_values);
        // Update crds values and obtain updated keys.
        let updated_labels: Vec<_> = {
            let mut gossip =
                self.time_gossip_write_lock("process_push", &self.stats.process_push_message);
            let now = timestamp();
            messages
                .into_iter()
                .flat_map(|(from, crds_values)| {
                    gossip.process_push_message(&from, crds_values, now)
                })
                .map(|v| v.value.label())
                .collect()
        };
        // Generate prune messages.
        let prunes = self
            .time_gossip_write_lock("prune_received_cache", &self.stats.prune_received_cache)
            .prune_received_cache(updated_labels, stakes);
        let prunes: Vec<(Pubkey /*from*/, Vec<Pubkey> /*origins*/)> = prunes
            .into_iter()
            .flat_map(|(from, prunes)| {
                std::iter::repeat(from).zip(
                    prunes
                        .into_iter()
                        .chunks(MAX_PRUNE_DATA_NODES)
                        .into_iter()
                        .map(Iterator::collect)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        let prune_messages: Vec<_> = {
            let gossip = self.gossip.read().unwrap();
            let wallclock = timestamp();
            let self_pubkey = self.id();
            thread_pool.install(|| {
                prunes
                    .into_par_iter()
                    .with_min_len(256)
                    .filter_map(|(from, prunes)| {
                        let peer = gossip.crds.get_contact_info(from)?;
                        let mut prune_data = PruneData {
                            pubkey: self_pubkey,
                            prunes,
                            signature: Signature::default(),
                            destination: from,
                            wallclock,
                        };
                        prune_data.sign(&self.keypair);
                        let prune_message = Protocol::PruneMessage(self_pubkey, prune_data);
                        Some((peer.gossip, prune_message))
                    })
                    .collect()
            })
        };
        if prune_messages.is_empty() {
            return;
        }
        let mut packets = to_packets_with_destination(recycler.clone(), &prune_messages);
        self.stats
            .push_response_count
            .add_relaxed(packets.packets.len() as u64);
        let new_push_requests = self.new_push_requests();
        inc_new_counter_debug!("cluster_info-push_message-pushes", new_push_requests.len());
        for (address, request) in new_push_requests {
            if ContactInfo::is_valid_address(&address) {
                match Packet::from_data(&address, &request) {
                    Ok(packet) => packets.packets.push(packet),
                    Err(err) => error!("failed to write push-request packet: {:?}", err),
                }
            } else {
                trace!("Dropping Gossip push response, as destination is unknown");
            }
        }
        let _ = response_sender.send(packets);
    }

    fn get_stakes_and_epoch_time(
        bank_forks: Option<&Arc<RwLock<BankForks>>>,
    ) -> (HashMap<Pubkey, u64>, u64) {
        let epoch_time_ms;
        let stakes: HashMap<_, _> = match bank_forks {
            Some(ref bank_forks) => {
                let bank = bank_forks.read().unwrap().working_bank();
                let epoch = bank.epoch();
                let epoch_schedule = bank.epoch_schedule();
                epoch_time_ms = epoch_schedule.get_slots_in_epoch(epoch) * DEFAULT_MS_PER_SLOT;
                bank.staked_nodes()
            }
            None => {
                inc_new_counter_info!("cluster_info-purge-no_working_bank", 1);
                epoch_time_ms = CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
                HashMap::new()
            }
        };

        (stakes, epoch_time_ms)
    }

    fn process_packets(
        &self,
        packets: VecDeque<Packet>,
        thread_pool: &ThreadPool,
        recycler: &PacketsRecycler,
        response_sender: &PacketSender,
        stakes: HashMap<Pubkey, u64>,
        feature_set: Option<&FeatureSet>,
        epoch_time_ms: u64,
    ) -> Result<()> {
        let _st = ScopedTimer::from(&self.stats.process_gossip_packets_time);
        let packets: Vec<_> = thread_pool.install(|| {
            packets
                .into_par_iter()
                .filter_map(|packet| {
                    let protocol: Protocol =
                        limited_deserialize(&packet.data[..packet.meta.size]).ok()?;
                    protocol.sanitize().ok()?;
                    let protocol = protocol.par_verify()?;
                    Some((packet.meta.addr(), protocol))
                })
                .collect()
        });
        // Check if there is a duplicate instance of
        // this node with more recent timestamp.
        let check_duplicate_instance = |values: &[CrdsValue]| {
            for value in values {
                if self.instance.check_duplicate(value) {
                    return Err(Error::DuplicateNodeInstance);
                }
            }
            Ok(())
        };
        // Split packets based on their types.
        let mut pull_requests = vec![];
        let mut pull_responses = vec![];
        let mut push_messages = vec![];
        let mut prune_messages = vec![];
        let mut ping_messages = vec![];
        let mut pong_messages = vec![];
        for (from_addr, packet) in packets {
            match packet {
                Protocol::PullRequest(filter, caller) => {
                    pull_requests.push((from_addr, filter, caller))
                }
                Protocol::PullResponse(from, data) => {
                    check_duplicate_instance(&data)?;
                    pull_responses.push((from, data));
                }
                Protocol::PushMessage(from, data) => {
                    check_duplicate_instance(&data)?;
                    push_messages.push((from, data));
                }
                Protocol::PruneMessage(from, data) => prune_messages.push((from, data)),
                Protocol::PingMessage(ping) => ping_messages.push((from_addr, ping)),
                Protocol::PongMessage(pong) => pong_messages.push((from_addr, pong)),
            }
        }
        self.handle_batch_ping_messages(ping_messages, recycler, response_sender);
        self.handle_batch_prune_messages(prune_messages);
        self.handle_batch_push_messages(
            push_messages,
            thread_pool,
            recycler,
            &stakes,
            response_sender,
        );
        self.handle_batch_pull_responses(pull_responses, thread_pool, &stakes, epoch_time_ms);
        self.handle_batch_pong_messages(pong_messages, Instant::now());
        self.handle_batch_pull_requests(
            pull_requests,
            thread_pool,
            recycler,
            &stakes,
            response_sender,
            feature_set,
        );
        Ok(())
    }

    /// Process messages from the network
    fn run_listen(
        &self,
        recycler: &PacketsRecycler,
        bank_forks: Option<&Arc<RwLock<BankForks>>>,
        requests_receiver: &PacketReceiver,
        response_sender: &PacketSender,
        thread_pool: &ThreadPool,
        last_print: &mut Instant,
    ) -> Result<()> {
        const RECV_TIMEOUT: Duration = Duration::from_secs(1);
        let packets: Vec<_> = requests_receiver.recv_timeout(RECV_TIMEOUT)?.packets.into();
        let mut packets = VecDeque::from(packets);
        while let Ok(packet) = requests_receiver.try_recv() {
            packets.extend(packet.packets.into_iter());
            let excess_count = packets.len().saturating_sub(MAX_GOSSIP_TRAFFIC);
            if excess_count > 0 {
                packets.drain(0..excess_count);
                self.stats
                    .gossip_packets_dropped_count
                    .add_relaxed(excess_count as u64);
            }
        }
        let (stakes, epoch_time_ms) = Self::get_stakes_and_epoch_time(bank_forks);
        // Using root_bank instead of working_bank here so that an enbaled
        // feature does not roll back (if the feature happens to get enabled in
        // a minority fork).
        let feature_set = bank_forks.map(|bank_forks| {
            bank_forks
                .read()
                .unwrap()
                .root_bank()
                .deref()
                .feature_set
                .clone()
        });
        self.process_packets(
            packets,
            thread_pool,
            recycler,
            response_sender,
            stakes,
            feature_set.as_deref(),
            epoch_time_ms,
        )?;

        self.print_reset_stats(last_print);

        Ok(())
    }

    fn print_reset_stats(&self, last_print: &mut Instant) {
        if last_print.elapsed().as_millis() > 2000 {
            let (table_size, purged_values_size, failed_inserts_size) = {
                let r_gossip = self.gossip.read().unwrap();
                (
                    r_gossip.crds.len(),
                    r_gossip.pull.purged_values.len(),
                    r_gossip.pull.failed_inserts.len(),
                )
            };
            datapoint_info!(
                "cluster_info_stats",
                ("entrypoint", self.stats.entrypoint.clear(), i64),
                ("entrypoint2", self.stats.entrypoint2.clear(), i64),
                ("push_vote_read", self.stats.push_vote_read.clear(), i64),
                (
                    "vote_process_push",
                    self.stats.vote_process_push.clear(),
                    i64
                ),
                ("get_votes", self.stats.get_votes.clear(), i64),
                (
                    "get_accounts_hash",
                    self.stats.get_accounts_hash.clear(),
                    i64
                ),
                ("all_tvu_peers", self.stats.all_tvu_peers.clear(), i64),
                ("tvu_peers", self.stats.tvu_peers.clear(), i64),
                (
                    "new_push_requests_num",
                    self.stats.new_push_requests_num.clear(),
                    i64
                ),
                ("table_size", table_size as i64, i64),
                ("purged_values_size", purged_values_size as i64, i64),
                ("failed_inserts_size", failed_inserts_size as i64, i64),
            );
            datapoint_info!(
                "cluster_info_stats2",
                (
                    "gossip_packets_dropped_count",
                    self.stats.gossip_packets_dropped_count.clear(),
                    i64
                ),
                ("retransmit_peers", self.stats.retransmit_peers.clear(), i64),
                ("repair_peers", self.stats.repair_peers.clear(), i64),
                (
                    "new_push_requests",
                    self.stats.new_push_requests.clear(),
                    i64
                ),
                (
                    "new_push_requests2",
                    self.stats.new_push_requests2.clear(),
                    i64
                ),
                ("purge", self.stats.purge.clear(), i64),
                (
                    "process_gossip_packets_time",
                    self.stats.process_gossip_packets_time.clear(),
                    i64
                ),
                (
                    "handle_batch_ping_messages_time",
                    self.stats.handle_batch_ping_messages_time.clear(),
                    i64
                ),
                (
                    "handle_batch_pong_messages_time",
                    self.stats.handle_batch_pong_messages_time.clear(),
                    i64
                ),
                (
                    "handle_batch_prune_messages_time",
                    self.stats.handle_batch_prune_messages_time.clear(),
                    i64
                ),
                (
                    "handle_batch_pull_requests_time",
                    self.stats.handle_batch_pull_requests_time.clear(),
                    i64
                ),
                (
                    "handle_batch_pull_responses_time",
                    self.stats.handle_batch_pull_responses_time.clear(),
                    i64
                ),
                (
                    "handle_batch_push_messages_time",
                    self.stats.handle_batch_push_messages_time.clear(),
                    i64
                ),
                (
                    "process_pull_resp",
                    self.stats.process_pull_response.clear(),
                    i64
                ),
                (
                    "filter_pull_resp",
                    self.stats.filter_pull_response.clear(),
                    i64
                ),
                (
                    "process_pull_resp_count",
                    self.stats.process_pull_response_count.clear(),
                    i64
                ),
                (
                    "pull_response_fail_insert",
                    self.stats.process_pull_response_fail_insert.clear(),
                    i64
                ),
                (
                    "pull_response_fail_timeout",
                    self.stats.process_pull_response_fail_timeout.clear(),
                    i64
                ),
                (
                    "pull_response_success",
                    self.stats.process_pull_response_success.clear(),
                    i64
                ),
                (
                    "process_pull_resp_timeout",
                    self.stats.process_pull_response_timeout.clear(),
                    i64
                ),
                (
                    "push_response_count",
                    self.stats.push_response_count.clear(),
                    i64
                ),
            );
            datapoint_info!(
                "cluster_info_stats3",
                (
                    "process_pull_resp_len",
                    self.stats.process_pull_response_len.clear(),
                    i64
                ),
                (
                    "process_pull_requests",
                    self.stats.process_pull_requests.clear(),
                    i64
                ),
                (
                    "pull_request_ping_pong_check_failed_count",
                    self.stats.pull_request_ping_pong_check_failed_count.clear(),
                    i64
                ),
                (
                    "generate_pull_responses",
                    self.stats.generate_pull_responses.clear(),
                    i64
                ),
                ("process_prune", self.stats.process_prune.clear(), i64),
                (
                    "process_push_message",
                    self.stats.process_push_message.clear(),
                    i64
                ),
                (
                    "prune_received_cache",
                    self.stats.prune_received_cache.clear(),
                    i64
                ),
                (
                    "epoch_slots_lookup",
                    self.stats.epoch_slots_lookup.clear(),
                    i64
                ),
                ("epoch_slots_push", self.stats.epoch_slots_push.clear(), i64),
                ("push_message", self.stats.push_message.clear(), i64),
                (
                    "new_pull_requests",
                    self.stats.new_pull_requests.clear(),
                    i64
                ),
                (
                    "mark_pull_request",
                    self.stats.mark_pull_request.clear(),
                    i64
                ),
            );
            datapoint_info!(
                "cluster_info_stats4",
                (
                    "skip_push_message_shred_version",
                    self.stats.skip_push_message_shred_version.clear(),
                    i64
                ),
                (
                    "skip_pull_response_shred_version",
                    self.stats.skip_pull_response_shred_version.clear(),
                    i64
                ),
                (
                    "skip_pull_shred_version",
                    self.stats.skip_pull_shred_version.clear(),
                    i64
                ),
                (
                    "push_message_count",
                    self.stats.push_message_count.clear(),
                    i64
                ),
                (
                    "push_message_value_count",
                    self.stats.push_message_value_count.clear(),
                    i64
                ),
                (
                    "new_pull_requests_count",
                    self.stats.new_pull_requests_count.clear(),
                    i64
                ),
                (
                    "prune_message_count",
                    self.stats.prune_message_count.clear(),
                    i64
                ),
                (
                    "prune_message_len",
                    self.stats.prune_message_len.clear(),
                    i64
                ),
            );
            datapoint_info!(
                "cluster_info_stats5",
                (
                    "pull_requests_count",
                    self.stats.pull_requests_count.clear(),
                    i64
                ),
            );

            *last_print = Instant::now();
        }
    }

    pub fn listen(
        self: Arc<Self>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        requests_receiver: PacketReceiver,
        response_sender: PacketSender,
        exit: &Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        let recycler = PacketsRecycler::default();
        Builder::new()
            .name("solana-listen".to_string())
            .spawn(move || {
                let thread_pool = ThreadPoolBuilder::new()
                    .num_threads(std::cmp::min(get_thread_count(), 8))
                    .thread_name(|i| format!("sol-gossip-work-{}", i))
                    .build()
                    .unwrap();
                let mut last_print = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    if let Err(err) = self.run_listen(
                        &recycler,
                        bank_forks.as_ref(),
                        &requests_receiver,
                        &response_sender,
                        &thread_pool,
                        &mut last_print,
                    ) {
                        match err {
                            Error::RecvTimeoutError(_) => {
                                let table_size = self.gossip.read().unwrap().crds.len();
                                debug!(
                                    "{}: run_listen timeout, table size: {}",
                                    self.id(),
                                    table_size,
                                );
                            }
                            Error::DuplicateNodeInstance => {
                                error!(
                                    "duplicate running instances of the same validator node: {}",
                                    self.id()
                                );
                                exit.store(true, Ordering::Relaxed);
                                // TODO: Pass through ValidatorExit here so
                                // that this will exit cleanly.
                                std::process::exit(1);
                            }
                            _ => error!("gossip run_listen failed: {}", err),
                        }
                    }
                    thread_mem_usage::datapoint("solana-listen");
                }
            })
            .unwrap()
    }

    pub fn gossip_contact_info(id: &Pubkey, gossip: SocketAddr, shred_version: u16) -> ContactInfo {
        ContactInfo {
            id: *id,
            gossip,
            wallclock: timestamp(),
            shred_version,
            ..ContactInfo::default()
        }
    }

    /// An alternative to Spy Node that has a valid gossip address and fully participate in Gossip.
    pub fn gossip_node(
        id: &Pubkey,
        gossip_addr: &SocketAddr,
        shred_version: u16,
    ) -> (ContactInfo, UdpSocket, Option<TcpListener>) {
        let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (port, (gossip_socket, ip_echo)) =
            Node::get_gossip_port(gossip_addr, VALIDATOR_PORT_RANGE, bind_ip_addr);
        let contact_info =
            Self::gossip_contact_info(id, SocketAddr::new(gossip_addr.ip(), port), shred_version);

        (contact_info, gossip_socket, Some(ip_echo))
    }

    /// A Node with dummy ports to spy on gossip via pull requests
    pub fn spy_node(
        id: &Pubkey,
        shred_version: u16,
    ) -> (ContactInfo, UdpSocket, Option<TcpListener>) {
        let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_, gossip_socket) = bind_in_range(bind_ip_addr, VALIDATOR_PORT_RANGE).unwrap();
        let contact_info = Self::gossip_contact_info(id, socketaddr_any!(), shred_version);

        (contact_info, gossip_socket, None)
    }
}

/// Turbine logic
/// 1 - For the current node find out if it is in layer 1
/// 1.1 - If yes, then broadcast to all layer 1 nodes
///      1 - using the layer 1 index, broadcast to all layer 2 nodes assuming you know neighborhood size
/// 1.2 - If no, then figure out what layer the node is in and who the neighbors are and only broadcast to them
///      1 - also check if there are nodes in the next layer and repeat the layer 1 to layer 2 logic

/// Returns Neighbor Nodes and Children Nodes `(neighbors, children)` for a given node based on its stake
pub fn compute_retransmit_peers(
    fanout: usize,
    my_index: usize,
    stakes_and_index: Vec<usize>,
) -> (Vec<usize>, Vec<usize>) {
    //calc num_layers and num_neighborhoods using the total number of nodes
    let (num_layers, layer_indices) =
        ClusterInfo::describe_data_plane(stakes_and_index.len(), fanout);

    if num_layers <= 1 {
        /* single layer data plane */
        (stakes_and_index, vec![])
    } else {
        //find my layer
        let locality = ClusterInfo::localize(&layer_indices, fanout, my_index);
        let upper_bound = cmp::min(locality.neighbor_bounds.1, stakes_and_index.len());
        let neighbors = stakes_and_index[locality.neighbor_bounds.0..upper_bound].to_vec();
        let mut children = Vec::new();
        for ix in locality.next_layer_peers {
            if let Some(peer) = stakes_and_index.get(ix) {
                children.push(*peer);
                continue;
            }
            break;
        }
        (neighbors, children)
    }
}

#[derive(Debug)]
pub struct Sockets {
    pub gossip: UdpSocket,
    pub ip_echo: Option<TcpListener>,
    pub tvu: Vec<UdpSocket>,
    pub tvu_forwards: Vec<UdpSocket>,
    pub tpu: Vec<UdpSocket>,
    pub tpu_forwards: Vec<UdpSocket>,
    pub broadcast: Vec<UdpSocket>,
    pub repair: UdpSocket,
    pub retransmit_sockets: Vec<UdpSocket>,
    pub serve_repair: UdpSocket,
}

#[derive(Debug)]
pub struct Node {
    pub info: ContactInfo,
    pub sockets: Sockets,
}

impl Node {
    pub fn new_localhost() -> Self {
        let pubkey = solana_sdk::pubkey::new_rand();
        Self::new_localhost_with_pubkey(&pubkey)
    }
    pub fn new_localhost_with_pubkey(pubkey: &Pubkey) -> Self {
        let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let tpu = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (gossip_port, (gossip, ip_echo)) =
            bind_common_in_range(bind_ip_addr, (1024, 65535)).unwrap();
        let gossip_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), gossip_port);
        let tvu = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tvu_forwards = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tpu_forwards = UdpSocket::bind("127.0.0.1:0").unwrap();
        let repair = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rpc_port = find_available_port_in_range(bind_ip_addr, (1024, 65535)).unwrap();
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), rpc_port);
        let rpc_pubsub_port = find_available_port_in_range(bind_ip_addr, (1024, 65535)).unwrap();
        let rpc_pubsub_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), rpc_pubsub_port);

        let broadcast = vec![UdpSocket::bind("0.0.0.0:0").unwrap()];
        let retransmit_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve_repair = UdpSocket::bind("127.0.0.1:0").unwrap();
        let unused = UdpSocket::bind("0.0.0.0:0").unwrap();
        let info = ContactInfo {
            id: *pubkey,
            gossip: gossip_addr,
            tvu: tvu.local_addr().unwrap(),
            tvu_forwards: tvu_forwards.local_addr().unwrap(),
            repair: repair.local_addr().unwrap(),
            tpu: tpu.local_addr().unwrap(),
            tpu_forwards: tpu_forwards.local_addr().unwrap(),
            unused: unused.local_addr().unwrap(),
            rpc: rpc_addr,
            rpc_pubsub: rpc_pubsub_addr,
            serve_repair: serve_repair.local_addr().unwrap(),
            wallclock: timestamp(),
            shred_version: 0,
        };
        Node {
            info,
            sockets: Sockets {
                gossip,
                ip_echo: Some(ip_echo),
                tvu: vec![tvu],
                tvu_forwards: vec![tvu_forwards],
                tpu: vec![tpu],
                tpu_forwards: vec![tpu_forwards],
                broadcast,
                repair,
                retransmit_sockets: vec![retransmit_socket],
                serve_repair,
            },
        }
    }
    fn get_gossip_port(
        gossip_addr: &SocketAddr,
        port_range: PortRange,
        bind_ip_addr: IpAddr,
    ) -> (u16, (UdpSocket, TcpListener)) {
        if gossip_addr.port() != 0 {
            (
                gossip_addr.port(),
                bind_common(bind_ip_addr, gossip_addr.port(), false).unwrap_or_else(|e| {
                    panic!("gossip_addr bind_to port {}: {}", gossip_addr.port(), e)
                }),
            )
        } else {
            bind_common_in_range(bind_ip_addr, port_range).expect("Failed to bind")
        }
    }
    fn bind(bind_ip_addr: IpAddr, port_range: PortRange) -> (u16, UdpSocket) {
        bind_in_range(bind_ip_addr, port_range).expect("Failed to bind")
    }

    pub fn new_with_external_ip(
        pubkey: &Pubkey,
        gossip_addr: &SocketAddr,
        port_range: PortRange,
        bind_ip_addr: IpAddr,
    ) -> Node {
        let (gossip_port, (gossip, ip_echo)) =
            Self::get_gossip_port(gossip_addr, port_range, bind_ip_addr);

        let (tvu_port, tvu_sockets) =
            multi_bind_in_range(bind_ip_addr, port_range, 8).expect("tvu multi_bind");

        let (tvu_forwards_port, tvu_forwards_sockets) =
            multi_bind_in_range(bind_ip_addr, port_range, 8).expect("tvu_forwards multi_bind");

        let (tpu_port, tpu_sockets) =
            multi_bind_in_range(bind_ip_addr, port_range, 32).expect("tpu multi_bind");

        let (tpu_forwards_port, tpu_forwards_sockets) =
            multi_bind_in_range(bind_ip_addr, port_range, 8).expect("tpu_forwards multi_bind");

        let (_, retransmit_sockets) =
            multi_bind_in_range(bind_ip_addr, port_range, 8).expect("retransmit multi_bind");

        let (repair_port, repair) = Self::bind(bind_ip_addr, port_range);
        let (serve_repair_port, serve_repair) = Self::bind(bind_ip_addr, port_range);

        let (_, broadcast) =
            multi_bind_in_range(bind_ip_addr, port_range, 4).expect("broadcast multi_bind");

        let info = ContactInfo {
            id: *pubkey,
            gossip: SocketAddr::new(gossip_addr.ip(), gossip_port),
            tvu: SocketAddr::new(gossip_addr.ip(), tvu_port),
            tvu_forwards: SocketAddr::new(gossip_addr.ip(), tvu_forwards_port),
            repair: SocketAddr::new(gossip_addr.ip(), repair_port),
            tpu: SocketAddr::new(gossip_addr.ip(), tpu_port),
            tpu_forwards: SocketAddr::new(gossip_addr.ip(), tpu_forwards_port),
            unused: socketaddr_any!(),
            rpc: socketaddr_any!(),
            rpc_pubsub: socketaddr_any!(),
            serve_repair: SocketAddr::new(gossip_addr.ip(), serve_repair_port),
            wallclock: 0,
            shred_version: 0,
        };
        trace!("new ContactInfo: {:?}", info);

        Node {
            info,
            sockets: Sockets {
                gossip,
                tvu: tvu_sockets,
                tvu_forwards: tvu_forwards_sockets,
                tpu: tpu_sockets,
                tpu_forwards: tpu_forwards_sockets,
                broadcast,
                repair,
                retransmit_sockets,
                serve_repair,
                ip_echo: Some(ip_echo),
            },
        }
    }
}

pub fn stake_weight_peers<S: std::hash::BuildHasher>(
    peers: &mut Vec<ContactInfo>,
    stakes: Option<Arc<HashMap<Pubkey, u64, S>>>,
) -> Vec<(u64, usize)> {
    peers.dedup();
    ClusterInfo::sorted_stakes_with_index(peers, stakes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crds_value::{CrdsValue, CrdsValueLabel, Vote as CrdsVote};
    use itertools::izip;
    use solana_perf::test_tx::test_tx;
    use solana_sdk::signature::{Keypair, Signer};
    use solana_vote_program::{vote_instruction, vote_state::Vote};
    use std::collections::HashSet;
    use std::iter::repeat_with;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4};
    use std::sync::Arc;

    #[test]
    fn test_gossip_node() {
        //check that a gossip nodes always show up as spies
        let (node, _, _) = ClusterInfo::spy_node(&solana_sdk::pubkey::new_rand(), 0);
        assert!(ClusterInfo::is_spy_node(&node));
        let (node, _, _) = ClusterInfo::gossip_node(
            &solana_sdk::pubkey::new_rand(),
            &"1.1.1.1:1111".parse().unwrap(),
            0,
        );
        assert!(ClusterInfo::is_spy_node(&node));
    }

    #[test]
    fn test_handle_pull() {
        solana_logger::setup();
        let node = Node::new_localhost();
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(node.info));

        let entrypoint_pubkey = solana_sdk::pubkey::new_rand();
        let data = test_crds_values(entrypoint_pubkey);
        let timeouts = HashMap::new();
        assert_eq!(
            (0, 0, 1),
            ClusterInfo::handle_pull_response(
                &cluster_info,
                &entrypoint_pubkey,
                data.clone(),
                &timeouts
            )
        );

        let entrypoint_pubkey2 = solana_sdk::pubkey::new_rand();
        assert_eq!(
            (1, 0, 0),
            ClusterInfo::handle_pull_response(&cluster_info, &entrypoint_pubkey2, data, &timeouts)
        );
    }

    fn new_rand_socket_addr<R: Rng>(rng: &mut R) -> SocketAddr {
        let addr = if rng.gen_bool(0.5) {
            IpAddr::V4(Ipv4Addr::new(rng.gen(), rng.gen(), rng.gen(), rng.gen()))
        } else {
            IpAddr::V6(Ipv6Addr::new(
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
            ))
        };
        SocketAddr::new(addr, /*port=*/ rng.gen())
    }

    fn new_rand_remote_node<R>(rng: &mut R) -> (Keypair, SocketAddr)
    where
        R: Rng,
    {
        let keypair = Keypair::new();
        let socket = new_rand_socket_addr(rng);
        (keypair, socket)
    }

    #[test]
    fn test_handle_pong_messages() {
        let now = Instant::now();
        let mut rng = rand::thread_rng();
        let this_node = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&this_node.pubkey(), timestamp()),
            this_node.clone(),
        );
        let remote_nodes: Vec<(Keypair, SocketAddr)> =
            repeat_with(|| new_rand_remote_node(&mut rng))
                .take(128)
                .collect();
        let pings: Vec<_> = {
            let mut ping_cache = cluster_info.ping_cache.write().unwrap();
            let mut pingf = || Ping::new_rand(&mut rng, &this_node).ok();
            remote_nodes
                .iter()
                .map(|(keypair, socket)| {
                    let node = (keypair.pubkey(), *socket);
                    let (check, ping) = ping_cache.check(now, node, &mut pingf);
                    // Assert that initially remote nodes will not pass the
                    // ping/pong check.
                    assert!(!check);
                    ping.unwrap()
                })
                .collect()
        };
        let pongs: Vec<(SocketAddr, Pong)> = pings
            .iter()
            .zip(&remote_nodes)
            .map(|(ping, (keypair, socket))| (*socket, Pong::new(ping, keypair).unwrap()))
            .collect();
        let now = now + Duration::from_millis(1);
        cluster_info.handle_batch_pong_messages(pongs, now);
        // Assert that remote nodes now pass the ping/pong check.
        {
            let mut ping_cache = cluster_info.ping_cache.write().unwrap();
            for (keypair, socket) in &remote_nodes {
                let node = (keypair.pubkey(), *socket);
                let (check, _) = ping_cache.check(now, node, || -> Option<Ping> { None });
                assert!(check);
            }
        }
        // Assert that a new random remote node still will not pass the check.
        {
            let mut ping_cache = cluster_info.ping_cache.write().unwrap();
            let (keypair, socket) = new_rand_remote_node(&mut rng);
            let node = (keypair.pubkey(), socket);
            let (check, _) = ping_cache.check(now, node, || -> Option<Ping> { None });
            assert!(!check);
        }
    }

    #[test]
    fn test_handle_ping_messages() {
        let mut rng = rand::thread_rng();
        let this_node = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&this_node.pubkey(), timestamp()),
            this_node.clone(),
        );
        let remote_nodes: Vec<(Keypair, SocketAddr)> =
            repeat_with(|| new_rand_remote_node(&mut rng))
                .take(128)
                .collect();
        let pings: Vec<_> = remote_nodes
            .iter()
            .map(|(keypair, _)| Ping::new_rand(&mut rng, keypair).unwrap())
            .collect();
        let pongs: Vec<_> = pings
            .iter()
            .map(|ping| Pong::new(ping, &this_node).unwrap())
            .collect();
        let recycler = PacketsRecycler::default();
        let packets = cluster_info
            .handle_ping_messages(
                remote_nodes
                    .iter()
                    .map(|(_, socket)| *socket)
                    .zip(pings.into_iter()),
                &recycler,
            )
            .unwrap()
            .packets;
        assert_eq!(remote_nodes.len(), packets.len());
        for (packet, (_, socket), pong) in izip!(
            packets.into_iter(),
            remote_nodes.into_iter(),
            pongs.into_iter()
        ) {
            assert_eq!(packet.meta.addr(), socket);
            let bytes = serialize(&pong).unwrap();
            match limited_deserialize(&packet.data[..packet.meta.size]).unwrap() {
                Protocol::PongMessage(pong) => assert_eq!(serialize(&pong).unwrap(), bytes),
                _ => panic!("invalid packet!"),
            }
        }
    }

    fn test_crds_values(pubkey: Pubkey) -> Vec<CrdsValue> {
        let entrypoint = ContactInfo::new_localhost(&pubkey, timestamp());
        let entrypoint_crdsvalue = CrdsValue::new_unsigned(CrdsData::ContactInfo(entrypoint));
        vec![entrypoint_crdsvalue]
    }

    #[test]
    fn test_filter_shred_version() {
        let from = solana_sdk::pubkey::new_rand();
        let my_shred_version = 1;
        let other_shred_version = 1;

        // Allow same shred_version
        let mut values = test_crds_values(from);
        ClusterInfo::filter_by_shred_version(
            &from,
            &mut values,
            other_shred_version,
            my_shred_version,
        );
        assert_eq!(values.len(), 1);

        // Allow shred_version=0.
        let other_shred_version = 0;
        ClusterInfo::filter_by_shred_version(
            &from,
            &mut values,
            other_shred_version,
            my_shred_version,
        );
        assert_eq!(values.len(), 1);

        // Change to sender's ContactInfo version, allow that.
        let other_shred_version = 2;
        ClusterInfo::filter_by_shred_version(
            &from,
            &mut values,
            other_shred_version,
            my_shred_version,
        );
        assert_eq!(values.len(), 1);

        let snapshot_hash_data = CrdsValue::new_unsigned(CrdsData::SnapshotHashes(SnapshotHash {
            from: solana_sdk::pubkey::new_rand(),
            hashes: vec![],
            wallclock: 0,
        }));
        values.push(snapshot_hash_data);
        // Change to sender's ContactInfo version, allow that.
        let other_shred_version = 2;
        ClusterInfo::filter_by_shred_version(
            &from,
            &mut values,
            other_shred_version,
            my_shred_version,
        );
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn test_max_snapshot_hashes_with_push_messages() {
        let mut rng = rand::thread_rng();
        for _ in 0..256 {
            let snapshot_hash = SnapshotHash::new_rand(&mut rng, None);
            let crds_value =
                CrdsValue::new_signed(CrdsData::SnapshotHashes(snapshot_hash), &Keypair::new());
            let message = Protocol::PushMessage(Pubkey::new_unique(), vec![crds_value]);
            let socket = new_rand_socket_addr(&mut rng);
            assert!(Packet::from_data(&socket, message).is_ok());
        }
    }

    #[test]
    fn test_max_snapshot_hashes_with_pull_responses() {
        let mut rng = rand::thread_rng();
        for _ in 0..256 {
            let snapshot_hash = SnapshotHash::new_rand(&mut rng, None);
            let crds_value =
                CrdsValue::new_signed(CrdsData::AccountsHashes(snapshot_hash), &Keypair::new());
            let response = Protocol::PullResponse(Pubkey::new_unique(), vec![crds_value]);
            let socket = new_rand_socket_addr(&mut rng);
            assert!(Packet::from_data(&socket, response).is_ok());
        }
    }

    #[test]
    fn test_max_prune_data_pubkeys() {
        let mut rng = rand::thread_rng();
        for _ in 0..64 {
            let self_keypair = Keypair::new();
            let prune_data =
                PruneData::new_rand(&mut rng, &self_keypair, Some(MAX_PRUNE_DATA_NODES));
            let prune_message = Protocol::PruneMessage(self_keypair.pubkey(), prune_data);
            let socket = new_rand_socket_addr(&mut rng);
            assert!(Packet::from_data(&socket, prune_message).is_ok());
        }
        // Assert that MAX_PRUNE_DATA_NODES is highest possible.
        let self_keypair = Keypair::new();
        let prune_data =
            PruneData::new_rand(&mut rng, &self_keypair, Some(MAX_PRUNE_DATA_NODES + 1));
        let prune_message = Protocol::PruneMessage(self_keypair.pubkey(), prune_data);
        let socket = new_rand_socket_addr(&mut rng);
        assert!(Packet::from_data(&socket, prune_message).is_err());
    }

    #[test]
    fn test_push_message_max_payload_size() {
        let header = Protocol::PushMessage(Pubkey::default(), Vec::default());
        assert_eq!(
            PUSH_MESSAGE_MAX_PAYLOAD_SIZE,
            PACKET_DATA_SIZE - serialized_size(&header).unwrap() as usize
        );
    }

    #[test]
    fn test_pull_response_min_serialized_size() {
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let crds_values = vec![CrdsValue::new_rand(&mut rng, None)];
            let pull_response = Protocol::PullResponse(Pubkey::new_unique(), crds_values);
            let size = serialized_size(&pull_response).unwrap();
            assert!(PULL_RESPONSE_MIN_SERIALIZED_SIZE as u64 <= size);
        }
    }

    #[test]
    fn test_cluster_spy_gossip() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        //check that gossip doesn't try to push to invalid addresses
        let node = Node::new_localhost();
        let (spy, _, _) = ClusterInfo::spy_node(&solana_sdk::pubkey::new_rand(), 0);
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(node.info));
        cluster_info.insert_info(spy);
        cluster_info
            .gossip
            .write()
            .unwrap()
            .refresh_push_active_set(&HashMap::new(), None);
        let reqs =
            cluster_info.generate_new_gossip_requests(&thread_pool, None, &HashMap::new(), true);
        //assert none of the addrs are invalid.
        reqs.iter().all(|(addr, _)| {
            let res = ContactInfo::is_valid_address(addr);
            assert!(res);
            res
        });
    }

    #[test]
    fn test_cluster_info_new() {
        let d = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(d.clone());
        assert_eq!(d.id, cluster_info.id());
    }

    #[test]
    fn insert_info_test() {
        let d = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(d);
        let d = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let label = CrdsValueLabel::ContactInfo(d.id);
        cluster_info.insert_info(d);
        assert!(cluster_info
            .gossip
            .read()
            .unwrap()
            .crds
            .lookup(&label)
            .is_some());
    }
    #[test]
    #[should_panic]
    fn test_update_contact_info() {
        let d = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(d);
        let entry_label = CrdsValueLabel::ContactInfo(cluster_info.id());
        assert!(cluster_info
            .gossip
            .read()
            .unwrap()
            .crds
            .lookup(&entry_label)
            .is_some());

        let now = timestamp();
        cluster_info.update_contact_info(|ci| ci.wallclock = now);
        assert_eq!(
            cluster_info
                .gossip
                .read()
                .unwrap()
                .crds
                .lookup(&entry_label)
                .unwrap()
                .contact_info()
                .unwrap()
                .wallclock,
            now
        );

        // Inserting Contactinfo with different pubkey should panic,
        // and update should fail
        cluster_info.update_contact_info(|ci| ci.id = solana_sdk::pubkey::new_rand())
    }

    fn assert_in_range(x: u16, range: (u16, u16)) {
        assert!(x >= range.0);
        assert!(x < range.1);
    }

    fn check_sockets(sockets: &[UdpSocket], ip: IpAddr, range: (u16, u16)) {
        assert!(sockets.len() > 1);
        let port = sockets[0].local_addr().unwrap().port();
        for socket in sockets.iter() {
            check_socket(socket, ip, range);
            assert_eq!(socket.local_addr().unwrap().port(), port);
        }
    }

    fn check_socket(socket: &UdpSocket, ip: IpAddr, range: (u16, u16)) {
        let local_addr = socket.local_addr().unwrap();
        assert_eq!(local_addr.ip(), ip);
        assert_in_range(local_addr.port(), range);
    }

    fn check_node_sockets(node: &Node, ip: IpAddr, range: (u16, u16)) {
        check_socket(&node.sockets.gossip, ip, range);
        check_socket(&node.sockets.repair, ip, range);

        check_sockets(&node.sockets.tvu, ip, range);
        check_sockets(&node.sockets.tpu, ip, range);
    }

    #[test]
    fn new_with_external_ip_test_random() {
        let ip = Ipv4Addr::from(0);
        let node = Node::new_with_external_ip(
            &solana_sdk::pubkey::new_rand(),
            &socketaddr!(ip, 0),
            VALIDATOR_PORT_RANGE,
            IpAddr::V4(ip),
        );

        check_node_sockets(&node, IpAddr::V4(ip), VALIDATOR_PORT_RANGE);
    }

    #[test]
    fn new_with_external_ip_test_gossip() {
        // Can't use VALIDATOR_PORT_RANGE because if this test runs in parallel with others, the
        // port returned by `bind_in_range()` might be snatched up before `Node::new_with_external_ip()` runs
        let port_range = (VALIDATOR_PORT_RANGE.1 + 10, VALIDATOR_PORT_RANGE.1 + 20);

        let ip = IpAddr::V4(Ipv4Addr::from(0));
        let port = bind_in_range(ip, port_range).expect("Failed to bind").0;
        let node = Node::new_with_external_ip(
            &solana_sdk::pubkey::new_rand(),
            &socketaddr!(0, port),
            port_range,
            ip,
        );

        check_node_sockets(&node, ip, port_range);

        assert_eq!(node.sockets.gossip.local_addr().unwrap().port(), port);
    }

    //test that all cluster_info objects only generate signed messages
    //when constructed with keypairs
    #[test]
    fn test_gossip_signature_verification() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        //create new cluster info, leader, and peer
        let keypair = Keypair::new();
        let peer_keypair = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let peer = ContactInfo::new_localhost(&peer_keypair.pubkey(), 0);
        let cluster_info = ClusterInfo::new(contact_info, Arc::new(keypair));
        cluster_info.insert_info(peer);
        cluster_info
            .gossip
            .write()
            .unwrap()
            .refresh_push_active_set(&HashMap::new(), None);
        //check that all types of gossip messages are signed correctly
        let (_, push_messages) = cluster_info
            .gossip
            .write()
            .unwrap()
            .new_push_messages(cluster_info.drain_push_queue(), timestamp());
        // there should be some pushes ready
        assert_eq!(push_messages.is_empty(), false);
        push_messages
            .values()
            .for_each(|v| v.par_iter().for_each(|v| assert!(v.verify())));

        let (_, _, val) = cluster_info
            .gossip
            .write()
            .unwrap()
            .new_pull_request(
                &thread_pool,
                timestamp(),
                None,
                &HashMap::new(),
                MAX_BLOOM_SIZE,
            )
            .ok()
            .unwrap();
        assert!(val.verify());
    }

    fn num_layers(nodes: usize, fanout: usize) -> usize {
        ClusterInfo::describe_data_plane(nodes, fanout).0
    }

    #[test]
    fn test_describe_data_plane() {
        // no nodes
        assert_eq!(num_layers(0, 200), 0);

        // 1 node
        assert_eq!(num_layers(1, 200), 1);

        // 10 nodes with fanout of 2
        assert_eq!(num_layers(10, 2), 3);

        // fanout + 1 nodes with fanout of 2
        assert_eq!(num_layers(3, 2), 2);

        // A little more realistic
        assert_eq!(num_layers(100, 10), 2);

        // A little more realistic with odd numbers
        assert_eq!(num_layers(103, 13), 2);

        // A little more realistic with just enough for 3 layers
        assert_eq!(num_layers(111, 10), 3);

        // larger
        let (layer_cnt, layer_indices) = ClusterInfo::describe_data_plane(10_000, 10);
        assert_eq!(layer_cnt, 4);
        // distances between index values should increase by `fanout` for every layer.
        let mut capacity = 10 * 10;
        assert_eq!(layer_indices[1], 10);
        layer_indices[1..].windows(2).for_each(|x| {
            if x.len() == 2 {
                assert_eq!(x[1] - x[0], capacity);
                capacity *= 10;
            }
        });

        // massive
        let (layer_cnt, layer_indices) = ClusterInfo::describe_data_plane(500_000, 200);
        let mut capacity = 200 * 200;
        assert_eq!(layer_cnt, 3);
        // distances between index values should increase by `fanout` for every layer.
        assert_eq!(layer_indices[1], 200);
        layer_indices[1..].windows(2).for_each(|x| {
            if x.len() == 2 {
                assert_eq!(x[1] - x[0], capacity);
                capacity *= 200;
            }
        });
        let total_capacity: usize = *layer_indices.last().unwrap();
        assert!(total_capacity >= 500_000);
    }

    #[test]
    fn test_localize() {
        // go for gold
        let (_, layer_indices) = ClusterInfo::describe_data_plane(500_000, 200);
        let mut me = 0;
        let mut layer_ix = 0;
        let locality = ClusterInfo::localize(&layer_indices, 200, me);
        assert_eq!(locality.layer_ix, layer_ix);
        assert_eq!(
            locality.next_layer_bounds,
            Some((layer_indices[layer_ix + 1], layer_indices[layer_ix + 2]))
        );
        me = 201;
        layer_ix = 1;
        let locality = ClusterInfo::localize(&layer_indices, 200, me);
        assert_eq!(
            locality.layer_ix, layer_ix,
            "layer_indices[layer_ix] is actually {}",
            layer_indices[layer_ix]
        );
        assert_eq!(
            locality.next_layer_bounds,
            Some((layer_indices[layer_ix + 1], layer_indices[layer_ix + 2]))
        );
        me = 20_000;
        layer_ix = 1;
        let locality = ClusterInfo::localize(&layer_indices, 200, me);
        assert_eq!(
            locality.layer_ix, layer_ix,
            "layer_indices[layer_ix] is actually {}",
            layer_indices[layer_ix]
        );
        assert_eq!(
            locality.next_layer_bounds,
            Some((layer_indices[layer_ix + 1], layer_indices[layer_ix + 2]))
        );

        // test no child layer since last layer should have massive capacity
        let (_, layer_indices) = ClusterInfo::describe_data_plane(500_000, 200);
        me = 40_201;
        layer_ix = 2;
        let locality = ClusterInfo::localize(&layer_indices, 200, me);
        assert_eq!(
            locality.layer_ix, layer_ix,
            "layer_indices[layer_ix] is actually {}",
            layer_indices[layer_ix]
        );
        assert_eq!(locality.next_layer_bounds, None);
    }

    #[test]
    fn test_localize_child_peer_overlap() {
        let (_, layer_indices) = ClusterInfo::describe_data_plane(500_000, 200);
        let last_ix = layer_indices.len() - 1;
        // sample every 33 pairs to reduce test time
        for x in (0..*layer_indices.get(last_ix - 2).unwrap()).step_by(33) {
            let me_locality = ClusterInfo::localize(&layer_indices, 200, x);
            let buddy_locality = ClusterInfo::localize(&layer_indices, 200, x + 1);
            assert!(!me_locality.next_layer_peers.is_empty());
            assert!(!buddy_locality.next_layer_peers.is_empty());
            me_locality
                .next_layer_peers
                .iter()
                .zip(buddy_locality.next_layer_peers.iter())
                .for_each(|(x, y)| assert_ne!(x, y));
        }
    }

    #[test]
    fn test_network_coverage() {
        // pretend to be each node in a scaled down network and make sure the set of all the broadcast peers
        // includes every node in the network.
        let (_, layer_indices) = ClusterInfo::describe_data_plane(25_000, 10);
        let mut broadcast_set = HashSet::new();
        for my_index in 0..25_000 {
            let my_locality = ClusterInfo::localize(&layer_indices, 10, my_index);
            broadcast_set.extend(my_locality.neighbor_bounds.0..my_locality.neighbor_bounds.1);
            broadcast_set.extend(my_locality.next_layer_peers);
        }

        for i in 0..25_000 {
            assert!(broadcast_set.contains(&(i as usize)));
        }
        assert!(broadcast_set.contains(&(layer_indices.last().unwrap() - 1)));
        //sanity check for past total capacity.
        assert!(!broadcast_set.contains(&(layer_indices.last().unwrap())));
    }

    #[test]
    fn test_push_vote() {
        let keys = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keys.pubkey(), 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);

        // make sure empty crds is handled correctly
        let now = timestamp();
        let (_, votes, max_ts) = cluster_info.get_votes(now);
        assert_eq!(votes, vec![]);
        assert_eq!(max_ts, now);

        // add a vote
        let tx = test_tx();
        let index = 1;
        cluster_info.push_vote(index, tx.clone());
        cluster_info.flush_push_queue();

        // -1 to make sure that the clock is strictly lower then when insert occurred
        let (labels, votes, max_ts) = cluster_info.get_votes(now - 1);
        assert_eq!(votes, vec![tx]);
        assert_eq!(labels.len(), 1);
        match labels[0] {
            CrdsValueLabel::Vote(_, pubkey) => {
                assert_eq!(pubkey, keys.pubkey());
            }

            _ => panic!("Bad match"),
        }
        assert!(max_ts >= now - 1);

        // make sure timestamp filter works
        let (_, votes, new_max_ts) = cluster_info.get_votes(max_ts);
        assert_eq!(votes, vec![]);
        assert_eq!(max_ts, new_max_ts);
    }

    #[test]
    fn test_push_epoch_slots() {
        let keys = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keys.pubkey(), 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        assert!(slots.is_empty());
        assert!(since.is_none());
        cluster_info.push_epoch_slots(&[0]);
        cluster_info.flush_push_queue();

        let (slots, since) = cluster_info.get_epoch_slots_since(Some(std::u64::MAX));
        assert!(slots.is_empty());
        assert_eq!(since, Some(std::u64::MAX));

        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        assert_eq!(slots.len(), 1);
        assert!(since.is_some());

        let (slots, since2) = cluster_info.get_epoch_slots_since(since);
        assert!(slots.is_empty());
        assert_eq!(since2, since);
    }

    #[test]
    fn test_append_entrypoint_to_pulls() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        let entrypoint_pubkey = solana_sdk::pubkey::new_rand();
        let entrypoint = ContactInfo::new_localhost(&entrypoint_pubkey, timestamp());
        cluster_info.set_entrypoint(entrypoint.clone());
        let pulls = cluster_info.new_pull_requests(&thread_pool, None, &HashMap::new());
        assert_eq!(1, pulls.len() as u64);
        match pulls.get(0) {
            Some((addr, msg)) => {
                assert_eq!(*addr, entrypoint.gossip);
                match msg {
                    Protocol::PullRequest(_, value) => {
                        assert!(value.verify());
                        assert_eq!(value.pubkey(), cluster_info.id())
                    }
                    _ => panic!("wrong protocol"),
                }
            }
            None => panic!("entrypoint should be a pull destination"),
        }

        // now add this message back to the table and make sure after the next pull, the entrypoint is unset
        let entrypoint_crdsvalue =
            CrdsValue::new_unsigned(CrdsData::ContactInfo(entrypoint.clone()));
        let cluster_info = Arc::new(cluster_info);
        let timeouts = cluster_info.gossip.read().unwrap().make_timeouts_test();
        ClusterInfo::handle_pull_response(
            &cluster_info,
            &entrypoint_pubkey,
            vec![entrypoint_crdsvalue],
            &timeouts,
        );
        let pulls = cluster_info.new_pull_requests(&thread_pool, None, &HashMap::new());
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(*cluster_info.entrypoint.read().unwrap(), Some(entrypoint));
    }

    #[test]
    fn test_split_messages_small() {
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        test_split_messages(value);
    }

    #[test]
    fn test_split_messages_large() {
        let value = CrdsValue::new_unsigned(CrdsData::LowestSlot(
            0,
            LowestSlot::new(Pubkey::default(), 0, 0),
        ));
        test_split_messages(value);
    }

    #[test]
    fn test_split_gossip_messages() {
        const NUM_CRDS_VALUES: usize = 2048;
        let mut rng = rand::thread_rng();
        let values: Vec<_> = std::iter::repeat_with(|| CrdsValue::new_rand(&mut rng, None))
            .take(NUM_CRDS_VALUES)
            .collect();
        let splits: Vec<_> =
            ClusterInfo::split_gossip_messages(PUSH_MESSAGE_MAX_PAYLOAD_SIZE, values.clone())
                .collect();
        let self_pubkey = solana_sdk::pubkey::new_rand();
        assert!(splits.len() * 3 < NUM_CRDS_VALUES);
        // Assert that all messages are included in the splits.
        assert_eq!(NUM_CRDS_VALUES, splits.iter().map(Vec::len).sum::<usize>());
        splits
            .iter()
            .flat_map(|s| s.iter())
            .zip(values)
            .for_each(|(a, b)| assert_eq!(*a, b));
        let socket = SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(rng.gen(), rng.gen(), rng.gen(), rng.gen()),
            rng.gen(),
        ));
        let header_size = PACKET_DATA_SIZE - PUSH_MESSAGE_MAX_PAYLOAD_SIZE;
        for values in splits {
            // Assert that sum of parts equals the whole.
            let size: u64 = header_size as u64
                + values
                    .iter()
                    .map(|v| serialized_size(v).unwrap())
                    .sum::<u64>();
            let message = Protocol::PushMessage(self_pubkey, values);
            assert_eq!(serialized_size(&message).unwrap(), size);
            // Assert that the message fits into a packet.
            assert!(Packet::from_data(&socket, message).is_ok());
        }
    }

    #[test]
    fn test_split_messages_packet_size() {
        // Test that if a value is smaller than payload size but too large to be wrapped in a vec
        // that it is still dropped
        let mut value = CrdsValue::new_unsigned(CrdsData::SnapshotHashes(SnapshotHash {
            from: Pubkey::default(),
            hashes: vec![],
            wallclock: 0,
        }));

        let mut i = 0;
        while value.size() < PUSH_MESSAGE_MAX_PAYLOAD_SIZE as u64 {
            value.data = CrdsData::SnapshotHashes(SnapshotHash {
                from: Pubkey::default(),
                hashes: vec![(0, Hash::default()); i],
                wallclock: 0,
            });
            i += 1;
        }
        let split: Vec<_> =
            ClusterInfo::split_gossip_messages(PUSH_MESSAGE_MAX_PAYLOAD_SIZE, vec![value])
                .collect();
        assert_eq!(split.len(), 0);
    }

    fn test_split_messages(value: CrdsValue) {
        const NUM_VALUES: u64 = 30;
        let value_size = value.size();
        let num_values_per_payload = (PUSH_MESSAGE_MAX_PAYLOAD_SIZE as u64 / value_size).max(1);

        // Expected len is the ceiling of the division
        let expected_len = (NUM_VALUES + num_values_per_payload - 1) / num_values_per_payload;
        let msgs = vec![value; NUM_VALUES as usize];

        let split: Vec<_> =
            ClusterInfo::split_gossip_messages(PUSH_MESSAGE_MAX_PAYLOAD_SIZE, msgs).collect();
        assert!(split.len() as u64 <= expected_len);
    }

    #[test]
    fn test_crds_filter_size() {
        //sanity test to ensure filter size never exceeds MTU size
        check_pull_request_size(CrdsFilter::new_rand(1000, 10));
        check_pull_request_size(CrdsFilter::new_rand(1000, 1000));
        check_pull_request_size(CrdsFilter::new_rand(100_000, 1000));
        check_pull_request_size(CrdsFilter::new_rand(100_000, MAX_BLOOM_SIZE));
    }

    fn check_pull_request_size(filter: CrdsFilter) {
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        let protocol = Protocol::PullRequest(filter, value);
        assert!(serialized_size(&protocol).unwrap() <= PACKET_DATA_SIZE as u64);
    }

    #[test]
    fn test_tvu_peers_and_stakes() {
        let d = ContactInfo::new_localhost(&Pubkey::new(&[0; 32]), timestamp());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(d.clone());
        let mut stakes = HashMap::new();

        // no stake
        let id = Pubkey::new(&[1u8; 32]);
        let contact_info = ContactInfo::new_localhost(&id, timestamp());
        cluster_info.insert_info(contact_info);

        // normal
        let id2 = Pubkey::new(&[2u8; 32]);
        let mut contact_info = ContactInfo::new_localhost(&id2, timestamp());
        cluster_info.insert_info(contact_info.clone());
        stakes.insert(id2, 10);

        // duplicate
        contact_info.wallclock = timestamp() + 1;
        cluster_info.insert_info(contact_info);

        // no tvu
        let id3 = Pubkey::new(&[3u8; 32]);
        let mut contact_info = ContactInfo::new_localhost(&id3, timestamp());
        contact_info.tvu = "0.0.0.0:0".parse().unwrap();
        cluster_info.insert_info(contact_info);
        stakes.insert(id3, 10);

        // normal but with different shred version
        let id4 = Pubkey::new(&[4u8; 32]);
        let mut contact_info = ContactInfo::new_localhost(&id4, timestamp());
        contact_info.shred_version = 1;
        assert_ne!(contact_info.shred_version, d.shred_version);
        cluster_info.insert_info(contact_info);
        stakes.insert(id4, 10);

        let stakes = Arc::new(stakes);
        let mut peers = cluster_info.tvu_peers();
        let peers_and_stakes = stake_weight_peers(&mut peers, Some(stakes));
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].id, id);
        assert_eq!(peers[1].id, id2);
        assert_eq!(peers_and_stakes.len(), 2);
        assert_eq!(peers_and_stakes[0].0, 10);
        assert_eq!(peers_and_stakes[1].0, 1);
    }

    #[test]
    fn test_pull_from_entrypoint_if_not_present() {
        let thread_pool = ThreadPoolBuilder::new().build().unwrap();
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        let entrypoint_pubkey = solana_sdk::pubkey::new_rand();
        let mut entrypoint = ContactInfo::new_localhost(&entrypoint_pubkey, timestamp());
        entrypoint.gossip = socketaddr!("127.0.0.2:1234");
        cluster_info.set_entrypoint(entrypoint.clone());

        let mut stakes = HashMap::new();

        let other_node_pubkey = solana_sdk::pubkey::new_rand();
        let other_node = ContactInfo::new_localhost(&other_node_pubkey, timestamp());
        assert_ne!(other_node.gossip, entrypoint.gossip);
        cluster_info.insert_info(other_node.clone());
        stakes.insert(other_node_pubkey, 10);

        // Pull request 1:  `other_node` is present but `entrypoint` was just added (so it has a
        // fresh timestamp).  There should only be one pull request to `other_node`
        let pulls = cluster_info.new_pull_requests(&thread_pool, None, &stakes);
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);

        // Pull request 2: pretend it's been a while since we've pulled from `entrypoint`.  There should
        // now be two pull requests
        cluster_info
            .entrypoint
            .write()
            .unwrap()
            .as_mut()
            .unwrap()
            .wallclock = 0;
        let pulls = cluster_info.new_pull_requests(&thread_pool, None, &stakes);
        assert_eq!(2, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);
        assert_eq!(pulls.get(1).unwrap().0, entrypoint.gossip);

        // Pull request 3:  `other_node` is present and `entrypoint` was just pulled from.  There should
        // only be one pull request to `other_node`
        let pulls = cluster_info.new_pull_requests(&thread_pool, None, &stakes);
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);
    }

    #[test]
    fn test_repair_peers() {
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        for i in 0..10 {
            // make these invalid for the upcoming repair request
            let peer_lowest = if i >= 5 { 10 } else { 0 };
            let other_node_pubkey = solana_sdk::pubkey::new_rand();
            let other_node = ContactInfo::new_localhost(&other_node_pubkey, timestamp());
            cluster_info.insert_info(other_node.clone());
            let value = CrdsValue::new_unsigned(CrdsData::LowestSlot(
                0,
                LowestSlot::new(other_node_pubkey, peer_lowest, timestamp()),
            ));
            let _ = cluster_info
                .gossip
                .write()
                .unwrap()
                .crds
                .insert(value, timestamp());
        }
        // only half the visible peers should be eligible to serve this repair
        assert_eq!(cluster_info.repair_peers(5).len(), 5);
    }

    #[test]
    fn test_max_bloom_size() {
        // check that the constant fits into the dynamic size
        assert!(MAX_BLOOM_SIZE <= max_bloom_size());
    }

    #[test]
    fn test_protocol_sanitize() {
        let pd = PruneData {
            wallclock: MAX_WALLCLOCK,
            ..PruneData::default()
        };
        let msg = Protocol::PruneMessage(Pubkey::default(), pd);
        assert_eq!(msg.sanitize(), Err(SanitizeError::ValueOutOfBounds));
    }

    // computes the maximum size for pull request blooms
    fn max_bloom_size() -> usize {
        let filter_size = serialized_size(&CrdsFilter::default())
            .expect("unable to serialize default filter") as usize;
        let protocol = Protocol::PullRequest(
            CrdsFilter::default(),
            CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default())),
        );
        let protocol_size =
            serialized_size(&protocol).expect("unable to serialize gossip protocol") as usize;
        PACKET_DATA_SIZE - (protocol_size - filter_size)
    }

    #[test]
    #[allow(clippy::same_item_push)]
    fn test_push_epoch_slots_large() {
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        let mut range: Vec<Slot> = vec![];
        //random should be hard to compress
        for _ in 0..32000 {
            let last = *range.last().unwrap_or(&0);
            range.push(last + rand::thread_rng().gen_range(1, 32));
        }
        cluster_info.push_epoch_slots(&range[..16000]);
        cluster_info.flush_push_queue();
        cluster_info.push_epoch_slots(&range[16000..]);
        cluster_info.flush_push_queue();
        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        let slots: Vec<_> = slots.iter().flat_map(|x| x.to_slots(0)).collect();
        assert_eq!(slots, range);
        assert!(since.is_some());
    }

    #[test]
    fn test_vote_size() {
        let slots = vec![1; 32];
        let vote = Vote::new(slots, Hash::default());
        let keypair = Arc::new(Keypair::new());

        // Create the biggest possible vote transaction
        let vote_ix = vote_instruction::vote_switch(
            &keypair.pubkey(),
            &keypair.pubkey(),
            vote,
            Hash::default(),
        );
        let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&keypair.pubkey()));

        vote_tx.partial_sign(&[keypair.as_ref()], Hash::default());
        vote_tx.partial_sign(&[keypair.as_ref()], Hash::default());

        let vote = CrdsVote {
            from: keypair.pubkey(),
            transaction: vote_tx,
            wallclock: 0,
        };
        let vote = CrdsValue::new_signed(CrdsData::Vote(1, vote), &Keypair::new());
        assert!(bincode::serialized_size(&vote).unwrap() <= PUSH_MESSAGE_MAX_PAYLOAD_SIZE as u64);
    }

    #[test]
    fn test_process_entrypoint_adopt_shred_version() {
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = Arc::new(ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        ));
        assert_eq!(cluster_info.my_shred_version(), 0);

        // Simulating starting up with default entrypoint, no known id, only a gossip
        // address
        let entrypoint_gossip_addr = socketaddr!("127.0.0.2:1234");
        let mut entrypoint = ContactInfo::new_localhost(&Pubkey::default(), timestamp());
        entrypoint.gossip = entrypoint_gossip_addr;
        assert_eq!(entrypoint.shred_version, 0);
        cluster_info.set_entrypoint(entrypoint);

        // Simulate getting entrypoint ContactInfo from gossip with an entrypoint shred version of
        // 0
        let mut gossiped_entrypoint_info =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        gossiped_entrypoint_info.gossip = entrypoint_gossip_addr;
        gossiped_entrypoint_info.shred_version = 0;
        cluster_info.insert_info(gossiped_entrypoint_info.clone());

        // Adopt the entrypoint's gossiped contact info and verify
        let mut entrypoint_processed = false;
        ClusterInfo::process_entrypoint(&cluster_info, &mut entrypoint_processed);
        assert_eq!(
            cluster_info.entrypoint.read().unwrap().as_ref().unwrap(),
            &gossiped_entrypoint_info
        );
        assert!(!entrypoint_processed); // <--- entrypoint processing incomplete because shred adoption still pending
        assert_eq!(cluster_info.my_shred_version(), 0); // <-- shred version still 0

        // Simulate getting entrypoint ContactInfo from gossip with an entrypoint shred version of
        // !0
        gossiped_entrypoint_info.shred_version = 1;
        cluster_info.insert_info(gossiped_entrypoint_info.clone());

        // Adopt the entrypoint's gossiped contact info and verify
        let mut entrypoint_processed = false;
        ClusterInfo::process_entrypoint(&cluster_info, &mut entrypoint_processed);
        assert_eq!(
            cluster_info.entrypoint.read().unwrap().as_ref().unwrap(),
            &gossiped_entrypoint_info
        );
        assert!(entrypoint_processed);
        assert_eq!(cluster_info.my_shred_version(), 1); // <-- shred version now adopted from entrypoint
    }

    #[test]
    fn test_process_entrypoint_without_adopt_shred_version() {
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = Arc::new(ClusterInfo::new(
            {
                let mut contact_info =
                    ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp());
                contact_info.shred_version = 2;
                contact_info
            },
            node_keypair,
        ));
        assert_eq!(cluster_info.my_shred_version(), 2);

        // Simulating starting up with default entrypoint, no known id, only a gossip
        // address
        let entrypoint_gossip_addr = socketaddr!("127.0.0.2:1234");
        let mut entrypoint = ContactInfo::new_localhost(&Pubkey::default(), timestamp());
        entrypoint.gossip = entrypoint_gossip_addr;
        assert_eq!(entrypoint.shred_version, 0);
        cluster_info.set_entrypoint(entrypoint);

        // Simulate getting entrypoint ContactInfo from gossip
        let mut gossiped_entrypoint_info =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        gossiped_entrypoint_info.gossip = entrypoint_gossip_addr;
        gossiped_entrypoint_info.shred_version = 1;
        cluster_info.insert_info(gossiped_entrypoint_info.clone());

        // Adopt the entrypoint's gossiped contact info and verify
        let mut entrypoint_processed = false;
        ClusterInfo::process_entrypoint(&cluster_info, &mut entrypoint_processed);
        assert_eq!(
            cluster_info.entrypoint.read().unwrap().as_ref().unwrap(),
            &gossiped_entrypoint_info
        );
        assert!(entrypoint_processed);
        assert_eq!(cluster_info.my_shred_version(), 2); // <--- No change to shred version
    }
}
