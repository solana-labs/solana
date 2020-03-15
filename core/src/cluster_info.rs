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
use crate::packet::limited_deserialize;
use crate::streamer::{PacketReceiver, PacketSender};
use crate::{
    contact_info::ContactInfo,
    crds_gossip::CrdsGossip,
    crds_gossip_error::CrdsGossipError,
    crds_gossip_pull::{CrdsFilter, CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS},
    crds_value::{
        self, CrdsData, CrdsValue, CrdsValueLabel, EpochSlotsIndex, LowestSlot, SnapshotHash, Vote,
    },
    epoch_slots::EpochSlots,
    packet::{Packet, PACKET_DATA_SIZE},
    result::{Error, Result},
    sendmmsg::{multicast, send_mmsg},
    weighted_shuffle::{weighted_best, weighted_shuffle},
};
use bincode::{serialize, serialized_size};
use core::cmp;
use itertools::Itertools;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::ThreadPool;
use solana_ledger::{bank_forks::BankForks, staking_utils};
use solana_measure::thread_mem_usage;
use solana_metrics::{datapoint_debug, inc_new_counter_debug, inc_new_counter_error};
use solana_net_utils::{
    bind_common, bind_common_in_range, bind_in_range, find_available_port_in_range,
    multi_bind_in_range, PortRange,
};
use solana_perf::packet::{to_packets_with_destination, Packets, PacketsRecycler};
use solana_rayon_threadlimit::get_thread_count;
use solana_sdk::hash::Hash;
use solana_sdk::timing::duration_as_s;
use solana_sdk::{
    clock::{Slot, DEFAULT_MS_PER_SLOT, DEFAULT_SLOTS_PER_EPOCH},
    pubkey::Pubkey,
    signature::{Keypair, Signable, Signature},
    timing::{duration_as_ms, timestamp},
    transaction::Transaction,
};
use std::{
    borrow::Cow,
    cmp::min,
    collections::{HashMap, HashSet},
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::{sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

pub const VALIDATOR_PORT_RANGE: PortRange = (8000, 10_000);

/// The Data plane fanout size, also used as the neighborhood size
pub const DATA_PLANE_FANOUT: usize = 200;
/// milliseconds we sleep for between gossip requests
pub const GOSSIP_SLEEP_MILLIS: u64 = 100;
/// The maximum size of a bloom filter
pub const MAX_BLOOM_SIZE: usize = MAX_CRDS_OBJECT_SIZE;
pub const MAX_CRDS_OBJECT_SIZE: usize = 928;
/// The maximum size of a protocol payload
const MAX_PROTOCOL_PAYLOAD_SIZE: u64 = PACKET_DATA_SIZE as u64 - MAX_PROTOCOL_HEADER_SIZE;
/// The largest protocol header size
const MAX_PROTOCOL_HEADER_SIZE: u64 = 214;
/// A hard limit on incoming gossip messages
/// Chosen to be able to handle 1Gbps of pure gossip traffic
/// 128MB/PACKET_DATA_SIZE
const MAX_GOSSIP_TRAFFIC: usize = 128_000_000 / PACKET_DATA_SIZE;

/// Keep the number of snapshot hashes a node publishes under MAX_PROTOCOL_PAYLOAD_SIZE
pub const MAX_SNAPSHOT_HASHES: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub enum ClusterInfoError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadGossipAddress,
}
#[derive(Clone)]
pub struct ClusterInfo {
    /// The network
    pub gossip: CrdsGossip,
    /// set the keypair that will be used to sign crds values generated. It is unset only in tests.
    pub(crate) keypair: Arc<Keypair>,
    /// The network entrypoint
    entrypoint: Option<ContactInfo>,
    last_datapoint_submit: Instant,
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

#[derive(Debug, Default, Deserialize, Serialize)]
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

// TODO These messages should go through the gpu pipeline for spam filtering
#[derive(Serialize, Deserialize, Debug)]
#[allow(clippy::large_enum_variant)]
enum Protocol {
    /// Gossip protocol messages
    PullRequest(CrdsFilter, CrdsValue),
    PullResponse(Pubkey, Vec<CrdsValue>),
    PushMessage(Pubkey, Vec<CrdsValue>),
    PruneMessage(Pubkey, PruneData),
}

impl ClusterInfo {
    /// Without a valid keypair gossip will not function. Only useful for tests.
    pub fn new_with_invalid_keypair(contact_info: ContactInfo) -> Self {
        Self::new(contact_info, Arc::new(Keypair::new()))
    }

    pub fn new(contact_info: ContactInfo, keypair: Arc<Keypair>) -> Self {
        let mut me = Self {
            gossip: CrdsGossip::default(),
            keypair,
            entrypoint: None,
            last_datapoint_submit: Instant::now(),
        };
        let id = contact_info.id;
        me.gossip.set_self(&id);
        me.insert_self(contact_info);
        me.push_self(&HashMap::new());
        me
    }

    pub fn insert_self(&mut self, contact_info: ContactInfo) {
        if self.id() == contact_info.id {
            let value = CrdsValue::new_signed(CrdsData::ContactInfo(contact_info), &self.keypair);
            let _ = self.gossip.crds.insert(value, timestamp());
        }
    }

    fn push_self(&mut self, stakes: &HashMap<Pubkey, u64>) {
        let mut my_data = self.my_data();
        let now = timestamp();
        my_data.wallclock = now;
        let entry = CrdsValue::new_signed(CrdsData::ContactInfo(my_data), &self.keypair);
        self.gossip.refresh_push_active_set(stakes);
        self.gossip
            .process_push_message(&self.id(), vec![entry], now);
    }

    // TODO kill insert_info, only used by tests
    pub fn insert_info(&mut self, contact_info: ContactInfo) {
        let value = CrdsValue::new_signed(CrdsData::ContactInfo(contact_info), &self.keypair);
        let _ = self.gossip.crds.insert(value, timestamp());
    }

    pub fn set_entrypoint(&mut self, entrypoint: ContactInfo) {
        self.entrypoint = Some(entrypoint)
    }

    pub fn id(&self) -> Pubkey {
        self.gossip.id
    }

    pub fn lookup(&self, id: &Pubkey) -> Option<&ContactInfo> {
        let entry = CrdsValueLabel::ContactInfo(*id);
        self.gossip
            .crds
            .lookup(&entry)
            .and_then(CrdsValue::contact_info)
    }

    pub fn my_data(&self) -> ContactInfo {
        self.lookup(&self.id()).cloned().unwrap()
    }

    pub fn lookup_epoch_slots(&self, ix: EpochSlotsIndex) -> EpochSlots {
        let entry = CrdsValueLabel::EpochSlots(ix, self.id());
        self.gossip
            .crds
            .lookup(&entry)
            .and_then(CrdsValue::epoch_slots)
            .cloned()
            .unwrap_or_else(|| EpochSlots::new(self.id(), timestamp()))
    }

    pub fn contact_info_trace(&self) -> String {
        let now = timestamp();
        let mut spy_nodes = 0;
        let mut archivers = 0;
        let my_pubkey = self.my_data().id;
        let nodes: Vec<_> = self
            .all_peers()
            .into_iter()
            .map(|(node, last_updated)| {
                if Self::is_spy_node(&node) {
                    spy_nodes += 1;
                } else if Self::is_archiver(&node) {
                    archivers += 1;
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

                let ip_addr = node.gossip.ip();
                format!(
                    "{:15} {:2}| {:5} | {:44} | {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {:5}| {}\n",
                    if ContactInfo::is_valid_address(&node.gossip) {
                        ip_addr.to_string()
                    } else {
                        "none".to_string()
                    },
                    if node.id == my_pubkey { "me" } else { "" }.to_string(),
                    now.saturating_sub(last_updated),
                    node.id.to_string(),
                    addr_to_string(&ip_addr, &node.gossip),
                    addr_to_string(&ip_addr, &node.tpu),
                    addr_to_string(&ip_addr, &node.tpu_forwards),
                    addr_to_string(&ip_addr, &node.tvu),
                    addr_to_string(&ip_addr, &node.tvu_forwards),
                    addr_to_string(&ip_addr, &node.repair),
                    addr_to_string(&ip_addr, &node.serve_repair),
                    addr_to_string(&ip_addr, &node.storage_addr),
                    addr_to_string(&ip_addr, &node.rpc),
                    addr_to_string(&ip_addr, &node.rpc_pubsub),
                    node.shred_version,
                )
            })
            .collect();

        format!(
            "IP Address        |Age(ms)| Node identifier                              \
             |Gossip| TPU  |TPUfwd| TVU  |TVUfwd|Repair|ServeR|Storag| RPC  |PubSub|ShredVer\n\
             ------------------+-------+----------------------------------------------+\
             ------+------+------+------+------+------+------+------+------+------+--------\n\
             {}\
             Nodes: {}{}{}",
            nodes.join(""),
            nodes.len() - spy_nodes - archivers,
            if archivers > 0 {
                format!("\nArchivers: {}", archivers)
            } else {
                "".to_string()
            },
            if spy_nodes > 0 {
                format!("\nSpies: {}", spy_nodes)
            } else {
                "".to_string()
            }
        )
    }

    pub fn push_lowest_slot(&mut self, id: Pubkey, min: Slot) {
        let now = timestamp();
        let last = self
            .gossip
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
            self.gossip
                .process_push_message(&self.id(), vec![entry], now);
        }
    }

    pub fn push_epoch_slots(&mut self, update: &[Slot]) {
        let mut num = 0;
        let mut current_slots: Vec<_> = (0..crds_value::MAX_EPOCH_SLOTS)
            .filter_map(|ix| {
                Some((
                    self.gossip
                        .crds
                        .lookup(&CrdsValueLabel::EpochSlots(ix, self.id()))
                        .and_then(CrdsValue::epoch_slots)
                        .and_then(|x| Some((x.wallclock, x.first_slot()?)))?,
                    ix,
                ))
            })
            .collect();
        current_slots.sort();
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
                self.gossip
                    .process_push_message(&self.id(), vec![entry], now);
            }
            num += n;
            if num < update.len() {
                epoch_slot_index += 1;
                reset = true;
            }
        }
    }
    pub fn push_snapshot_hashes(&mut self, snapshot_hashes: Vec<(Slot, Hash)>) {
        if snapshot_hashes.len() > MAX_SNAPSHOT_HASHES {
            warn!(
                "snapshot_hashes too large, ignored: {}",
                snapshot_hashes.len()
            );
            return;
        }

        let now = timestamp();
        let entry = CrdsValue::new_signed(
            CrdsData::SnapshotHash(SnapshotHash::new(self.id(), snapshot_hashes, now)),
            &self.keypair,
        );
        self.gossip
            .process_push_message(&self.id(), vec![entry], now);
    }

    pub fn push_vote(&mut self, tower_index: usize, vote: Transaction) {
        let now = timestamp();
        let vote = Vote::new(&self.id(), vote, now);
        let current_votes: Vec<_> = (0..crds_value::MAX_VOTES)
            .filter_map(|ix| {
                self.gossip
                    .crds
                    .lookup(&CrdsValueLabel::Vote(ix, self.id()))
            })
            .collect();
        let vote_ix = CrdsValue::compute_vote_index(tower_index, current_votes);
        let entry = CrdsValue::new_signed(CrdsData::Vote(vote_ix, vote), &self.keypair);
        self.gossip
            .process_push_message(&self.id(), vec![entry], now);
    }

    /// Get votes in the crds
    /// * since - The timestamp of when the vote inserted must be greater than
    /// since. This allows the bank to query for new votes only.
    ///
    /// * return - The votes, and the max timestamp from the new set.
    pub fn get_votes(&self, since: u64) -> (Vec<Transaction>, u64) {
        let votes: Vec<_> = self
            .gossip
            .crds
            .table
            .values()
            .filter(|x| x.insert_timestamp > since)
            .filter_map(|x| {
                x.value
                    .vote()
                    .map(|v| (x.insert_timestamp, v.transaction.clone()))
            })
            .collect();
        let max_ts = votes.iter().map(|x| x.0).max().unwrap_or(since);
        let txs: Vec<Transaction> = votes.into_iter().map(|x| x.1).collect();
        inc_new_counter_info!("cluster_info-get_votes-count", txs.len());
        (txs, max_ts)
    }

    pub fn get_snapshot_hash(&self, slot: Slot) -> Vec<(Pubkey, Hash)> {
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.snapshot_hash().map(|v| v))
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

    pub fn get_snapshot_hash_for_node(&self, pubkey: &Pubkey) -> Option<&Vec<(Slot, Hash)>> {
        self.gossip
            .crds
            .table
            .get(&CrdsValueLabel::SnapshotHash(*pubkey))
            .map(|x| &x.value.snapshot_hash().unwrap().hashes)
    }

    pub fn get_lowest_slot_for_node(
        &self,
        pubkey: &Pubkey,
        since: Option<u64>,
    ) -> Option<(&LowestSlot, u64)> {
        self.gossip
            .crds
            .table
            .get(&CrdsValueLabel::LowestSlot(*pubkey))
            .filter(|x| {
                since
                    .map(|since| x.insert_timestamp > since)
                    .unwrap_or(true)
            })
            .map(|x| (x.value.lowest_slot().unwrap(), x.insert_timestamp))
    }

    pub fn get_epoch_slots_since(&self, since: Option<u64>) -> (Vec<EpochSlots>, Option<u64>) {
        let vals: Vec<_> = self
            .gossip
            .crds
            .table
            .values()
            .filter(|x| {
                since
                    .map(|since| x.insert_timestamp > since)
                    .unwrap_or(true)
            })
            .filter_map(|x| Some((x.value.epoch_slots()?, x.insert_timestamp)))
            .collect();
        let max = vals.iter().map(|x| x.1).max().or(since);
        let vec = vals.into_iter().map(|x| x.0).cloned().collect();
        (vec, max)
    }

    pub fn get_contact_info_for_node(&self, pubkey: &Pubkey) -> Option<&ContactInfo> {
        self.gossip
            .crds
            .table
            .get(&CrdsValueLabel::ContactInfo(*pubkey))
            .map(|x| x.value.contact_info().unwrap())
    }

    /// all validators that have a valid rpc port regardless of `shred_version`.
    pub fn all_rpc_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me.id && ContactInfo::is_valid_address(&x.rpc))
            .cloned()
            .collect()
    }

    // All nodes in gossip (including spy nodes) and the last time we heard about them
    pub(crate) fn all_peers(&self) -> Vec<(ContactInfo, u64)> {
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| {
                x.value
                    .contact_info()
                    .map(|ci| (ci.clone(), x.local_timestamp))
            })
            .collect()
    }

    pub fn gossip_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            // shred_version not considered for gossip peers (ie, spy nodes do not set shred_version)
            .filter(|x| x.id != me && ContactInfo::is_valid_address(&x.gossip))
            .cloned()
            .collect()
    }

    /// all validators that have a valid tvu port regardless of `shred_version`.
    pub fn all_tvu_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| {
                ContactInfo::is_valid_address(&x.tvu)
                    && !ClusterInfo::is_archiver(x)
                    && x.id != me.id
            })
            .cloned()
            .collect()
    }

    /// all validators that have a valid tvu port and are on the same `shred_version`.
    pub fn tvu_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| {
                ContactInfo::is_valid_address(&x.tvu)
                    && !ClusterInfo::is_archiver(x)
                    && x.id != me.id
                    && x.shred_version == me.shred_version
            })
            .cloned()
            .collect()
    }

    /// all peers that have a valid storage addr regardless of `shred_version`.
    pub fn all_storage_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| ContactInfo::is_valid_address(&x.storage_addr) && x.id != me.id)
            .cloned()
            .collect()
    }

    /// all peers that have a valid storage addr and are on the same `shred_version`.
    pub fn storage_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| {
                ContactInfo::is_valid_address(&x.storage_addr)
                    && x.id != me.id
                    && x.shred_version == me.shred_version
            })
            .cloned()
            .collect()
    }

    /// all peers that have a valid tvu
    pub fn retransmit_peers(&self) -> Vec<ContactInfo> {
        let me = self.my_data();
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| {
                x.id != me.id
                    && x.shred_version == me.shred_version
                    && ContactInfo::is_valid_address(&x.tvu)
                    && ContactInfo::is_valid_address(&x.tvu_forwards)
            })
            .cloned()
            .collect()
    }

    /// all tvu peers with valid gossip addrs that likely have the slot being requested
    pub fn repair_peers(&self, slot: Slot) -> Vec<ContactInfo> {
        let me = self.my_data();
        ClusterInfo::tvu_peers(self)
            .into_iter()
            .filter(|x| {
                x.id != me.id
                    && x.shred_version == me.shred_version
                    && ContactInfo::is_valid_address(&x.serve_repair)
                    && {
                        self.get_lowest_slot_for_node(&x.id, None)
                            .map(|(lowest_slot, _)| lowest_slot.lowest <= slot)
                            .unwrap_or_else(|| /* fallback to legacy behavior */ true)
                    }
            })
            .collect()
    }

    fn is_spy_node(contact_info: &ContactInfo) -> bool {
        (!ContactInfo::is_valid_address(&contact_info.tpu)
            || !ContactInfo::is_valid_address(&contact_info.gossip)
            || !ContactInfo::is_valid_address(&contact_info.tvu))
            && !ContactInfo::is_valid_address(&contact_info.storage_addr)
    }

    pub fn is_archiver(contact_info: &ContactInfo) -> bool {
        ContactInfo::is_valid_address(&contact_info.storage_addr)
            && !ContactInfo::is_valid_address(&contact_info.tpu)
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
        peers.push(self.lookup(&self.id()).unwrap().clone());
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
        let me = self.my_data().id;
        self.gossip
            .crds
            .table
            .values()
            .filter_map(|x| x.value.contact_info())
            .filter(|x| x.id != me && ContactInfo::is_valid_address(&x.tpu))
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

    fn sorted_tvu_peers_and_stakes(
        &self,
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    ) -> (Vec<ContactInfo>, Vec<(u64, usize)>) {
        let mut peers = self.tvu_peers();
        peers.dedup();
        let stakes_and_index = ClusterInfo::sorted_stakes_with_index(&peers, stakes);
        (peers, stakes_and_index)
    }

    /// broadcast messages from the leader to layer 1 nodes
    /// # Remarks
    pub fn broadcast_shreds(
        &mut self,
        s: &UdpSocket,
        shreds: Vec<Vec<u8>>,
        seeds: &[[u8; 32]],
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    ) -> Result<()> {
        let (peers, peers_and_stakes) = self.sorted_tvu_peers_and_stakes(stakes);
        let broadcast_len = peers_and_stakes.len();
        if broadcast_len == 0 {
            if duration_as_s(&Instant::now().duration_since(self.last_datapoint_submit)) >= 1.0 {
                datapoint_info!(
                    "cluster_info-num_nodes",
                    ("live_count", 1, i64),
                    ("broadcast_count", 1, i64)
                );
                self.last_datapoint_submit = Instant::now();
            }
            return Ok(());
        }
        let mut packets: Vec<_> = shreds
            .into_iter()
            .zip(seeds)
            .map(|(shred, seed)| {
                let broadcast_index = weighted_best(&peers_and_stakes, *seed);

                (shred, &peers[broadcast_index].tvu)
            })
            .collect();

        let mut sent = 0;
        while sent < packets.len() {
            match send_mmsg(s, &mut packets[sent..]) {
                Ok(n) => sent += n,
                Err(e) => {
                    return Err(Error::IO(e));
                }
            }
        }

        let mut num_live_peers = 1i64;
        peers.iter().for_each(|p| {
            // A peer is considered live if they generated their contact info recently
            if timestamp() - p.wallclock <= CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS {
                num_live_peers += 1;
            }
        });
        if duration_as_s(&Instant::now().duration_since(self.last_datapoint_submit)) >= 1.0 {
            datapoint_info!(
                "cluster_info-num_nodes",
                ("live_count", num_live_peers, i64),
                ("broadcast_count", broadcast_len + 1, i64)
            );
            self.last_datapoint_submit = Instant::now();
        }
        Ok(())
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

    // If the network entrypoint hasn't been discovered yet, add it to the crds table
    fn add_entrypoint(&mut self, pulls: &mut Vec<(Pubkey, CrdsFilter, SocketAddr, CrdsValue)>) {
        let pull_from_entrypoint = if let Some(entrypoint) = &mut self.entrypoint {
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
                    let found_entrypoint = self.gossip.crds.table.iter().any(|(_, v)| {
                        v.value
                            .contact_info()
                            .map(|ci| ci.gossip == entrypoint.gossip)
                            .unwrap_or(false)
                    });
                    !found_entrypoint
                }
            }
        } else {
            false
        };

        if pull_from_entrypoint {
            if let Some(entrypoint) = &self.entrypoint {
                let self_info = self
                    .gossip
                    .crds
                    .lookup(&CrdsValueLabel::ContactInfo(self.id()))
                    .unwrap_or_else(|| panic!("self_id invalid {}", self.id()));

                return self
                    .gossip
                    .pull
                    .build_crds_filters(&self.gossip.crds, MAX_BLOOM_SIZE)
                    .into_iter()
                    .for_each(|filter| {
                        pulls.push((entrypoint.id, filter, entrypoint.gossip, self_info.clone()))
                    });
            }
        }
    }

    /// Splits a Vec of CrdsValues into a nested Vec, trying to make sure that
    /// each Vec is no larger than `MAX_PROTOCOL_PAYLOAD_SIZE`
    /// Note: some messages cannot be contained within that size so in the worst case this returns
    /// N nested Vecs with 1 item each.
    fn split_gossip_messages(msgs: Vec<CrdsValue>) -> Vec<Vec<CrdsValue>> {
        let mut messages = vec![];
        let mut payload = vec![];
        let base_size = serialized_size(&payload).expect("Couldn't check size");
        let max_payload_size = MAX_PROTOCOL_PAYLOAD_SIZE - base_size;
        let mut payload_size = 0;
        for msg in msgs {
            let msg_size = msg.size();
            // If the message is too big to fit in this batch
            if payload_size + msg_size > max_payload_size as u64 {
                // See if it can fit in the next batch
                if msg_size <= max_payload_size as u64 {
                    if !payload.is_empty() {
                        // Flush the  current payload
                        messages.push(payload);
                        // Init the next payload
                        payload = vec![msg];
                        payload_size = msg_size;
                    }
                } else {
                    debug!(
                        "dropping message larger than the maximum payload size {:?}",
                        msg
                    );
                }
                continue;
            }
            payload_size += msg_size;
            payload.push(msg);
        }
        if !payload.is_empty() {
            messages.push(payload);
        }
        messages
    }

    fn new_pull_requests(&mut self, stakes: &HashMap<Pubkey, u64>) -> Vec<(SocketAddr, Protocol)> {
        let now = timestamp();
        let mut pulls: Vec<_> = self
            .gossip
            .new_pull_request(now, stakes, MAX_BLOOM_SIZE)
            .ok()
            .into_iter()
            .filter_map(|(peer, filters, me)| {
                let peer_label = CrdsValueLabel::ContactInfo(peer);
                self.gossip
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
            .collect();
        self.add_entrypoint(&mut pulls);
        pulls
            .into_iter()
            .map(|(peer, filter, gossip, self_info)| {
                self.gossip.mark_pull_request_creation_time(&peer, now);
                (gossip, Protocol::PullRequest(filter, self_info))
            })
            .collect()
    }
    fn new_push_requests(&mut self) -> Vec<(SocketAddr, Protocol)> {
        let self_id = self.gossip.id;
        let (_, push_messages) = self.gossip.new_push_messages(timestamp());
        push_messages
            .into_iter()
            .filter_map(|(peer, messages)| {
                let peer_label = CrdsValueLabel::ContactInfo(peer);
                self.gossip
                    .crds
                    .lookup(&peer_label)
                    .and_then(CrdsValue::contact_info)
                    .map(|p| (p.gossip, messages))
            })
            .flat_map(|(peer, msgs)| {
                Self::split_gossip_messages(msgs)
                    .into_iter()
                    .map(move |payload| (peer, Protocol::PushMessage(self_id, payload)))
            })
            .collect()
    }

    fn gossip_request(&mut self, stakes: &HashMap<Pubkey, u64>) -> Vec<(SocketAddr, Protocol)> {
        let pulls: Vec<_> = self.new_pull_requests(stakes);
        let pushes: Vec<_> = self.new_push_requests();
        vec![pulls, pushes].into_iter().flatten().collect()
    }

    /// At random pick a node and try to get updated changes from them
    fn run_gossip(
        obj: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        stakes: &HashMap<Pubkey, u64>,
        sender: &PacketSender,
    ) -> Result<()> {
        let reqs = obj.write().unwrap().gossip_request(&stakes);
        if !reqs.is_empty() {
            let packets = to_packets_with_destination(recycler.clone(), &reqs);
            sender.send(packets)?;
        }
        Ok(())
    }

    /// randomly pick a node and ask them for updates asynchronously
    pub fn gossip(
        obj: Arc<RwLock<Self>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        sender: PacketSender,
        exit: &Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        Builder::new()
            .name("solana-gossip".to_string())
            .spawn(move || {
                let mut last_push = timestamp();
                let mut last_contact_info_trace = timestamp();
                let mut adopt_shred_version = obj.read().unwrap().my_data().shred_version == 0;
                let recycler = PacketsRecycler::default();
                loop {
                    let start = timestamp();
                    thread_mem_usage::datapoint("solana-gossip");
                    if start - last_contact_info_trace > 10000 {
                        // Log contact info every 10 seconds
                        info!("\n{}", obj.read().unwrap().contact_info_trace());
                        last_contact_info_trace = start;
                    }

                    let stakes: HashMap<_, _> = match bank_forks {
                        Some(ref bank_forks) => {
                            staking_utils::staked_nodes(&bank_forks.read().unwrap().working_bank())
                        }
                        None => HashMap::new(),
                    };
                    let _ = Self::run_gossip(&obj, &recycler, &stakes, &sender);
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
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
                    let timeouts = obj.read().unwrap().gossip.make_timeouts(&stakes, timeout);
                    let num_purged = obj.write().unwrap().gossip.purge(timestamp(), &timeouts);
                    inc_new_counter_info!("cluster_info-purge-count", num_purged);
                    let table_size = obj.read().unwrap().gossip.crds.table.len();
                    datapoint_debug!(
                        "cluster_info-purge",
                        ("table_size", table_size as i64, i64),
                        ("purge_stake_timeout", timeout as i64, i64)
                    );
                    // Adopt the entrypoint's `shred_version` if ours is unset
                    if adopt_shred_version {
                        // If gossip was given an entrypoint, lookup its id
                        let entrypoint_id = obj.read().unwrap().entrypoint.as_ref().map(|e| e.id);
                        if let Some(entrypoint_id) = entrypoint_id {
                            // If a pull from the entrypoint was successful, it should exist in the crds table
                            let entrypoint = obj.read().unwrap().lookup(&entrypoint_id).cloned();
                            if let Some(entrypoint) = entrypoint {
                                let mut self_info = obj.read().unwrap().my_data();
                                if entrypoint.shred_version == 0 {
                                    info!("Unable to adopt entrypoint's shred version");
                                } else {
                                    info!(
                                        "Setting shred version to {:?} from entrypoint {:?}",
                                        entrypoint.shred_version, entrypoint.id
                                    );
                                    self_info.shred_version = entrypoint.shred_version;
                                    obj.write().unwrap().insert_self(self_info);
                                    adopt_shred_version = false;
                                }
                            }
                        }
                    }
                    //TODO: possibly tune this parameter
                    //we saw a deadlock passing an obj.read().unwrap().timeout into sleep
                    if start - last_push > CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS / 2 {
                        obj.write().unwrap().push_self(&stakes);
                        last_push = timestamp();
                    }
                    let elapsed = timestamp() - start;
                    if GOSSIP_SLEEP_MILLIS > elapsed {
                        let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                        sleep(Duration::from_millis(time_left));
                    }
                }
            })
            .unwrap()
    }

    #[allow(clippy::cognitive_complexity)]
    fn handle_packets(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        stakes: &HashMap<Pubkey, u64>,
        packets: Packets,
        response_sender: &PacketSender,
        epoch_ms: u64,
    ) {
        // iter over the packets, collect pulls separately and process everything else
        let allocated = thread_mem_usage::Allocatedp::default();
        let mut gossip_pull_data: Vec<PullData> = vec![];
        let timeouts = me.read().unwrap().gossip.make_timeouts(&stakes, epoch_ms);
        packets.packets.iter().for_each(|packet| {
            let from_addr = packet.meta.addr();
            limited_deserialize(&packet.data[..packet.meta.size])
                .into_iter()
                .for_each(|request| match request {
                    Protocol::PullRequest(filter, caller) => {
                        let start = allocated.get();
                        if !caller.verify() {
                            inc_new_counter_error!(
                                "cluster_info-gossip_pull_request_verify_fail",
                                1
                            );
                        } else if let Some(contact_info) = caller.contact_info() {
                            if contact_info.id == me.read().unwrap().gossip.id {
                                warn!("PullRequest ignored, I'm talking to myself");
                                inc_new_counter_debug!("cluster_info-window-request-loopback", 1);
                            } else {
                                gossip_pull_data.push(PullData {
                                    from_addr,
                                    caller,
                                    filter,
                                });
                            }
                        }
                        datapoint_debug!(
                            "solana-gossip-listen-memory",
                            ("pull_request", (allocated.get() - start) as i64, i64),
                        );
                    }
                    Protocol::PullResponse(from, mut data) => {
                        let start = allocated.get();
                        data.retain(|v| {
                            let ret = v.verify();
                            if !ret {
                                inc_new_counter_error!(
                                    "cluster_info-gossip_pull_response_verify_fail",
                                    1
                                );
                            }
                            ret
                        });
                        Self::handle_pull_response(me, &from, data, &timeouts);
                        datapoint_debug!(
                            "solana-gossip-listen-memory",
                            ("pull_response", (allocated.get() - start) as i64, i64),
                        );
                    }
                    Protocol::PushMessage(from, mut data) => {
                        let start = allocated.get();
                        data.retain(|v| {
                            let ret = v.verify();
                            if !ret {
                                inc_new_counter_error!(
                                    "cluster_info-gossip_push_msg_verify_fail",
                                    1
                                );
                            }
                            ret
                        });
                        let rsp = Self::handle_push_message(me, recycler, &from, data, stakes);
                        if let Some(rsp) = rsp {
                            let _ignore_disconnect = response_sender.send(rsp);
                        }
                        datapoint_debug!(
                            "solana-gossip-listen-memory",
                            ("push_message", (allocated.get() - start) as i64, i64),
                        );
                    }
                    Protocol::PruneMessage(from, data) => {
                        let start = allocated.get();
                        if data.verify() {
                            inc_new_counter_debug!("cluster_info-prune_message", 1);
                            inc_new_counter_debug!(
                                "cluster_info-prune_message-size",
                                data.prunes.len()
                            );
                            match me.write().unwrap().gossip.process_prune_msg(
                                &from,
                                &data.destination,
                                &data.prunes,
                                data.wallclock,
                                timestamp(),
                            ) {
                                Err(CrdsGossipError::PruneMessageTimeout) => {
                                    inc_new_counter_debug!("cluster_info-prune_message_timeout", 1)
                                }
                                Err(CrdsGossipError::BadPruneDestination) => {
                                    inc_new_counter_debug!("cluster_info-bad_prune_destination", 1)
                                }
                                _ => (),
                            }
                        } else {
                            inc_new_counter_debug!("cluster_info-gossip_prune_msg_verify_fail", 1);
                        }
                        datapoint_debug!(
                            "solana-gossip-listen-memory",
                            ("prune_message", (allocated.get() - start) as i64, i64),
                        );
                    }
                })
        });
        // process the collected pulls together
        let rsp = Self::handle_pull_requests(me, recycler, gossip_pull_data);
        if let Some(rsp) = rsp {
            let _ignore_disconnect = response_sender.send(rsp);
        }
    }

    fn handle_pull_requests(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        requests: Vec<PullData>,
    ) -> Option<Packets> {
        // split the requests into addrs and filters
        let mut caller_and_filters = vec![];
        let mut addrs = vec![];
        for pull_data in requests {
            caller_and_filters.push((pull_data.caller, pull_data.filter));
            addrs.push(pull_data.from_addr);
        }
        let now = timestamp();
        let self_id = me.read().unwrap().id();
        let pull_responses = me
            .write()
            .unwrap()
            .gossip
            .process_pull_requests(caller_and_filters, now);
        let mut packets = Packets::new_with_recycler(recycler.clone(), 64, "handle_pull_requests");
        pull_responses
            .into_iter()
            .zip(addrs.into_iter())
            .for_each(|(response, from_addr)| {
                if !from_addr.ip().is_unspecified() && from_addr.port() != 0 {
                    let len = response.len();
                    trace!("get updates since response {}", len);
                    inc_new_counter_debug!("cluster_info-pull_request-rsp", len);
                    Self::split_gossip_messages(response)
                        .into_iter()
                        .for_each(|payload| {
                            let protocol = Protocol::PullResponse(self_id, payload);
                            // The remote node may not know its public IP:PORT. Instead of responding to the caller's
                            // gossip addr, respond to the origin addr. The last origin addr is picked from the list of
                            // addrs.
                            packets
                                .packets
                                .push(Packet::from_data(&from_addr, protocol))
                        })
                } else {
                    trace!("Dropping Gossip pull response, as destination is unknown");
                }
            });
        if packets.is_empty() {
            return None;
        }
        Some(packets)
    }

    fn handle_pull_response(
        me: &Arc<RwLock<Self>>,
        from: &Pubkey,
        data: Vec<CrdsValue>,
        timeouts: &HashMap<Pubkey, u64>,
    ) {
        let len = data.len();
        let now = Instant::now();
        let self_id = me.read().unwrap().gossip.id;
        trace!("PullResponse me: {} from: {} len={}", self_id, from, len);
        me.write()
            .unwrap()
            .gossip
            .process_pull_response(from, timeouts, data, timestamp());
        inc_new_counter_debug!("cluster_info-pull_request_response", 1);
        inc_new_counter_debug!("cluster_info-pull_request_response-size", len);

        report_time_spent("ReceiveUpdates", &now.elapsed(), &format!(" len: {}", len));
    }

    fn handle_push_message(
        me: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        from: &Pubkey,
        data: Vec<CrdsValue>,
        stakes: &HashMap<Pubkey, u64>,
    ) -> Option<Packets> {
        let self_id = me.read().unwrap().gossip.id;
        inc_new_counter_debug!("cluster_info-push_message", 1);

        let updated: Vec<_> =
            me.write()
                .unwrap()
                .gossip
                .process_push_message(from, data, timestamp());

        let updated_labels: Vec<_> = updated.into_iter().map(|u| u.value.label()).collect();
        let prunes_map: HashMap<Pubkey, HashSet<Pubkey>> = me
            .write()
            .unwrap()
            .gossip
            .prune_received_cache(updated_labels, stakes);

        let rsp: Vec<_> = prunes_map
            .into_iter()
            .filter_map(|(from, prune_set)| {
                inc_new_counter_debug!("cluster_info-push_message-prunes", prune_set.len());
                me.read().unwrap().lookup(&from).cloned().and_then(|ci| {
                    let mut prune_msg = PruneData {
                        pubkey: self_id,
                        prunes: prune_set.into_iter().collect(),
                        signature: Signature::default(),
                        destination: from,
                        wallclock: timestamp(),
                    };
                    prune_msg.sign(&me.read().unwrap().keypair);
                    let rsp = Protocol::PruneMessage(self_id, prune_msg);
                    Some((ci.gossip, rsp))
                })
            })
            .collect();
        if rsp.is_empty() {
            return None;
        }
        let mut packets = to_packets_with_destination(recycler.clone(), &rsp);
        if !packets.is_empty() {
            let pushes: Vec<_> = me.write().unwrap().new_push_requests();
            inc_new_counter_debug!("cluster_info-push_message-pushes", pushes.len());
            pushes.into_iter().for_each(|(remote_gossip_addr, req)| {
                if !remote_gossip_addr.ip().is_unspecified() && remote_gossip_addr.port() != 0 {
                    let p = Packet::from_data(&remote_gossip_addr, &req);
                    packets.packets.push(p);
                } else {
                    trace!("Dropping Gossip push response, as destination is unknown");
                }
            });
            Some(packets)
        } else {
            None
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        recycler: &PacketsRecycler,
        bank_forks: Option<&Arc<RwLock<BankForks>>>,
        requests_receiver: &PacketReceiver,
        response_sender: &PacketSender,
        thread_pool: &ThreadPool,
    ) -> Result<()> {
        //TODO cache connections
        let timeout = Duration::new(1, 0);
        let mut requests = vec![requests_receiver.recv_timeout(timeout)?];
        let mut num_requests = requests.last().unwrap().packets.len();
        while let Ok(more_reqs) = requests_receiver.try_recv() {
            if num_requests >= MAX_GOSSIP_TRAFFIC {
                continue;
            }
            num_requests += more_reqs.packets.len();
            requests.push(more_reqs)
        }
        if num_requests >= MAX_GOSSIP_TRAFFIC {
            warn!("Too much gossip traffic, ignoring some messages");
        }
        let epoch_ms;
        let stakes: HashMap<_, _> = match bank_forks {
            Some(ref bank_forks) => {
                let bank = bank_forks.read().unwrap().working_bank();
                let epoch = bank.epoch();
                let epoch_schedule = bank.epoch_schedule();
                epoch_ms = epoch_schedule.get_slots_in_epoch(epoch) * DEFAULT_MS_PER_SLOT;
                staking_utils::staked_nodes(&bank)
            }
            None => {
                inc_new_counter_info!("cluster_info-purge-no_working_bank", 1);
                epoch_ms = CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
                HashMap::new()
            }
        };
        let sender = response_sender.clone();
        thread_pool.install(|| {
            requests.into_par_iter().for_each_with(sender, |s, reqs| {
                Self::handle_packets(obj, &recycler, &stakes, reqs, s, epoch_ms)
            });
        });

        Ok(())
    }
    pub fn listen(
        me: Arc<RwLock<Self>>,
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
                let thread_pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .build()
                    .unwrap();
                loop {
                    let e = Self::run_listen(
                        &me,
                        &recycler,
                        bank_forks.as_ref(),
                        &requests_receiver,
                        &response_sender,
                        &thread_pool,
                    );
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    if e.is_err() {
                        let me = me.read().unwrap();
                        debug!(
                            "{}: run_listen timeout, table size: {}",
                            me.gossip.id,
                            me.gossip.crds.table.len()
                        );
                    }
                    thread_mem_usage::datapoint("solana-listen");
                }
            })
            .unwrap()
    }

    pub fn gossip_contact_info(id: &Pubkey, gossip: SocketAddr) -> ContactInfo {
        ContactInfo {
            id: *id,
            gossip,
            wallclock: timestamp(),
            ..ContactInfo::default()
        }
    }

    pub fn spy_contact_info(id: &Pubkey) -> ContactInfo {
        let dummy_addr = socketaddr_any!();

        Self::gossip_contact_info(id, dummy_addr)
    }

    /// An alternative to Spy Node that has a valid gossip address and fully participate in Gossip.
    pub fn gossip_node(
        id: &Pubkey,
        gossip_addr: &SocketAddr,
    ) -> (ContactInfo, UdpSocket, Option<TcpListener>) {
        let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (port, (gossip_socket, ip_echo)) =
            Node::get_gossip_port(gossip_addr, VALIDATOR_PORT_RANGE, bind_ip_addr);
        let contact_info = Self::gossip_contact_info(id, SocketAddr::new(gossip_addr.ip(), port));

        (contact_info, gossip_socket, Some(ip_echo))
    }

    /// A Node with dummy ports to spy on gossip via pull requests
    pub fn spy_node(id: &Pubkey) -> (ContactInfo, UdpSocket, Option<TcpListener>) {
        let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_, gossip_socket) = bind_in_range(bind_ip_addr, VALIDATOR_PORT_RANGE).unwrap();
        let contact_info = Self::spy_contact_info(id);

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
    pub storage: Option<UdpSocket>,
    pub serve_repair: UdpSocket,
}

#[derive(Debug)]
pub struct Node {
    pub info: ContactInfo,
    pub sockets: Sockets,
}

impl Node {
    pub fn new_localhost() -> Self {
        let pubkey = Pubkey::new_rand();
        Self::new_localhost_with_pubkey(&pubkey)
    }
    pub fn new_localhost_archiver(pubkey: &Pubkey) -> Self {
        let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tvu = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tvu_forwards = UdpSocket::bind("127.0.0.1:0").unwrap();
        let storage = UdpSocket::bind("127.0.0.1:0").unwrap();
        let empty = "0.0.0.0:0".parse().unwrap();
        let repair = UdpSocket::bind("127.0.0.1:0").unwrap();
        let broadcast = vec![UdpSocket::bind("0.0.0.0:0").unwrap()];
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve_repair = UdpSocket::bind("127.0.0.1:0").unwrap();

        let info = ContactInfo {
            id: *pubkey,
            gossip: gossip.local_addr().unwrap(),
            tvu: tvu.local_addr().unwrap(),
            tvu_forwards: tvu_forwards.local_addr().unwrap(),
            repair: repair.local_addr().unwrap(),
            tpu: empty,
            tpu_forwards: empty,
            storage_addr: storage.local_addr().unwrap(),
            rpc: empty,
            rpc_pubsub: empty,
            serve_repair: serve_repair.local_addr().unwrap(),
            wallclock: timestamp(),
            shred_version: 0,
        };

        Node {
            info,
            sockets: Sockets {
                gossip,
                tvu: vec![tvu],
                tvu_forwards: vec![],
                tpu: vec![],
                tpu_forwards: vec![],
                broadcast,
                repair,
                retransmit_sockets: vec![retransmit],
                serve_repair,
                storage: Some(storage),
                ip_echo: None,
            },
        }
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
        let storage = UdpSocket::bind("0.0.0.0:0").unwrap();
        let serve_repair = UdpSocket::bind("127.0.0.1:0").unwrap();
        let info = ContactInfo {
            id: *pubkey,
            gossip: gossip_addr,
            tvu: tvu.local_addr().unwrap(),
            tvu_forwards: tvu_forwards.local_addr().unwrap(),
            repair: repair.local_addr().unwrap(),
            tpu: tpu.local_addr().unwrap(),
            tpu_forwards: tpu_forwards.local_addr().unwrap(),
            storage_addr: storage.local_addr().unwrap(),
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
                storage: None,
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
            storage_addr: socketaddr_any!(),
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
                storage: None,
                serve_repair,
                ip_echo: Some(ip_echo),
            },
        }
    }
    pub fn new_archiver_with_external_ip(
        pubkey: &Pubkey,
        gossip_addr: &SocketAddr,
        port_range: PortRange,
        bind_ip_addr: IpAddr,
    ) -> Node {
        let mut new = Self::new_with_external_ip(pubkey, gossip_addr, port_range, bind_ip_addr);
        let (storage_port, storage_socket) = Self::bind(bind_ip_addr, port_range);

        new.info.storage_addr = SocketAddr::new(gossip_addr.ip(), storage_port);
        new.sockets.storage = Some(storage_socket);

        let empty = socketaddr_any!();
        new.info.tpu = empty;
        new.info.tpu_forwards = empty;
        new.sockets.tpu = vec![];
        new.sockets.tpu_forwards = vec![];

        new
    }
}

fn report_time_spent(label: &str, time: &Duration, extra: &str) {
    let count = duration_as_ms(time);
    if count > 5 {
        info!("{} took: {} ms {}", label, count, extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crds_value::CrdsValueLabel;
    use rayon::prelude::*;
    use solana_perf::test_tx::test_tx;
    use solana_sdk::signature::{Keypair, Signer};
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_gossip_node() {
        //check that a gossip nodes always show up as spies
        let (node, _, _) = ClusterInfo::spy_node(&Pubkey::new_rand());
        assert!(ClusterInfo::is_spy_node(&node));
        let (node, _, _) =
            ClusterInfo::gossip_node(&Pubkey::new_rand(), &"1.1.1.1:1111".parse().unwrap());
        assert!(ClusterInfo::is_spy_node(&node));
    }

    #[test]
    fn test_cluster_spy_gossip() {
        //check that gossip doesn't try to push to invalid addresses
        let node = Node::new_localhost();
        let (spy, _, _) = ClusterInfo::spy_node(&Pubkey::new_rand());
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            node.info,
        )));
        cluster_info.write().unwrap().insert_info(spy);
        cluster_info
            .write()
            .unwrap()
            .gossip
            .refresh_push_active_set(&HashMap::new());
        let reqs = cluster_info
            .write()
            .unwrap()
            .gossip_request(&HashMap::new());
        //assert none of the addrs are invalid.
        reqs.iter().all(|(addr, _)| {
            let res = ContactInfo::is_valid_address(addr);
            assert!(res);
            res
        });
    }

    #[test]
    fn test_cluster_info_new() {
        let d = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        let cluster_info = ClusterInfo::new_with_invalid_keypair(d.clone());
        assert_eq!(d.id, cluster_info.my_data().id);
    }

    #[test]
    fn insert_info_test() {
        let d = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(d);
        let d = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        let label = CrdsValueLabel::ContactInfo(d.id);
        cluster_info.insert_info(d);
        assert!(cluster_info.gossip.crds.lookup(&label).is_some());
    }
    #[test]
    fn test_insert_self() {
        let d = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(d.clone());
        let entry_label = CrdsValueLabel::ContactInfo(cluster_info.id());
        assert!(cluster_info.gossip.crds.lookup(&entry_label).is_some());

        // inserting something else shouldn't work
        let d = ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp());
        cluster_info.insert_self(d.clone());
        let label = CrdsValueLabel::ContactInfo(d.id);
        assert!(cluster_info.gossip.crds.lookup(&label).is_none());
    }

    fn assert_in_range(x: u16, range: (u16, u16)) {
        assert!(x >= range.0);
        assert!(x < range.1);
    }

    fn check_sockets(sockets: &Vec<UdpSocket>, ip: IpAddr, range: (u16, u16)) {
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
            &Pubkey::new_rand(),
            &socketaddr!(ip, 0),
            VALIDATOR_PORT_RANGE,
            IpAddr::V4(ip),
        );

        check_node_sockets(&node, IpAddr::V4(ip), VALIDATOR_PORT_RANGE);
    }

    #[test]
    fn new_with_external_ip_test_gossip() {
        let ip = IpAddr::V4(Ipv4Addr::from(0));
        let port = {
            bind_in_range(ip, VALIDATOR_PORT_RANGE)
                .expect("Failed to bind")
                .0
        };
        let node = Node::new_with_external_ip(
            &Pubkey::new_rand(),
            &socketaddr!(0, port),
            VALIDATOR_PORT_RANGE,
            ip,
        );

        check_node_sockets(&node, ip, VALIDATOR_PORT_RANGE);

        assert_eq!(node.sockets.gossip.local_addr().unwrap().port(), port);
    }

    #[test]
    fn new_archiver_external_ip_test() {
        let ip = Ipv4Addr::from(0);
        let node = Node::new_archiver_with_external_ip(
            &Pubkey::new_rand(),
            &socketaddr!(ip, 0),
            VALIDATOR_PORT_RANGE,
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        );

        let ip = IpAddr::V4(ip);
        check_socket(&node.sockets.storage.unwrap(), ip, VALIDATOR_PORT_RANGE);
        check_socket(&node.sockets.gossip, ip, VALIDATOR_PORT_RANGE);
        check_socket(&node.sockets.repair, ip, VALIDATOR_PORT_RANGE);

        check_sockets(&node.sockets.tvu, ip, VALIDATOR_PORT_RANGE);
    }

    //test that all cluster_info objects only generate signed messages
    //when constructed with keypairs
    #[test]
    fn test_gossip_signature_verification() {
        //create new cluster info, leader, and peer
        let keypair = Keypair::new();
        let peer_keypair = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let peer = ContactInfo::new_localhost(&peer_keypair.pubkey(), 0);
        let mut cluster_info = ClusterInfo::new(contact_info.clone(), Arc::new(keypair));
        cluster_info.insert_info(peer.clone());
        cluster_info.gossip.refresh_push_active_set(&HashMap::new());
        //check that all types of gossip messages are signed correctly
        let (_, push_messages) = cluster_info.gossip.new_push_messages(timestamp());
        // there should be some pushes ready
        assert_eq!(push_messages.len() > 0, true);
        push_messages
            .values()
            .for_each(|v| v.par_iter().for_each(|v| assert!(v.verify())));

        let (_, _, val) = cluster_info
            .gossip
            .new_pull_request(timestamp(), &HashMap::new(), MAX_BLOOM_SIZE)
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
        let now = timestamp();
        let contact_info = ContactInfo::new_localhost(&keys.pubkey(), 0);
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);

        // make sure empty crds is handled correctly
        let (votes, max_ts) = cluster_info.get_votes(now);
        assert_eq!(votes, vec![]);
        assert_eq!(max_ts, now);

        // add a vote
        let tx = test_tx();
        cluster_info.push_vote(0, tx.clone());

        // -1 to make sure that the clock is strictly lower then when insert occurred
        let (votes, max_ts) = cluster_info.get_votes(now - 1);
        assert_eq!(votes, vec![tx]);
        assert!(max_ts >= now - 1);

        // make sure timestamp filter works
        let (votes, new_max_ts) = cluster_info.get_votes(max_ts);
        assert_eq!(votes, vec![]);
        assert_eq!(max_ts, new_max_ts);
    }

    #[test]
    fn test_push_epoch_slots() {
        let keys = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keys.pubkey(), 0);
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        assert!(slots.is_empty());
        assert!(since.is_none());
        cluster_info.push_epoch_slots(&[0]);

        let (slots, since) = cluster_info.get_epoch_slots_since(Some(std::u64::MAX));
        assert!(slots.is_empty());
        assert_eq!(since, Some(std::u64::MAX));

        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        assert_eq!(slots.len(), 1);
        assert!(since.is_some());

        let (slots, since2) = cluster_info.get_epoch_slots_since(since.clone());
        assert!(slots.is_empty());
        assert_eq!(since2, since);
    }

    #[test]
    fn test_add_entrypoint() {
        let node_keypair = Arc::new(Keypair::new());
        let mut cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        let entrypoint_pubkey = Pubkey::new_rand();
        let entrypoint = ContactInfo::new_localhost(&entrypoint_pubkey, timestamp());
        cluster_info.set_entrypoint(entrypoint.clone());
        let pulls = cluster_info.new_pull_requests(&HashMap::new());
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
        let cluster_info = Arc::new(RwLock::new(cluster_info));
        let timeouts = cluster_info.read().unwrap().gossip.make_timeouts_test();
        ClusterInfo::handle_pull_response(
            &cluster_info,
            &entrypoint_pubkey,
            vec![entrypoint_crdsvalue],
            &timeouts,
        );
        let pulls = cluster_info
            .write()
            .unwrap()
            .new_pull_requests(&HashMap::new());
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(cluster_info.read().unwrap().entrypoint, Some(entrypoint));
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
    fn test_split_messages_packet_size() {
        // Test that if a value is smaller than payload size but too large to be wrapped in a vec
        // that it is still dropped
        let payload: Vec<CrdsValue> = vec![];
        let vec_size = serialized_size(&payload).unwrap();
        let desired_size = MAX_PROTOCOL_PAYLOAD_SIZE - vec_size;
        let mut value = CrdsValue::new_unsigned(CrdsData::SnapshotHash(SnapshotHash {
            from: Pubkey::default(),
            hashes: vec![],
            wallclock: 0,
        }));

        let mut i = 0;
        while value.size() <= desired_size {
            value.data = CrdsData::SnapshotHash(SnapshotHash {
                from: Pubkey::default(),
                hashes: vec![(0, Hash::default()); i],
                wallclock: 0,
            });
            i += 1;
        }
        let split = ClusterInfo::split_gossip_messages(vec![value.clone()]);
        assert_eq!(split.len(), 0);
    }

    fn test_split_messages(value: CrdsValue) {
        const NUM_VALUES: u64 = 30;
        let value_size = value.size();
        let num_values_per_payload = (MAX_PROTOCOL_PAYLOAD_SIZE / value_size).max(1);

        // Expected len is the ceiling of the division
        let expected_len = (NUM_VALUES + num_values_per_payload - 1) / num_values_per_payload;
        let msgs = vec![value; NUM_VALUES as usize];

        let split = ClusterInfo::split_gossip_messages(msgs);
        assert!(split.len() as u64 <= expected_len);
    }

    #[test]
    fn test_crds_filter_size() {
        //sanity test to ensure filter size never exceeds MTU size
        check_pull_request_size(CrdsFilter::new_rand(1000, 10));
        check_pull_request_size(CrdsFilter::new_rand(1000, 1000));
        check_pull_request_size(CrdsFilter::new_rand(100000, 1000));
        check_pull_request_size(CrdsFilter::new_rand(100000, MAX_BLOOM_SIZE));
    }

    fn check_pull_request_size(filter: CrdsFilter) {
        let value = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        let protocol = Protocol::PullRequest(filter, value.clone());
        assert!(serialized_size(&protocol).unwrap() <= PACKET_DATA_SIZE as u64);
    }

    #[test]
    fn test_tvu_peers_and_stakes() {
        let d = ContactInfo::new_localhost(&Pubkey::new(&[0; 32]), timestamp());
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(d.clone());
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
        cluster_info.insert_info(contact_info.clone());
        stakes.insert(id4, 10);

        let stakes = Arc::new(stakes);
        let (peers, peers_and_stakes) = cluster_info.sorted_tvu_peers_and_stakes(Some(stakes));
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].id, id);
        assert_eq!(peers[1].id, id2);
        assert_eq!(peers_and_stakes.len(), 2);
        assert_eq!(peers_and_stakes[0].0, 10);
        assert_eq!(peers_and_stakes[1].0, 1);
    }

    #[test]
    fn test_pull_from_entrypoint_if_not_present() {
        let node_keypair = Arc::new(Keypair::new());
        let mut cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        let entrypoint_pubkey = Pubkey::new_rand();
        let mut entrypoint = ContactInfo::new_localhost(&entrypoint_pubkey, timestamp());
        entrypoint.gossip = socketaddr!("127.0.0.2:1234");
        cluster_info.set_entrypoint(entrypoint.clone());

        let mut stakes = HashMap::new();

        let other_node_pubkey = Pubkey::new_rand();
        let other_node = ContactInfo::new_localhost(&other_node_pubkey, timestamp());
        assert_ne!(other_node.gossip, entrypoint.gossip);
        cluster_info.insert_info(other_node.clone());
        stakes.insert(other_node_pubkey, 10);

        // Pull request 1:  `other_node` is present but `entrypoint` was just added (so it has a
        // fresh timestamp).  There should only be one pull request to `other_node`
        let pulls = cluster_info.new_pull_requests(&stakes);
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);

        // Pull request 2: pretend it's been a while since we've pulled from `entrypoint`.  There should
        // now be two pull requests
        cluster_info.entrypoint.as_mut().unwrap().wallclock = 0;
        let pulls = cluster_info.new_pull_requests(&stakes);
        assert_eq!(2, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);
        assert_eq!(pulls.get(1).unwrap().0, entrypoint.gossip);

        // Pull request 3:  `other_node` is present and `entrypoint` was just pulled from.  There should
        // only be one pull request to `other_node`
        let pulls = cluster_info.new_pull_requests(&stakes);
        assert_eq!(1, pulls.len() as u64);
        assert_eq!(pulls.get(0).unwrap().0, other_node.gossip);
    }

    #[test]
    fn test_repair_peers() {
        let node_keypair = Arc::new(Keypair::new());
        let mut cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
        );
        for i in 0..10 {
            let mut peer_lowest = 0;
            if i >= 5 {
                // make these invalid for the upcoming repair request
                peer_lowest = 10;
            }
            let other_node_pubkey = Pubkey::new_rand();
            let other_node = ContactInfo::new_localhost(&other_node_pubkey, timestamp());
            cluster_info.insert_info(other_node.clone());
            let value = CrdsValue::new_unsigned(CrdsData::LowestSlot(
                0,
                LowestSlot::new(other_node_pubkey, peer_lowest, timestamp()),
            ));
            let _ = cluster_info.gossip.crds.insert(value, timestamp());
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
    fn test_protocol_size() {
        let contact_info = CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default()));
        let dummy_vec =
            vec![CrdsValue::new_unsigned(CrdsData::ContactInfo(ContactInfo::default())); 10];
        let dummy_vec_size = serialized_size(&dummy_vec).unwrap();
        let mut max_protocol_size;

        max_protocol_size =
            serialized_size(&Protocol::PullRequest(CrdsFilter::default(), contact_info)).unwrap()
                - serialized_size(&CrdsFilter::default()).unwrap();
        max_protocol_size = max_protocol_size.max(
            serialized_size(&Protocol::PullResponse(
                Pubkey::default(),
                dummy_vec.clone(),
            ))
            .unwrap()
                - dummy_vec_size,
        );
        max_protocol_size = max_protocol_size.max(
            serialized_size(&Protocol::PushMessage(Pubkey::default(), dummy_vec)).unwrap()
                - dummy_vec_size,
        );
        max_protocol_size = max_protocol_size.max(
            serialized_size(&Protocol::PruneMessage(
                Pubkey::default(),
                PruneData::default(),
            ))
            .unwrap()
                - serialized_size(&PruneData::default()).unwrap(),
        );

        // finally assert the header size estimation is correct
        assert_eq!(MAX_PROTOCOL_HEADER_SIZE, max_protocol_size);
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
    fn test_push_epoch_slots_large() {
        use rand::Rng;
        let node_keypair = Arc::new(Keypair::new());
        let mut cluster_info = ClusterInfo::new(
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
        cluster_info.push_epoch_slots(&range[16000..]);
        let (slots, since) = cluster_info.get_epoch_slots_since(None);
        let slots: Vec<_> = slots.iter().flat_map(|x| x.to_slots(0)).collect();
        assert_eq!(slots, range);
        assert!(since.is_some());
    }
}
