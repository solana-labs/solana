use {
    crate::{
        cluster_slots_service::cluster_slots::ClusterSlots,
        repair::{
            duplicate_repair_status::get_ancestor_hash_repair_sample_size,
            quic_endpoint::{LocalRequest, RemoteRequest},
            repair_response,
            repair_service::{OutstandingShredRepairs, RepairStats, REPAIR_MS},
            request_response::RequestResponse,
            result::{Error, RepairVerifyError, Result},
        },
    },
    bincode::{serialize, Options},
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    lru::LruCache,
    rand::{
        distributions::{Distribution, WeightedError, WeightedIndex},
        Rng,
    },
    solana_gossip::{
        cluster_info::{ClusterInfo, ClusterInfoError},
        contact_info::{LegacyContactInfo as ContactInfo, LegacyContactInfo, Protocol},
        ping_pong::{self, PingCache, Pong},
        weighted_shuffle::WeightedShuffle,
    },
    solana_ledger::{
        ancestor_iterator::{AncestorIterator, AncestorIteratorWithHash},
        blockstore::Blockstore,
        shred::{Nonce, Shred, ShredFetchStats, SIZE_OF_NONCE},
    },
    solana_perf::{
        data_budget::DataBudget,
        packet::{Packet, PacketBatch, PacketBatchRecycler},
    },
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{
        clock::Slot,
        genesis_config::ClusterType,
        hash::{Hash, HASH_BYTES},
        packet::PACKET_DATA_SIZE,
        pubkey::{Pubkey, PUBKEY_BYTES},
        signature::{Signable, Signature, Signer, SIGNATURE_BYTES},
        signer::keypair::Keypair,
        timing::{duration_as_ms, timestamp},
    },
    solana_streamer::{
        sendmmsg::{batch_send, SendPktsError},
        socket::SocketAddrSpace,
        streamer::PacketBatchSender,
    },
    std::{
        cmp::Reverse,
        collections::{HashMap, HashSet},
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{Builder, JoinHandle},
        time::{Duration, Instant},
    },
    tokio::sync::{mpsc::Sender as AsyncSender, oneshot::Sender as OneShotSender},
};

/// the number of slots to respond with when responding to `Orphan` requests
pub const MAX_ORPHAN_REPAIR_RESPONSES: usize = 11;
// Number of slots to cache their respective repair peers and sampling weights.
pub(crate) const REPAIR_PEERS_CACHE_CAPACITY: usize = 128;
// Limit cache entries ttl in order to avoid re-using outdated data.
const REPAIR_PEERS_CACHE_TTL: Duration = Duration::from_secs(10);

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_ANCESTOR_BYTES_IN_PACKET, 1220);
pub const MAX_ANCESTOR_BYTES_IN_PACKET: usize =
    PACKET_DATA_SIZE -
    SIZE_OF_NONCE -
    4 /*(response version enum discriminator)*/ -
    4 /*slot_hash length*/;

pub const MAX_ANCESTOR_RESPONSES: usize =
    MAX_ANCESTOR_BYTES_IN_PACKET / std::mem::size_of::<(Slot, Hash)>();
/// Number of bytes in the randomly generated token sent with ping messages.
pub(crate) const REPAIR_PING_TOKEN_SIZE: usize = HASH_BYTES;
pub const REPAIR_PING_CACHE_CAPACITY: usize = 65536;
pub const REPAIR_PING_CACHE_TTL: Duration = Duration::from_secs(1280);
const REPAIR_PING_CACHE_RATE_LIMIT_DELAY: Duration = Duration::from_secs(2);
pub(crate) const REPAIR_RESPONSE_SERIALIZED_PING_BYTES: usize =
    4 /*enum discriminator*/ + PUBKEY_BYTES + REPAIR_PING_TOKEN_SIZE + SIGNATURE_BYTES;
const SIGNED_REPAIR_TIME_WINDOW: Duration = Duration::from_secs(60 * 10); // 10 min

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_ANCESTOR_RESPONSES, 30);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum ShredRepairType {
    /// Requesting `MAX_ORPHAN_REPAIR_RESPONSES ` parent shreds
    Orphan(Slot),
    /// Requesting any shred with index greater than or equal to the particular index
    HighestShred(Slot, u64),
    /// Requesting the missing shred at a particular index
    Shred(Slot, u64),
}

impl ShredRepairType {
    pub fn slot(&self) -> Slot {
        match self {
            ShredRepairType::Orphan(slot)
            | ShredRepairType::HighestShred(slot, _)
            | ShredRepairType::Shred(slot, _) => *slot,
        }
    }
}

impl RequestResponse for ShredRepairType {
    type Response = Shred;
    fn num_expected_responses(&self) -> u32 {
        match self {
            ShredRepairType::Orphan(_) => MAX_ORPHAN_REPAIR_RESPONSES as u32,
            ShredRepairType::Shred(_, _) | ShredRepairType::HighestShred(_, _) => 1,
        }
    }
    fn verify_response(&self, response_shred: &Shred) -> bool {
        match self {
            ShredRepairType::Orphan(slot) => response_shred.slot() <= *slot,
            ShredRepairType::HighestShred(slot, index) => {
                response_shred.slot() == *slot && response_shred.index() as u64 >= *index
            }
            ShredRepairType::Shred(slot, index) => {
                response_shred.slot() == *slot && response_shred.index() as u64 == *index
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct AncestorHashesRepairType(pub Slot);
impl AncestorHashesRepairType {
    pub fn slot(&self) -> Slot {
        self.0
    }
}

#[derive(Debug, AbiEnumVisitor, AbiExample, Deserialize, Serialize)]
#[frozen_abi(digest = "AKpurCovzn6rsji4aQrP3hUdEHxjtXUfT7AatZXN7Rpz")]
pub enum AncestorHashesResponse {
    Hashes(Vec<(Slot, Hash)>),
    Ping(Ping),
}

impl RequestResponse for AncestorHashesRepairType {
    type Response = AncestorHashesResponse;
    fn num_expected_responses(&self) -> u32 {
        1
    }
    fn verify_response(&self, response: &AncestorHashesResponse) -> bool {
        match response {
            AncestorHashesResponse::Hashes(hashes) => hashes.len() <= MAX_ANCESTOR_RESPONSES,
            AncestorHashesResponse::Ping(ping) => ping.verify(),
        }
    }
}

#[derive(Default)]
struct ServeRepairStats {
    total_requests: usize,
    dropped_requests_outbound_bandwidth: usize,
    dropped_requests_load_shed: usize,
    dropped_requests_low_stake: usize,
    whitelisted_requests: usize,
    total_dropped_response_packets: usize,
    total_response_packets: usize,
    total_response_bytes_staked: usize,
    total_response_bytes_unstaked: usize,
    processed: usize,
    window_index: usize,
    highest_window_index: usize,
    orphan: usize,
    pong: usize,
    ancestor_hashes: usize,
    window_index_misses: usize,
    ping_cache_check_failed: usize,
    pings_sent: usize,
    decode_time_us: u64,
    handle_requests_time_us: u64,
    handle_requests_staked: usize,
    handle_requests_unstaked: usize,
    err_self_repair: usize,
    err_time_skew: usize,
    err_malformed: usize,
    err_sig_verify: usize,
    err_unsigned: usize,
    err_id_mismatch: usize,
}

#[derive(Debug, AbiExample, Deserialize, Serialize)]
pub struct RepairRequestHeader {
    signature: Signature,
    sender: Pubkey,
    recipient: Pubkey,
    timestamp: u64,
    nonce: Nonce,
}

impl RepairRequestHeader {
    pub fn new(sender: Pubkey, recipient: Pubkey, timestamp: u64, nonce: Nonce) -> Self {
        Self {
            signature: Signature::default(),
            sender,
            recipient,
            timestamp,
            nonce,
        }
    }
}

pub(crate) type Ping = ping_pong::Ping<[u8; REPAIR_PING_TOKEN_SIZE]>;

/// Window protocol messages
#[derive(Debug, AbiEnumVisitor, AbiExample, Deserialize, Serialize)]
#[frozen_abi(digest = "3VzVe3kMrG6ijkVPyCGeJVA9hQjWcFEZbAQPc5Zizrjm")]
pub enum RepairProtocol {
    LegacyWindowIndex(LegacyContactInfo, Slot, u64),
    LegacyHighestWindowIndex(LegacyContactInfo, Slot, u64),
    LegacyOrphan(LegacyContactInfo, Slot),
    LegacyWindowIndexWithNonce(LegacyContactInfo, Slot, u64, Nonce),
    LegacyHighestWindowIndexWithNonce(LegacyContactInfo, Slot, u64, Nonce),
    LegacyOrphanWithNonce(LegacyContactInfo, Slot, Nonce),
    LegacyAncestorHashes(LegacyContactInfo, Slot, Nonce),
    Pong(ping_pong::Pong),
    WindowIndex {
        header: RepairRequestHeader,
        slot: Slot,
        shred_index: u64,
    },
    HighestWindowIndex {
        header: RepairRequestHeader,
        slot: Slot,
        shred_index: u64,
    },
    Orphan {
        header: RepairRequestHeader,
        slot: Slot,
    },
    AncestorHashes {
        header: RepairRequestHeader,
        slot: Slot,
    },
}

const REPAIR_REQUEST_PONG_SERIALIZED_BYTES: usize = PUBKEY_BYTES + HASH_BYTES + SIGNATURE_BYTES;
const REPAIR_REQUEST_MIN_BYTES: usize = REPAIR_REQUEST_PONG_SERIALIZED_BYTES;

fn discard_malformed_repair_requests(
    requests: &mut Vec<RemoteRequest>,
    stats: &mut ServeRepairStats,
) -> usize {
    let num_requests = requests.len();
    requests.retain(|request| request.bytes.len() >= REPAIR_REQUEST_MIN_BYTES);
    stats.err_malformed += num_requests - requests.len();
    requests.len()
}

#[derive(Debug, AbiEnumVisitor, AbiExample, Deserialize, Serialize)]
#[frozen_abi(digest = "CkffjyMPCwuJgk9NiCMELXLCecAnTPZqpKEnUCb3VyVf")]
pub(crate) enum RepairResponse {
    Ping(Ping),
}

impl RepairProtocol {
    fn sender(&self) -> &Pubkey {
        match self {
            Self::LegacyWindowIndex(ci, _, _) => ci.pubkey(),
            Self::LegacyHighestWindowIndex(ci, _, _) => ci.pubkey(),
            Self::LegacyOrphan(ci, _) => ci.pubkey(),
            Self::LegacyWindowIndexWithNonce(ci, _, _, _) => ci.pubkey(),
            Self::LegacyHighestWindowIndexWithNonce(ci, _, _, _) => ci.pubkey(),
            Self::LegacyOrphanWithNonce(ci, _, _) => ci.pubkey(),
            Self::LegacyAncestorHashes(ci, _, _) => ci.pubkey(),
            Self::Pong(pong) => pong.from(),
            Self::WindowIndex { header, .. } => &header.sender,
            Self::HighestWindowIndex { header, .. } => &header.sender,
            Self::Orphan { header, .. } => &header.sender,
            Self::AncestorHashes { header, .. } => &header.sender,
        }
    }

    fn supports_signature(&self) -> bool {
        match self {
            Self::LegacyWindowIndex(_, _, _)
            | Self::LegacyHighestWindowIndex(_, _, _)
            | Self::LegacyOrphan(_, _)
            | Self::LegacyWindowIndexWithNonce(_, _, _, _)
            | Self::LegacyHighestWindowIndexWithNonce(_, _, _, _)
            | Self::LegacyOrphanWithNonce(_, _, _)
            | Self::LegacyAncestorHashes(_, _, _) => false,
            Self::Pong(_)
            | Self::WindowIndex { .. }
            | Self::HighestWindowIndex { .. }
            | Self::Orphan { .. }
            | Self::AncestorHashes { .. } => true,
        }
    }

    fn max_response_packets(&self) -> usize {
        match self {
            RepairProtocol::WindowIndex { .. }
            | RepairProtocol::HighestWindowIndex { .. }
            | RepairProtocol::AncestorHashes { .. } => 1,
            RepairProtocol::Orphan { .. } => MAX_ORPHAN_REPAIR_RESPONSES,
            RepairProtocol::Pong(_) => 0, // no response
            RepairProtocol::LegacyWindowIndex(_, _, _)
            | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
            | RepairProtocol::LegacyOrphan(_, _)
            | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
            | RepairProtocol::LegacyAncestorHashes(_, _, _) => 0, // unsupported
        }
    }

    fn max_response_bytes(&self) -> usize {
        self.max_response_packets() * PACKET_DATA_SIZE
    }
}

#[derive(Clone)]
pub struct ServeRepair {
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,
}

// Cache entry for repair peers for a slot.
pub(crate) struct RepairPeers {
    asof: Instant,
    peers: Vec<Node>,
    weighted_index: WeightedIndex<u64>,
}

struct Node {
    pubkey: Pubkey,
    serve_repair: SocketAddr,
    serve_repair_quic: SocketAddr,
}

impl RepairPeers {
    fn new(asof: Instant, peers: &[ContactInfo], weights: &[u64]) -> Result<Self> {
        if peers.len() != weights.len() {
            return Err(Error::from(WeightedError::InvalidWeight));
        }
        let (peers, weights): (Vec<_>, Vec<u64>) = peers
            .iter()
            .zip(weights)
            .filter_map(|(peer, &weight)| {
                let node = Node {
                    pubkey: *peer.pubkey(),
                    serve_repair: peer.serve_repair(Protocol::UDP).ok()?,
                    serve_repair_quic: peer.serve_repair(Protocol::QUIC).ok()?,
                };
                Some((node, weight))
            })
            .unzip();
        if peers.is_empty() {
            return Err(Error::from(ClusterInfoError::NoPeers));
        }
        let weighted_index = WeightedIndex::new(weights)?;
        Ok(Self {
            asof,
            peers,
            weighted_index,
        })
    }

    fn sample<R: Rng>(&self, rng: &mut R) -> &Node {
        let index = self.weighted_index.sample(rng);
        &self.peers[index]
    }
}

struct RepairRequestWithMeta {
    request: RepairProtocol,
    from_addr: SocketAddr,
    stake: u64,
    whitelisted: bool,
    response_sender: Option<OneShotSender<Vec<Vec<u8>>>>,
}

impl ServeRepair {
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        bank_forks: Arc<RwLock<BankForks>>,
        repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,
    ) -> Self {
        Self {
            cluster_info,
            bank_forks,
            repair_whitelist,
        }
    }

    pub(crate) fn my_id(&self) -> Pubkey {
        self.cluster_info.id()
    }

    fn handle_repair(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: &Blockstore,
        request: RepairProtocol,
        stats: &mut ServeRepairStats,
        ping_cache: &mut PingCache,
    ) -> Option<PacketBatch> {
        let now = Instant::now();
        let (res, label) = {
            match &request {
                RepairProtocol::WindowIndex {
                    header: RepairRequestHeader { nonce, .. },
                    slot,
                    shred_index,
                } => {
                    stats.window_index += 1;
                    let batch = Self::run_window_request(
                        recycler,
                        from_addr,
                        blockstore,
                        *slot,
                        *shred_index,
                        *nonce,
                    );
                    if batch.is_none() {
                        stats.window_index_misses += 1;
                    }
                    (batch, "WindowIndexWithNonce")
                }
                RepairProtocol::HighestWindowIndex {
                    header: RepairRequestHeader { nonce, .. },
                    slot,
                    shred_index: highest_index,
                } => {
                    stats.highest_window_index += 1;
                    (
                        Self::run_highest_window_request(
                            recycler,
                            from_addr,
                            blockstore,
                            *slot,
                            *highest_index,
                            *nonce,
                        ),
                        "HighestWindowIndexWithNonce",
                    )
                }
                RepairProtocol::Orphan {
                    header: RepairRequestHeader { nonce, .. },
                    slot,
                } => {
                    stats.orphan += 1;
                    (
                        Self::run_orphan(
                            recycler,
                            from_addr,
                            blockstore,
                            *slot,
                            MAX_ORPHAN_REPAIR_RESPONSES,
                            *nonce,
                        ),
                        "OrphanWithNonce",
                    )
                }
                RepairProtocol::AncestorHashes {
                    header: RepairRequestHeader { nonce, .. },
                    slot,
                } => {
                    stats.ancestor_hashes += 1;
                    (
                        Self::run_ancestor_hashes(recycler, from_addr, blockstore, *slot, *nonce),
                        "AncestorHashes",
                    )
                }
                RepairProtocol::Pong(pong) => {
                    stats.pong += 1;
                    ping_cache.add(pong, *from_addr, Instant::now());
                    (None, "Pong")
                }
                RepairProtocol::LegacyWindowIndex(_, _, _)
                | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
                | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
                | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
                | RepairProtocol::LegacyOrphan(_, _)
                | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
                | RepairProtocol::LegacyAncestorHashes(_, _, _) => {
                    error!("Unexpected legacy request: {request:?}");
                    debug_assert!(
                        false,
                        "Legacy requests should have been filtered out during signature
                        verification. {request:?}"
                    );
                    (None, "Legacy")
                }
            }
        };
        Self::report_time_spent(label, &now.elapsed(), "");
        res
    }

    fn report_time_spent(label: &str, time: &Duration, extra: &str) {
        let count = duration_as_ms(time);
        if count > 5 {
            info!("{} took: {} ms {}", label, count, extra);
        }
    }

    fn decode_request(
        remote_request: RemoteRequest,
        epoch_staked_nodes: &Option<Arc<HashMap<Pubkey, u64>>>,
        whitelist: &HashSet<Pubkey>,
        my_id: &Pubkey,
        socket_addr_space: &SocketAddrSpace,
    ) -> Result<RepairRequestWithMeta> {
        let Ok(request) = deserialize_request::<RepairProtocol>(&remote_request) else {
            return Err(Error::from(RepairVerifyError::Malformed));
        };
        let from_addr = remote_request.remote_address;
        if !ContactInfo::is_valid_address(&from_addr, socket_addr_space) {
            return Err(Error::from(RepairVerifyError::Malformed));
        }
        Self::verify_signed_packet(my_id, &remote_request.bytes, &request)?;
        if let Some(remote_pubkey) = remote_request.remote_pubkey {
            if &remote_pubkey != request.sender() {
                error!(
                    "remote pubkey {remote_pubkey} != request sender {}",
                    request.sender()
                );
            }
        }
        if request.sender() == my_id {
            error!("self repair: from_addr={from_addr} my_id={my_id} request={request:?}");
            return Err(Error::from(RepairVerifyError::SelfRepair));
        }
        let stake = *epoch_staked_nodes
            .as_ref()
            .and_then(|stakes| stakes.get(request.sender()))
            .unwrap_or(&0);

        let whitelisted = whitelist.contains(request.sender());

        Ok(RepairRequestWithMeta {
            request,
            from_addr,
            stake,
            whitelisted,
            response_sender: remote_request.response_sender,
        })
    }

    fn record_request_decode_error(error: &Error, stats: &mut ServeRepairStats) {
        match error {
            Error::RepairVerify(RepairVerifyError::IdMismatch) => {
                stats.err_id_mismatch += 1;
            }
            Error::RepairVerify(RepairVerifyError::Malformed) => {
                stats.err_malformed += 1;
            }
            Error::RepairVerify(RepairVerifyError::SelfRepair) => {
                stats.err_self_repair += 1;
            }
            Error::RepairVerify(RepairVerifyError::SigVerify) => {
                stats.err_sig_verify += 1;
            }
            Error::RepairVerify(RepairVerifyError::TimeSkew) => {
                stats.err_time_skew += 1;
            }
            Error::RepairVerify(RepairVerifyError::Unsigned) => {
                stats.err_unsigned += 1;
            }
            _ => {
                debug_assert!(false, "unhandled error {error:?}");
            }
        }
    }

    fn decode_requests(
        requests: Vec<RemoteRequest>,
        epoch_staked_nodes: &Option<Arc<HashMap<Pubkey, u64>>>,
        whitelist: &HashSet<Pubkey>,
        my_id: &Pubkey,
        socket_addr_space: &SocketAddrSpace,
        stats: &mut ServeRepairStats,
    ) -> Vec<RepairRequestWithMeta> {
        let decode_request = |request| {
            let result = Self::decode_request(
                request,
                epoch_staked_nodes,
                whitelist,
                my_id,
                socket_addr_space,
            );
            match &result {
                Ok(req) => {
                    if req.stake == 0 {
                        stats.handle_requests_unstaked += 1;
                    } else {
                        stats.handle_requests_staked += 1;
                    }
                }
                Err(e) => {
                    Self::record_request_decode_error(e, stats);
                }
            }
            result.ok()
        };
        requests.into_iter().filter_map(decode_request).collect()
    }

    /// Process messages from the network
    fn run_listen(
        &self,
        ping_cache: &mut PingCache,
        recycler: &PacketBatchRecycler,
        blockstore: &Blockstore,
        requests_receiver: &Receiver<RemoteRequest>,
        response_sender: &PacketBatchSender,
        stats: &mut ServeRepairStats,
        data_budget: &DataBudget,
    ) -> std::result::Result<(), RecvTimeoutError> {
        const TIMEOUT: Duration = Duration::from_secs(1);
        let mut requests = vec![requests_receiver.recv_timeout(TIMEOUT)?];
        const MAX_REQUESTS_PER_ITERATION: usize = 1024;
        let mut total_requests = requests.len();

        let socket_addr_space = *self.cluster_info.socket_addr_space();
        let root_bank = self.bank_forks.read().unwrap().root_bank();
        let epoch_staked_nodes = root_bank.epoch_staked_nodes(root_bank.epoch());
        let identity_keypair = self.cluster_info.keypair().clone();
        let my_id = identity_keypair.pubkey();

        let max_buffered_packets = if self.repair_whitelist.read().unwrap().len() > 0 {
            4 * MAX_REQUESTS_PER_ITERATION
        } else {
            2 * MAX_REQUESTS_PER_ITERATION
        };

        let mut dropped_requests = 0;
        let mut well_formed_requests = discard_malformed_repair_requests(&mut requests, stats);
        loop {
            let mut more: Vec<_> = requests_receiver.try_iter().collect();
            if more.is_empty() {
                break;
            }
            total_requests += more.len();
            if well_formed_requests > max_buffered_packets {
                // Already exceeded max. Don't waste time discarding
                dropped_requests += more.len();
                continue;
            }
            let retained = discard_malformed_repair_requests(&mut more, stats);
            well_formed_requests += retained;
            if retained > 0 && well_formed_requests <= max_buffered_packets {
                requests.extend(more);
            } else {
                dropped_requests += more.len();
            }
        }

        stats.dropped_requests_load_shed += dropped_requests;
        stats.total_requests += total_requests;

        let decode_start = Instant::now();
        let mut decoded_requests = {
            let whitelist = self.repair_whitelist.read().unwrap();
            Self::decode_requests(
                requests,
                &epoch_staked_nodes,
                &whitelist,
                &my_id,
                &socket_addr_space,
                stats,
            )
        };
        let whitelisted_request_count = decoded_requests.iter().filter(|r| r.whitelisted).count();
        stats.decode_time_us += decode_start.elapsed().as_micros() as u64;
        stats.whitelisted_requests += whitelisted_request_count.min(MAX_REQUESTS_PER_ITERATION);

        if decoded_requests.len() > MAX_REQUESTS_PER_ITERATION {
            stats.dropped_requests_low_stake += decoded_requests.len() - MAX_REQUESTS_PER_ITERATION;
            decoded_requests.sort_unstable_by_key(|r| Reverse((r.whitelisted, r.stake)));
            decoded_requests.truncate(MAX_REQUESTS_PER_ITERATION);
        }

        let handle_requests_start = Instant::now();
        self.handle_requests(
            ping_cache,
            recycler,
            blockstore,
            decoded_requests,
            response_sender,
            stats,
            data_budget,
        );
        stats.handle_requests_time_us += handle_requests_start.elapsed().as_micros() as u64;

        Ok(())
    }

    fn report_reset_stats(&self, stats: &mut ServeRepairStats) {
        if stats.err_self_repair > 0 {
            let my_id = self.cluster_info.id();
            warn!(
                "{}: Ignored received repair requests from ME: {}",
                my_id, stats.err_self_repair,
            );
        }

        datapoint_info!(
            "serve_repair-requests_received",
            ("total_requests", stats.total_requests, i64),
            (
                "dropped_requests_outbound_bandwidth",
                stats.dropped_requests_outbound_bandwidth,
                i64
            ),
            (
                "dropped_requests_load_shed",
                stats.dropped_requests_load_shed,
                i64
            ),
            (
                "dropped_requests_low_stake",
                stats.dropped_requests_low_stake,
                i64
            ),
            ("whitelisted_requests", stats.whitelisted_requests, i64),
            (
                "total_dropped_response_packets",
                stats.total_dropped_response_packets,
                i64
            ),
            ("handle_requests_staked", stats.handle_requests_staked, i64),
            (
                "handle_requests_unstaked",
                stats.handle_requests_unstaked,
                i64
            ),
            ("processed", stats.processed, i64),
            ("total_response_packets", stats.total_response_packets, i64),
            (
                "total_response_bytes_staked",
                stats.total_response_bytes_staked,
                i64
            ),
            (
                "total_response_bytes_unstaked",
                stats.total_response_bytes_unstaked,
                i64
            ),
            ("self_repair", stats.err_self_repair, i64),
            ("window_index", stats.window_index, i64),
            (
                "request-highest-window-index",
                stats.highest_window_index,
                i64
            ),
            ("orphan", stats.orphan, i64),
            (
                "serve_repair-request-ancestor-hashes",
                stats.ancestor_hashes,
                i64
            ),
            ("pong", stats.pong, i64),
            ("window_index_misses", stats.window_index_misses, i64),
            (
                "ping_cache_check_failed",
                stats.ping_cache_check_failed,
                i64
            ),
            ("pings_sent", stats.pings_sent, i64),
            ("decode_time_us", stats.decode_time_us, i64),
            (
                "handle_requests_time_us",
                stats.handle_requests_time_us,
                i64
            ),
            ("err_time_skew", stats.err_time_skew, i64),
            ("err_malformed", stats.err_malformed, i64),
            ("err_sig_verify", stats.err_sig_verify, i64),
            ("err_unsigned", stats.err_unsigned, i64),
            ("err_id_mismatch", stats.err_id_mismatch, i64),
        );

        *stats = ServeRepairStats::default();
    }

    pub fn listen(
        self,
        blockstore: Arc<Blockstore>,
        requests_receiver: Receiver<RemoteRequest>,
        response_sender: PacketBatchSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        const INTERVAL_MS: u64 = 1000;
        const MAX_BYTES_PER_SECOND: usize = 12_000_000;
        const MAX_BYTES_PER_INTERVAL: usize = MAX_BYTES_PER_SECOND * INTERVAL_MS as usize / 1000;

        // rate limit delay should be greater than the repair request iteration delay
        assert!(REPAIR_PING_CACHE_RATE_LIMIT_DELAY > Duration::from_millis(REPAIR_MS));

        let mut ping_cache = PingCache::new(
            REPAIR_PING_CACHE_TTL,
            REPAIR_PING_CACHE_RATE_LIMIT_DELAY,
            REPAIR_PING_CACHE_CAPACITY,
        );

        let recycler = PacketBatchRecycler::default();
        Builder::new()
            .name("solRepairListen".to_string())
            .spawn(move || {
                let mut last_print = Instant::now();
                let mut stats = ServeRepairStats::default();
                let data_budget = DataBudget::default();
                while !exit.load(Ordering::Relaxed) {
                    let result = self.run_listen(
                        &mut ping_cache,
                        &recycler,
                        &blockstore,
                        &requests_receiver,
                        &response_sender,
                        &mut stats,
                        &data_budget,
                    );
                    match result {
                        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            info!("repair listener disconnected");
                            return;
                        }
                    };
                    if last_print.elapsed().as_secs() > 2 {
                        self.report_reset_stats(&mut stats);
                        last_print = Instant::now();
                    }
                    data_budget.update(INTERVAL_MS, |_bytes| MAX_BYTES_PER_INTERVAL);
                }
            })
            .unwrap()
    }

    fn verify_signed_packet(my_id: &Pubkey, bytes: &[u8], request: &RepairProtocol) -> Result<()> {
        match request {
            RepairProtocol::LegacyWindowIndex(_, _, _)
            | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
            | RepairProtocol::LegacyOrphan(_, _)
            | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
            | RepairProtocol::LegacyAncestorHashes(_, _, _) => {
                return Err(Error::from(RepairVerifyError::Unsigned));
            }
            RepairProtocol::Pong(pong) => {
                if !pong.verify() {
                    return Err(Error::from(RepairVerifyError::SigVerify));
                }
            }
            RepairProtocol::WindowIndex { header, .. }
            | RepairProtocol::HighestWindowIndex { header, .. }
            | RepairProtocol::Orphan { header, .. }
            | RepairProtocol::AncestorHashes { header, .. } => {
                if &header.recipient != my_id {
                    return Err(Error::from(RepairVerifyError::IdMismatch));
                }
                let time_diff_ms = timestamp().abs_diff(header.timestamp);
                if u128::from(time_diff_ms) > SIGNED_REPAIR_TIME_WINDOW.as_millis() {
                    return Err(Error::from(RepairVerifyError::TimeSkew));
                }
                let Some(leading_buf) = bytes.get(..4) else {
                    debug_assert!(
                        false,
                        "request should have failed deserialization: {request:?}",
                    );
                    return Err(Error::from(RepairVerifyError::Malformed));
                };
                let Some(trailing_buf) = bytes.get(4 + SIGNATURE_BYTES..) else {
                    debug_assert!(
                        false,
                        "request should have failed deserialization: {request:?}",
                    );
                    return Err(Error::from(RepairVerifyError::Malformed));
                };
                let from_id = request.sender();
                let signed_data = [leading_buf, trailing_buf].concat();
                if !header.signature.verify(from_id.as_ref(), &signed_data) {
                    return Err(Error::from(RepairVerifyError::SigVerify));
                }
            }
        }
        Ok(())
    }

    fn check_ping_cache(
        ping_cache: &mut PingCache,
        request: &RepairProtocol,
        from_addr: &SocketAddr,
        identity_keypair: &Keypair,
    ) -> (bool, Option<Packet>) {
        let mut rng = rand::thread_rng();
        let mut pingf = move || Ping::new_rand(&mut rng, identity_keypair).ok();
        let (check, ping) =
            ping_cache.check(Instant::now(), (*request.sender(), *from_addr), &mut pingf);
        let ping_pkt = if let Some(ping) = ping {
            match request {
                RepairProtocol::WindowIndex { .. }
                | RepairProtocol::HighestWindowIndex { .. }
                | RepairProtocol::Orphan { .. } => {
                    let ping = RepairResponse::Ping(ping);
                    Packet::from_data(Some(from_addr), ping).ok()
                }
                RepairProtocol::AncestorHashes { .. } => {
                    let ping = AncestorHashesResponse::Ping(ping);
                    Packet::from_data(Some(from_addr), ping).ok()
                }
                RepairProtocol::Pong(_) => None,
                RepairProtocol::LegacyWindowIndex(_, _, _)
                | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
                | RepairProtocol::LegacyOrphan(_, _)
                | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
                | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
                | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
                | RepairProtocol::LegacyAncestorHashes(_, _, _) => {
                    error!("Unexpected legacy request: {request:?}");
                    debug_assert!(
                        false,
                        "Legacy requests should have been filtered out during signature
                        verification. {request:?}"
                    );
                    None
                }
            }
        } else {
            None
        };
        (check, ping_pkt)
    }

    fn handle_requests(
        &self,
        ping_cache: &mut PingCache,
        recycler: &PacketBatchRecycler,
        blockstore: &Blockstore,
        requests: Vec<RepairRequestWithMeta>,
        packet_batch_sender: &PacketBatchSender,
        stats: &mut ServeRepairStats,
        data_budget: &DataBudget,
    ) {
        let identity_keypair = self.cluster_info.keypair().clone();
        let mut pending_pings = Vec::default();

        for RepairRequestWithMeta {
            request,
            from_addr,
            stake,
            whitelisted: _,
            response_sender,
        } in requests.into_iter()
        {
            if !data_budget.check(request.max_response_bytes()) {
                stats.dropped_requests_outbound_bandwidth += 1;
                continue;
            }
            // Bypass ping/pong check for requests comming from QUIC endpoint.
            if !matches!(&request, RepairProtocol::Pong(_)) && response_sender.is_none() {
                let (check, ping_pkt) =
                    Self::check_ping_cache(ping_cache, &request, &from_addr, &identity_keypair);
                if let Some(ping_pkt) = ping_pkt {
                    pending_pings.push(ping_pkt);
                }
                if !check {
                    stats.ping_cache_check_failed += 1;
                    continue;
                }
            }
            stats.processed += 1;
            let Some(rsp) =
                Self::handle_repair(recycler, &from_addr, blockstore, request, stats, ping_cache)
            else {
                continue;
            };
            let num_response_packets = rsp.len();
            let num_response_bytes = rsp.iter().map(|p| p.meta().size).sum();
            if data_budget.take(num_response_bytes)
                && send_response(rsp, packet_batch_sender, response_sender)
            {
                stats.total_response_packets += num_response_packets;
                match stake > 0 {
                    true => stats.total_response_bytes_staked += num_response_bytes,
                    false => stats.total_response_bytes_unstaked += num_response_bytes,
                }
            } else {
                stats.dropped_requests_outbound_bandwidth += 1;
                stats.total_dropped_response_packets += num_response_packets;
            }
        }

        if !pending_pings.is_empty() {
            stats.pings_sent += pending_pings.len();
            let batch = PacketBatch::new(pending_pings);
            let _ = packet_batch_sender.send(batch);
        }
    }

    pub fn ancestor_repair_request_bytes(
        &self,
        keypair: &Keypair,
        repair_peer_id: &Pubkey,
        request_slot: Slot,
        nonce: Nonce,
    ) -> Result<Vec<u8>> {
        let header = RepairRequestHeader {
            signature: Signature::default(),
            sender: self.my_id(),
            recipient: *repair_peer_id,
            timestamp: timestamp(),
            nonce,
        };
        let request = RepairProtocol::AncestorHashes {
            header,
            slot: request_slot,
        };
        Self::repair_proto_to_bytes(&request, keypair)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn repair_request(
        &self,
        cluster_slots: &ClusterSlots,
        repair_request: ShredRepairType,
        peers_cache: &mut LruCache<Slot, RepairPeers>,
        repair_stats: &mut RepairStats,
        repair_validators: &Option<HashSet<Pubkey>>,
        outstanding_requests: &mut OutstandingShredRepairs,
        identity_keypair: &Keypair,
        quic_endpoint_sender: &AsyncSender<LocalRequest>,
        quic_endpoint_response_sender: &Sender<(SocketAddr, Vec<u8>)>,
        repair_protocol: Protocol,
    ) -> Result<Option<(SocketAddr, Vec<u8>)>> {
        // find a peer that appears to be accepting replication and has the desired slot, as indicated
        // by a valid tvu port location
        let slot = repair_request.slot();
        let repair_peers = match peers_cache.get(&slot) {
            Some(entry) if entry.asof.elapsed() < REPAIR_PEERS_CACHE_TTL => entry,
            _ => {
                peers_cache.pop(&slot);
                let repair_peers = self.repair_peers(repair_validators, slot);
                let weights = cluster_slots.compute_weights(slot, &repair_peers);
                let repair_peers = RepairPeers::new(Instant::now(), &repair_peers, &weights)?;
                peers_cache.put(slot, repair_peers);
                peers_cache.get(&slot).unwrap()
            }
        };
        let peer = repair_peers.sample(&mut rand::thread_rng());
        let nonce = outstanding_requests.add_request(repair_request, timestamp());
        let out = self.map_repair_request(
            &repair_request,
            &peer.pubkey,
            repair_stats,
            nonce,
            identity_keypair,
        )?;
        debug!(
            "Sending repair request from {} for {:#?}",
            identity_keypair.pubkey(),
            repair_request
        );
        match repair_protocol {
            Protocol::UDP => Ok(Some((peer.serve_repair, out))),
            Protocol::QUIC => {
                let num_expected_responses =
                    usize::try_from(repair_request.num_expected_responses()).unwrap();
                let request = LocalRequest {
                    remote_address: peer.serve_repair_quic,
                    bytes: out,
                    num_expected_responses,
                    response_sender: quic_endpoint_response_sender.clone(),
                };
                quic_endpoint_sender
                    .blocking_send(request)
                    .map_err(|_| Error::SendError)
                    .map(|()| None)
            }
        }
    }

    pub(crate) fn repair_request_ancestor_hashes_sample_peers(
        &self,
        slot: Slot,
        cluster_slots: &ClusterSlots,
        repair_validators: &Option<HashSet<Pubkey>>,
        repair_protocol: Protocol,
    ) -> Result<Vec<(Pubkey, SocketAddr)>> {
        let repair_peers: Vec<_> = self.repair_peers(repair_validators, slot);
        if repair_peers.is_empty() {
            return Err(ClusterInfoError::NoPeers.into());
        }
        let (weights, index): (Vec<_>, Vec<_>) = cluster_slots
            .compute_weights_exclude_nonfrozen(slot, &repair_peers)
            .into_iter()
            .unzip();
        let peers = WeightedShuffle::new("repair_request_ancestor_hashes", &weights)
            .shuffle(&mut rand::thread_rng())
            .map(|i| index[i])
            .filter_map(|i| {
                let addr = repair_peers[i].serve_repair(repair_protocol).ok()?;
                Some((*repair_peers[i].pubkey(), addr))
            })
            .take(get_ancestor_hash_repair_sample_size())
            .collect();
        Ok(peers)
    }

    #[cfg(test)]
    pub(crate) fn repair_request_duplicate_compute_best_peer(
        &self,
        slot: Slot,
        cluster_slots: &ClusterSlots,
        repair_validators: &Option<HashSet<Pubkey>>,
    ) -> Result<(Pubkey, SocketAddr)> {
        let repair_peers: Vec<_> = self.repair_peers(repair_validators, slot);
        if repair_peers.is_empty() {
            return Err(ClusterInfoError::NoPeers.into());
        }
        let (weights, index): (Vec<_>, Vec<_>) = cluster_slots
            .compute_weights_exclude_nonfrozen(slot, &repair_peers)
            .into_iter()
            .unzip();
        let k = WeightedIndex::new(weights)?.sample(&mut rand::thread_rng());
        let n = index[k];
        Ok((
            *repair_peers[n].pubkey(),
            repair_peers[n].serve_repair(Protocol::UDP)?,
        ))
    }

    pub(crate) fn map_repair_request(
        &self,
        repair_request: &ShredRepairType,
        repair_peer_id: &Pubkey,
        repair_stats: &mut RepairStats,
        nonce: Nonce,
        identity_keypair: &Keypair,
    ) -> Result<Vec<u8>> {
        let header = RepairRequestHeader {
            signature: Signature::default(),
            sender: self.my_id(),
            recipient: *repair_peer_id,
            timestamp: timestamp(),
            nonce,
        };
        let request_proto = match repair_request {
            ShredRepairType::Shred(slot, shred_index) => {
                repair_stats
                    .shred
                    .update(repair_peer_id, *slot, *shred_index);
                RepairProtocol::WindowIndex {
                    header,
                    slot: *slot,
                    shred_index: *shred_index,
                }
            }
            ShredRepairType::HighestShred(slot, shred_index) => {
                repair_stats
                    .highest_shred
                    .update(repair_peer_id, *slot, *shred_index);
                RepairProtocol::HighestWindowIndex {
                    header,
                    slot: *slot,
                    shred_index: *shred_index,
                }
            }
            ShredRepairType::Orphan(slot) => {
                repair_stats.orphan.update(repair_peer_id, *slot, 0);
                RepairProtocol::Orphan {
                    header,
                    slot: *slot,
                }
            }
        };
        Self::repair_proto_to_bytes(&request_proto, identity_keypair)
    }

    /// Distinguish and process `RepairResponse` ping packets ignoring other
    /// packets in the batch.
    pub(crate) fn handle_repair_response_pings(
        repair_socket: &UdpSocket,
        keypair: &Keypair,
        packet_batch: &mut PacketBatch,
        stats: &mut ShredFetchStats,
    ) {
        let mut pending_pongs = Vec::default();
        for packet in packet_batch.iter_mut() {
            if packet.meta().size != REPAIR_RESPONSE_SERIALIZED_PING_BYTES {
                continue;
            }
            if let Ok(RepairResponse::Ping(ping)) = packet.deserialize_slice(..) {
                if !ping.verify() {
                    // Do _not_ set `discard` to allow shred processing to attempt to
                    // handle the packet.
                    // Ping error count may include false posities for shreds of size
                    // `REPAIR_RESPONSE_SERIALIZED_PING_BYTES` whose first 4 bytes
                    // match `RepairResponse` discriminator (these 4 bytes overlap
                    // with the shred signature field).
                    stats.ping_err_verify_count += 1;
                    continue;
                }
                packet.meta_mut().set_discard(true);
                stats.ping_count += 1;
                if let Ok(pong) = Pong::new(&ping, keypair) {
                    let pong = RepairProtocol::Pong(pong);
                    if let Ok(pong_bytes) = serialize(&pong) {
                        let from_addr = packet.meta().socket_addr();
                        pending_pongs.push((pong_bytes, from_addr));
                    }
                }
            }
        }
        if !pending_pongs.is_empty() {
            if let Err(SendPktsError::IoError(err, num_failed)) =
                batch_send(repair_socket, &pending_pongs)
            {
                warn!(
                    "batch_send failed to send {}/{} packets. First error: {:?}",
                    num_failed,
                    pending_pongs.len(),
                    err
                );
            }
        }
    }

    pub fn repair_proto_to_bytes(request: &RepairProtocol, keypair: &Keypair) -> Result<Vec<u8>> {
        debug_assert!(request.supports_signature());
        let mut payload = serialize(&request)?;
        let signable_data = [&payload[..4], &payload[4 + SIGNATURE_BYTES..]].concat();
        let signature = keypair.sign_message(&signable_data[..]);
        payload[4..4 + SIGNATURE_BYTES].copy_from_slice(signature.as_ref());
        Ok(payload)
    }

    fn repair_peers(
        &self,
        repair_validators: &Option<HashSet<Pubkey>>,
        slot: Slot,
    ) -> Vec<ContactInfo> {
        if let Some(repair_validators) = repair_validators {
            repair_validators
                .iter()
                .filter_map(|key| {
                    if *key != self.my_id() {
                        self.cluster_info.lookup_contact_info(key, |ci| ci.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            self.cluster_info.repair_peers(slot)
        }
    }

    fn run_window_request(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: &Blockstore,
        slot: Slot,
        shred_index: u64,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        // Try to find the requested index in one of the slots
        let packet = repair_response::repair_response_packet(
            blockstore,
            slot,
            shred_index,
            from_addr,
            nonce,
        )?;
        Some(PacketBatch::new_unpinned_with_recycler_data(
            recycler,
            "run_window_request",
            vec![packet],
        ))
    }

    fn run_highest_window_request(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: &Blockstore,
        slot: Slot,
        highest_index: u64,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        // Try to find the requested index in one of the slots
        let meta = blockstore.meta(slot).ok()??;
        if meta.received > highest_index {
            // meta.received must be at least 1 by this point
            let packet = repair_response::repair_response_packet(
                blockstore,
                slot,
                meta.received - 1,
                from_addr,
                nonce,
            )?;
            return Some(PacketBatch::new_unpinned_with_recycler_data(
                recycler,
                "run_highest_window_request",
                vec![packet],
            ));
        }
        None
    }

    fn run_orphan(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: &Blockstore,
        slot: Slot,
        max_responses: usize,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        let mut res =
            PacketBatch::new_unpinned_with_recycler(recycler, max_responses, "run_orphan");
        // Try to find the next "n" parent slots of the input slot
        let packets = std::iter::successors(blockstore.meta(slot).ok()?, |meta| {
            blockstore.meta(meta.parent_slot?).ok()?
        })
        .map_while(|meta| {
            repair_response::repair_response_packet(
                blockstore,
                meta.slot,
                meta.received.checked_sub(1u64)?,
                from_addr,
                nonce,
            )
        });
        for packet in packets.take(max_responses) {
            res.push(packet);
        }
        (!res.is_empty()).then_some(res)
    }

    fn run_ancestor_hashes(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: &Blockstore,
        slot: Slot,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        let ancestor_slot_hashes = if blockstore.is_duplicate_confirmed(slot) {
            let ancestor_iterator =
                AncestorIteratorWithHash::from(AncestorIterator::new_inclusive(slot, blockstore));
            ancestor_iterator.take(MAX_ANCESTOR_RESPONSES).collect()
        } else {
            // If this slot is not duplicate confirmed, return nothing
            vec![]
        };
        let response = AncestorHashesResponse::Hashes(ancestor_slot_hashes);
        let serialized_response = serialize(&response).ok()?;

        // Could probably directly write response into packet via `serialize_into()`
        // instead of incurring extra copy in `repair_response_packet_from_bytes`, but
        // serialize_into doesn't return the written size...
        let packet = repair_response::repair_response_packet_from_bytes(
            serialized_response,
            from_addr,
            nonce,
        )?;
        Some(PacketBatch::new_unpinned_with_recycler_data(
            recycler,
            "run_ancestor_hashes",
            vec![packet],
        ))
    }
}

#[inline]
pub(crate) fn get_repair_protocol(_: ClusterType) -> Protocol {
    Protocol::UDP
}

fn deserialize_request<T>(request: &RemoteRequest) -> std::result::Result<T, bincode::Error>
where
    T: serde::de::DeserializeOwned,
{
    bincode::options()
        .with_limit(request.bytes.len() as u64)
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .deserialize(&request.bytes)
}

// Returns true on success.
fn send_response(
    packets: PacketBatch,
    packet_batch_sender: &PacketBatchSender,
    response_sender: Option<OneShotSender<Vec<Vec<u8>>>>,
) -> bool {
    match response_sender {
        None => packet_batch_sender.send(packets).is_ok(),
        Some(response_sender) => {
            let response = packets
                .iter()
                .filter_map(|packet| packet.data(..))
                .map(Vec::from)
                .collect();
            response_sender.send(response).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::repair::repair_response,
        solana_gossip::{contact_info::ContactInfo, socketaddr, socketaddr_any},
        solana_ledger::{
            blockstore::make_many_slot_entries,
            blockstore_processor::fill_blockstore_slot_with_ticks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path,
            shred::{max_ticks_per_n_shreds, Shred, ShredFlags},
        },
        solana_perf::packet::{deserialize_from_with_limit, Packet},
        solana_runtime::bank::Bank,
        solana_sdk::{
            feature_set::FeatureSet, hash::Hash, pubkey::Pubkey, signature::Keypair,
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{io::Cursor, net::Ipv4Addr},
    };

    #[test]
    fn test_serialized_ping_size() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let ping = Ping::new_rand(&mut rng, &keypair).unwrap();
        let ping = RepairResponse::Ping(ping);
        let pkt = Packet::from_data(None, ping).unwrap();
        assert_eq!(pkt.meta().size, REPAIR_RESPONSE_SERIALIZED_PING_BYTES);
    }

    #[test]
    fn test_deserialize_shred_as_ping() {
        let data_buf = vec![7u8, 44]; // REPAIR_RESPONSE_SERIALIZED_PING_BYTES - SIZE_OF_DATA_SHRED_HEADERS
        let keypair = Keypair::new();
        let mut shred = Shred::new_from_data(
            123, // slot
            456, // index
            111, // parent_offset
            &data_buf,
            ShredFlags::empty(),
            222, // reference_tick
            333, // version
            444, // fec_set_index
        );
        shred.sign(&keypair);
        let mut pkt = Packet::default();
        shred.copy_to_packet(&mut pkt);
        pkt.meta_mut().size = REPAIR_RESPONSE_SERIALIZED_PING_BYTES;
        let res = pkt.deserialize_slice::<RepairResponse, _>(..);
        if let Ok(RepairResponse::Ping(ping)) = res {
            assert!(!ping.verify());
        } else {
            assert!(res.is_err());
        }
    }

    fn repair_request_header_for_tests() -> RepairRequestHeader {
        RepairRequestHeader {
            signature: Signature::default(),
            sender: Pubkey::default(),
            recipient: Pubkey::default(),
            timestamp: timestamp(),
            nonce: Nonce::default(),
        }
    }

    fn make_remote_request(packet: &Packet) -> RemoteRequest {
        RemoteRequest {
            remote_pubkey: None,
            remote_address: packet.meta().socket_addr(),
            bytes: packet.data(..).map(Vec::from).unwrap(),
            response_sender: None,
        }
    }

    #[test]
    fn test_check_well_formed_repair_request() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let ping = ping_pong::Ping::<[u8; 32]>::new_rand(&mut rng, &keypair).unwrap();
        let pong = Pong::new(&ping, &keypair).unwrap();
        let request = RepairProtocol::Pong(pong);
        let mut pkt = Packet::from_data(None, request).unwrap();
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 1);
        pkt.meta_mut().size = 5;
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 0);
        assert_eq!(stats.err_malformed, 1);

        let request = RepairProtocol::WindowIndex {
            header: repair_request_header_for_tests(),
            slot: 123,
            shred_index: 456,
        };
        let mut pkt = Packet::from_data(None, request).unwrap();
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 1);
        pkt.meta_mut().size = 8;
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 0);
        assert_eq!(stats.err_malformed, 1);

        let request = RepairProtocol::AncestorHashes {
            header: repair_request_header_for_tests(),
            slot: 123,
        };
        let mut pkt = Packet::from_data(None, request).unwrap();
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 1);
        pkt.meta_mut().size = 1;
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 0);
        assert_eq!(stats.err_malformed, 1);

        let request = RepairProtocol::LegacyOrphan(LegacyContactInfo::default(), 123);
        let mut pkt = Packet::from_data(None, request).unwrap();
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 1);
        pkt.meta_mut().size = 3;
        let mut batch = vec![make_remote_request(&pkt)];
        let mut stats = ServeRepairStats::default();
        let num_well_formed = discard_malformed_repair_requests(&mut batch, &mut stats);
        assert_eq!(num_well_formed, 0);
        assert_eq!(stats.err_malformed, 1);
    }

    #[test]
    fn test_serialize_deserialize_signed_request() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let cluster_info = Arc::new(new_test_cluster_info());
        let serve_repair = ServeRepair::new(
            cluster_info.clone(),
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );
        let keypair = cluster_info.keypair().clone();
        let repair_peer_id = solana_sdk::pubkey::new_rand();
        let repair_request = ShredRepairType::Orphan(123);

        let rsp = serve_repair
            .map_repair_request(
                &repair_request,
                &repair_peer_id,
                &mut RepairStats::default(),
                456,
                &keypair,
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol =
            deserialize_from_with_limit(&mut cursor).unwrap();
        assert_eq!(cursor.position(), rsp.len() as u64);
        if let RepairProtocol::Orphan { header, slot } = deserialized_request {
            assert_eq!(slot, 123);
            assert_eq!(header.nonce, 456);
            assert_eq!(&header.sender, &serve_repair.my_id());
            assert_eq!(&header.recipient, &repair_peer_id);
            let signed_data = [&rsp[..4], &rsp[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }
    }

    #[test]
    fn test_serialize_deserialize_ancestor_hashes_request() {
        let slot: Slot = 50;
        let nonce = 70;
        let cluster_info = Arc::new(new_test_cluster_info());
        let repair_peer_id = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let keypair = cluster_info.keypair().clone();

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        let bank_forks = BankForks::new_rw_arc(bank);
        let serve_repair = ServeRepair::new(
            cluster_info,
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );

        let request_bytes = serve_repair
            .ancestor_repair_request_bytes(&keypair, &repair_peer_id, slot, nonce)
            .unwrap();
        let mut cursor = Cursor::new(&request_bytes[..]);
        let deserialized_request: RepairProtocol =
            deserialize_from_with_limit(&mut cursor).unwrap();
        assert_eq!(cursor.position(), request_bytes.len() as u64);
        if let RepairProtocol::AncestorHashes {
            header,
            slot: deserialized_slot,
        } = deserialized_request
        {
            assert_eq!(deserialized_slot, slot);
            assert_eq!(header.nonce, nonce);
            assert_eq!(&header.sender, &serve_repair.my_id());
            assert_eq!(&header.recipient, &repair_peer_id);
            let signed_data = [&request_bytes[..4], &request_bytes[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }
    }

    #[test]
    fn test_map_requests_signed() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let cluster_info = Arc::new(new_test_cluster_info());
        let serve_repair = ServeRepair::new(
            cluster_info.clone(),
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );
        let keypair = cluster_info.keypair().clone();
        let repair_peer_id = solana_sdk::pubkey::new_rand();

        let slot = 50;
        let shred_index = 60;
        let nonce = 70;

        let request = ShredRepairType::Shred(slot, shred_index);
        let request_bytes = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                &keypair,
            )
            .unwrap();

        let mut cursor = Cursor::new(&request_bytes[..]);
        let deserialized_request: RepairProtocol =
            deserialize_from_with_limit(&mut cursor).unwrap();
        assert_eq!(cursor.position(), request_bytes.len() as u64);
        if let RepairProtocol::WindowIndex {
            header,
            slot: deserialized_slot,
            shred_index: deserialized_shred_index,
        } = deserialized_request
        {
            assert_eq!(deserialized_slot, slot);
            assert_eq!(deserialized_shred_index, shred_index);
            assert_eq!(header.nonce, nonce);
            assert_eq!(&header.sender, &serve_repair.my_id());
            assert_eq!(&header.recipient, &repair_peer_id);
            let signed_data = [&request_bytes[..4], &request_bytes[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }

        let request = ShredRepairType::HighestShred(slot, shred_index);
        let request_bytes = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                &keypair,
            )
            .unwrap();

        let mut cursor = Cursor::new(&request_bytes[..]);
        let deserialized_request: RepairProtocol =
            deserialize_from_with_limit(&mut cursor).unwrap();
        assert_eq!(cursor.position(), request_bytes.len() as u64);
        if let RepairProtocol::HighestWindowIndex {
            header,
            slot: deserialized_slot,
            shred_index: deserialized_shred_index,
        } = deserialized_request
        {
            assert_eq!(deserialized_slot, slot);
            assert_eq!(deserialized_shred_index, shred_index);
            assert_eq!(header.nonce, nonce);
            assert_eq!(&header.sender, &serve_repair.my_id());
            assert_eq!(&header.recipient, &repair_peer_id);
            let signed_data = [&request_bytes[..4], &request_bytes[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }
    }

    #[test]
    fn test_verify_signed_packet() {
        let my_keypair = Keypair::new();
        let other_keypair = Keypair::new();

        fn sign_packet(packet: &mut Packet, keypair: &Keypair) {
            let signable_data = [
                packet.data(..4).unwrap(),
                packet.data(4 + SIGNATURE_BYTES..).unwrap(),
            ]
            .concat();
            let signature = keypair.sign_message(&signable_data[..]);
            packet.buffer_mut()[4..4 + SIGNATURE_BYTES].copy_from_slice(signature.as_ref());
        }

        // well formed packet
        let packet = {
            let header = RepairRequestHeader::new(
                my_keypair.pubkey(),
                other_keypair.pubkey(),
                timestamp(),
                678,
            );
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, request).unwrap();
            sign_packet(&mut packet, &my_keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        assert_matches!(
            ServeRepair::verify_signed_packet(
                &other_keypair.pubkey(),
                packet.data(..).unwrap(),
                &request
            ),
            Ok(())
        );

        // recipient mismatch
        let packet = {
            let header = RepairRequestHeader::new(
                my_keypair.pubkey(),
                other_keypair.pubkey(),
                timestamp(),
                678,
            );
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, request).unwrap();
            sign_packet(&mut packet, &my_keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        assert_matches!(
            ServeRepair::verify_signed_packet(
                &my_keypair.pubkey(),
                packet.data(..).unwrap(),
                &request
            ),
            Err(Error::RepairVerify(RepairVerifyError::IdMismatch))
        );

        // outside time window
        let packet = {
            let time_diff_ms = u64::try_from(SIGNED_REPAIR_TIME_WINDOW.as_millis() * 2).unwrap();
            let old_timestamp = timestamp().saturating_sub(time_diff_ms);
            let header = RepairRequestHeader::new(
                my_keypair.pubkey(),
                other_keypair.pubkey(),
                old_timestamp,
                678,
            );
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, request).unwrap();
            sign_packet(&mut packet, &my_keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        assert_matches!(
            ServeRepair::verify_signed_packet(
                &other_keypair.pubkey(),
                packet.data(..).unwrap(),
                &request
            ),
            Err(Error::RepairVerify(RepairVerifyError::TimeSkew))
        );

        // bad signature
        let packet = {
            let header = RepairRequestHeader::new(
                my_keypair.pubkey(),
                other_keypair.pubkey(),
                timestamp(),
                678,
            );
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, request).unwrap();
            sign_packet(&mut packet, &other_keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        assert_matches!(
            ServeRepair::verify_signed_packet(
                &other_keypair.pubkey(),
                packet.data(..).unwrap(),
                &request
            ),
            Err(Error::RepairVerify(RepairVerifyError::SigVerify))
        );
    }

    #[test]
    fn test_run_highest_window_request() {
        run_highest_window_request(5, 3, 9);
    }

    /// test run_window_request responds with the right shred, and do not overrun
    fn run_highest_window_request(slot: Slot, num_slots: u64, nonce: Nonce) {
        let recycler = PacketBatchRecycler::default();
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                0,
                0,
                nonce,
            );
            assert!(rv.is_none());

            let _ = fill_blockstore_slot_with_ticks(
                &blockstore,
                max_ticks_per_n_shreds(1, None) + 1,
                slot,
                slot - num_slots + 1,
                Hash::default(),
            );

            let index = 1;
            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot,
                index,
                nonce,
            )
            .expect("packets");
            let request = ShredRepairType::HighestShred(slot, index);
            verify_responses(&request, rv.iter());

            let rv: Vec<Shred> = rv
                .into_iter()
                .filter_map(|p| {
                    assert_eq!(repair_response::nonce(p).unwrap(), nonce);
                    Shred::new_from_serialized_shred(p.data(..).unwrap().to_vec()).ok()
                })
                .collect();
            assert!(!rv.is_empty());
            let index = blockstore.meta(slot).unwrap().unwrap().received - 1;
            assert_eq!(rv[0].index(), index as u32);
            assert_eq!(rv[0].slot(), slot);

            let rv = ServeRepair::run_highest_window_request(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot,
                index + 1,
                nonce,
            );
            assert!(rv.is_none());
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_run_window_request() {
        run_window_request(2, 9);
    }

    /// test window requests respond with the right shred, and do not overrun
    fn run_window_request(slot: Slot, nonce: Nonce) {
        let recycler = PacketBatchRecycler::default();
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv = ServeRepair::run_window_request(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot,
                0,
                nonce,
            );
            assert!(rv.is_none());
            let shred = Shred::new_from_data(slot, 1, 1, &[], ShredFlags::empty(), 0, 2, 0);

            blockstore
                .insert_shreds(vec![shred], None, false)
                .expect("Expect successful ledger write");

            let index = 1;
            let rv = ServeRepair::run_window_request(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot,
                index,
                nonce,
            )
            .expect("packets");
            let request = ShredRepairType::Shred(slot, index);
            verify_responses(&request, rv.iter());
            let rv: Vec<Shred> = rv
                .into_iter()
                .filter_map(|p| {
                    assert_eq!(repair_response::nonce(p).unwrap(), nonce);
                    Shred::new_from_serialized_shred(p.data(..).unwrap().to_vec()).ok()
                })
                .collect();
            assert_eq!(rv[0].index(), 1);
            assert_eq!(rv[0].slot(), slot);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    fn new_test_cluster_info() -> ClusterInfo {
        let keypair = Arc::new(Keypair::new());
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), timestamp());
        ClusterInfo::new(contact_info, keypair, SocketAddrSpace::Unspecified)
    }

    #[test]
    fn window_index_request() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let cluster_slots = ClusterSlots::default();
        let cluster_info = Arc::new(new_test_cluster_info());
        let serve_repair = ServeRepair::new(
            cluster_info.clone(),
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );
        let identity_keypair = cluster_info.keypair().clone();
        let mut outstanding_requests = OutstandingShredRepairs::default();
        let (quic_endpoint_sender, _quic_endpoint_receiver) =
            tokio::sync::mpsc::channel(/*buffer:*/ 128);
        let (quic_endpoint_response_sender, _quic_endpoint_response_receiver) =
            crossbeam_channel::unbounded();
        let rv = serve_repair.repair_request(
            &cluster_slots,
            ShredRepairType::Shred(0, 0),
            &mut LruCache::new(100),
            &mut RepairStats::default(),
            &None,
            &mut outstanding_requests,
            &identity_keypair,
            &quic_endpoint_sender,
            &quic_endpoint_response_sender,
            Protocol::UDP, // repair_protocol
        );
        assert_matches!(rv, Err(Error::ClusterInfo(ClusterInfoError::NoPeers)));

        let serve_repair_addr = socketaddr!(Ipv4Addr::LOCALHOST, 1243);
        let mut nxt = ContactInfo::new(
            solana_sdk::pubkey::new_rand(),
            timestamp(), // wallclock
            0u16,        // shred_version
        );
        nxt.set_gossip((Ipv4Addr::LOCALHOST, 1234)).unwrap();
        nxt.set_tvu((Ipv4Addr::LOCALHOST, 1235)).unwrap();
        nxt.set_tvu_quic((Ipv4Addr::LOCALHOST, 1236)).unwrap();
        nxt.set_tpu((Ipv4Addr::LOCALHOST, 1238)).unwrap();
        nxt.set_tpu_forwards((Ipv4Addr::LOCALHOST, 1239)).unwrap();
        nxt.set_tpu_vote((Ipv4Addr::LOCALHOST, 1240)).unwrap();
        nxt.set_rpc((Ipv4Addr::LOCALHOST, 1241)).unwrap();
        nxt.set_rpc_pubsub((Ipv4Addr::LOCALHOST, 1242)).unwrap();
        nxt.set_serve_repair(serve_repair_addr).unwrap();
        nxt.set_serve_repair_quic((Ipv4Addr::LOCALHOST, 1237))
            .unwrap();
        cluster_info.insert_info(nxt.clone());
        let rv = serve_repair
            .repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &None,
                &mut outstanding_requests,
                &identity_keypair,
                &quic_endpoint_sender,
                &quic_endpoint_response_sender,
                Protocol::UDP, // repair_protocol
            )
            .unwrap()
            .unwrap();
        assert_eq!(nxt.serve_repair(Protocol::UDP).unwrap(), serve_repair_addr);
        assert_eq!(rv.0, nxt.serve_repair(Protocol::UDP).unwrap());

        let serve_repair_addr2 = socketaddr!([127, 0, 0, 2], 1243);
        let mut nxt = ContactInfo::new(
            solana_sdk::pubkey::new_rand(),
            timestamp(), // wallclock
            0u16,        // shred_version
        );
        nxt.set_gossip((Ipv4Addr::LOCALHOST, 1234)).unwrap();
        nxt.set_tvu((Ipv4Addr::LOCALHOST, 1235)).unwrap();
        nxt.set_tvu_quic((Ipv4Addr::LOCALHOST, 1236)).unwrap();
        nxt.set_tpu((Ipv4Addr::LOCALHOST, 1238)).unwrap();
        nxt.set_tpu_forwards((Ipv4Addr::LOCALHOST, 1239)).unwrap();
        nxt.set_tpu_vote((Ipv4Addr::LOCALHOST, 1240)).unwrap();
        nxt.set_rpc((Ipv4Addr::LOCALHOST, 1241)).unwrap();
        nxt.set_rpc_pubsub((Ipv4Addr::LOCALHOST, 1242)).unwrap();
        nxt.set_serve_repair(serve_repair_addr2).unwrap();
        nxt.set_serve_repair_quic((Ipv4Addr::LOCALHOST, 1237))
            .unwrap();
        cluster_info.insert_info(nxt);
        let mut one = false;
        let mut two = false;
        while !one || !two {
            //this randomly picks an option, so eventually it should pick both
            let rv = serve_repair
                .repair_request(
                    &cluster_slots,
                    ShredRepairType::Shred(0, 0),
                    &mut LruCache::new(100),
                    &mut RepairStats::default(),
                    &None,
                    &mut outstanding_requests,
                    &identity_keypair,
                    &quic_endpoint_sender,
                    &quic_endpoint_response_sender,
                    Protocol::UDP, // repair_protocol
                )
                .unwrap()
                .unwrap();
            if rv.0 == serve_repair_addr {
                one = true;
            }
            if rv.0 == serve_repair_addr2 {
                two = true;
            }
        }
        assert!(one && two);
    }

    #[test]
    fn test_run_orphan() {
        run_orphan(2, 3, 9);
    }

    fn run_orphan(slot: Slot, num_slots: u64, nonce: Nonce) {
        solana_logger::setup();
        let recycler = PacketBatchRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            let rv =
                ServeRepair::run_orphan(&recycler, &socketaddr_any!(), &blockstore, slot, 0, nonce);
            assert!(rv.is_none());

            // Create slots [slot, slot + num_slots) with 5 shreds apiece
            let (shreds, _) = make_many_slot_entries(slot, num_slots, 5);

            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");

            // We don't have slot `slot + num_slots`, so we don't know how to service this request
            let rv = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot + num_slots,
                5,
                nonce,
            );
            assert!(rv.is_none());

            // For a orphan request for `slot + num_slots - 1`, we should return the highest shreds
            // from slots in the range [slot, slot + num_slots - 1]
            let rv: Vec<_> = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot + num_slots - 1,
                5,
                nonce,
            )
            .expect("run_orphan packets")
            .iter()
            .cloned()
            .collect();

            // Verify responses
            let request = ShredRepairType::Orphan(slot);
            verify_responses(&request, rv.iter());

            let expected: Vec<_> = (slot..slot + num_slots)
                .rev()
                .filter_map(|slot| {
                    let index = blockstore.meta(slot).unwrap().unwrap().received - 1;
                    repair_response::repair_response_packet(
                        &blockstore,
                        slot,
                        index,
                        &socketaddr_any!(),
                        nonce,
                    )
                })
                .collect();
            assert_eq!(rv, expected);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn run_orphan_corrupted_shred_size() {
        solana_logger::setup();
        let recycler = PacketBatchRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
            // Create slots [1, 2] with 1 shred apiece
            let (mut shreds, _) = make_many_slot_entries(1, 2, 1);

            assert_eq!(shreds[0].slot(), 1);
            assert_eq!(shreds[0].index(), 0);
            // TODO: The test previously relied on corrupting shred payload
            // size which we no longer want to expose. Current test no longer
            // covers packet size check in repair_response_packet_from_bytes.
            shreds.remove(0);
            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");
            let nonce = 42;
            // Make sure repair response is corrupted
            assert!(repair_response::repair_response_packet(
                &blockstore,
                1,
                0,
                &socketaddr_any!(),
                nonce,
            )
            .is_none());

            // Orphan request for slot 2 should only return slot 1 since
            // calling `repair_response_packet` on slot 1's shred will
            // be corrupted
            let rv: Vec<_> =
                ServeRepair::run_orphan(&recycler, &socketaddr_any!(), &blockstore, 2, 5, nonce)
                    .expect("run_orphan packets")
                    .iter()
                    .cloned()
                    .collect();

            // Verify responses
            let expected = vec![repair_response::repair_response_packet(
                &blockstore,
                2,
                0,
                &socketaddr_any!(),
                nonce,
            )
            .unwrap()];
            assert_eq!(rv, expected);
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_run_ancestor_hashes() {
        fn deserialize_ancestor_hashes_response(packet: &Packet) -> AncestorHashesResponse {
            packet
                .deserialize_slice(..packet.meta().size - SIZE_OF_NONCE)
                .unwrap()
        }

        solana_logger::setup();
        let recycler = PacketBatchRecycler::default();
        let ledger_path = get_tmp_ledger_path!();
        {
            let slot = 0;
            let num_slots = MAX_ANCESTOR_RESPONSES as u64;
            let nonce = 10;

            let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());

            // Create slots [slot, slot + num_slots) with 5 shreds apiece
            let (shreds, _) = make_many_slot_entries(slot, num_slots, 5);

            blockstore
                .insert_shreds(shreds, None, false)
                .expect("Expect successful ledger write");

            // We don't have slot `slot + num_slots`, so we return empty
            let rv = ServeRepair::run_ancestor_hashes(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot + num_slots,
                nonce,
            )
            .expect("run_ancestor_hashes packets");
            assert_eq!(rv.len(), 1);
            let packet = &rv[0];
            let ancestor_hashes_response = deserialize_ancestor_hashes_response(packet);
            match ancestor_hashes_response {
                AncestorHashesResponse::Hashes(hashes) => {
                    assert!(hashes.is_empty());
                }
                _ => {
                    panic!("unexpected response: {:?}", &ancestor_hashes_response);
                }
            }

            // `slot + num_slots - 1` is not marked duplicate confirmed so nothing should return
            // empty
            let rv = ServeRepair::run_ancestor_hashes(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot + num_slots - 1,
                nonce,
            )
            .expect("run_ancestor_hashes packets");
            assert_eq!(rv.len(), 1);
            let packet = &rv[0];
            let ancestor_hashes_response = deserialize_ancestor_hashes_response(packet);
            match ancestor_hashes_response {
                AncestorHashesResponse::Hashes(hashes) => {
                    assert!(hashes.is_empty());
                }
                _ => {
                    panic!("unexpected response: {:?}", &ancestor_hashes_response);
                }
            }

            // Set duplicate confirmed
            let mut expected_ancestors = Vec::with_capacity(num_slots as usize);
            expected_ancestors.resize(num_slots as usize, (0, Hash::default()));
            for (i, duplicate_confirmed_slot) in (slot..slot + num_slots).enumerate() {
                let frozen_hash = Hash::new_unique();
                expected_ancestors[num_slots as usize - i - 1] =
                    (duplicate_confirmed_slot, frozen_hash);
                blockstore.insert_bank_hash(duplicate_confirmed_slot, frozen_hash, true);
            }
            let rv = ServeRepair::run_ancestor_hashes(
                &recycler,
                &socketaddr_any!(),
                &blockstore,
                slot + num_slots - 1,
                nonce,
            )
            .expect("run_ancestor_hashes packets");
            assert_eq!(rv.len(), 1);
            let packet = &rv[0];
            let ancestor_hashes_response = deserialize_ancestor_hashes_response(packet);
            match ancestor_hashes_response {
                AncestorHashesResponse::Hashes(hashes) => {
                    assert_eq!(hashes, expected_ancestors);
                }
                _ => {
                    panic!("unexpected response: {:?}", &ancestor_hashes_response);
                }
            }
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_repair_with_repair_validators() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let cluster_slots = ClusterSlots::default();
        let cluster_info = Arc::new(new_test_cluster_info());
        let me = cluster_info.my_contact_info();
        let (quic_endpoint_sender, _quic_endpoint_receiver) =
            tokio::sync::mpsc::channel(/*buffer:*/ 128);
        let (quic_endpoint_response_sender, _quic_endpoint_response_receiver) =
            crossbeam_channel::unbounded();
        // Insert two peers on the network
        let contact_info2 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let contact_info3 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        cluster_info.insert_info(contact_info2.clone());
        cluster_info.insert_info(contact_info3.clone());
        let identity_keypair = cluster_info.keypair().clone();
        let serve_repair = ServeRepair::new(
            cluster_info,
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );

        // If:
        // 1) repair validator set doesn't exist in gossip
        // 2) repair validator set only includes our own id
        // then no repairs should be generated
        for pubkey in &[solana_sdk::pubkey::new_rand(), *me.pubkey()] {
            let known_validators = Some(vec![*pubkey].into_iter().collect());
            assert!(serve_repair.repair_peers(&known_validators, 1).is_empty());
            assert_matches!(
                serve_repair.repair_request(
                    &cluster_slots,
                    ShredRepairType::Shred(0, 0),
                    &mut LruCache::new(100),
                    &mut RepairStats::default(),
                    &known_validators,
                    &mut OutstandingShredRepairs::default(),
                    &identity_keypair,
                    &quic_endpoint_sender,
                    &quic_endpoint_response_sender,
                    Protocol::UDP, // repair_protocol
                ),
                Err(Error::ClusterInfo(ClusterInfoError::NoPeers))
            );
        }

        // If known validator exists in gossip, should return repair successfully
        let known_validators = Some(vec![*contact_info2.pubkey()].into_iter().collect());
        let repair_peers = serve_repair.repair_peers(&known_validators, 1);
        assert_eq!(repair_peers.len(), 1);
        assert_eq!(repair_peers[0].pubkey(), contact_info2.pubkey());
        assert_matches!(
            serve_repair.repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &known_validators,
                &mut OutstandingShredRepairs::default(),
                &identity_keypair,
                &quic_endpoint_sender,
                &quic_endpoint_response_sender,
                Protocol::UDP, // repair_protocol
            ),
            Ok(Some(_))
        );

        // Using no known validators should default to all
        // validator's available in gossip, excluding myself
        let repair_peers: HashSet<Pubkey> = serve_repair
            .repair_peers(&None, 1)
            .into_iter()
            .map(|node| *node.pubkey())
            .collect();
        assert_eq!(repair_peers.len(), 2);
        assert!(repair_peers.contains(contact_info2.pubkey()));
        assert!(repair_peers.contains(contact_info3.pubkey()));
        assert_matches!(
            serve_repair.repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &None,
                &mut OutstandingShredRepairs::default(),
                &identity_keypair,
                &quic_endpoint_sender,
                &quic_endpoint_response_sender,
                Protocol::UDP, // repair_protocol
            ),
            Ok(Some(_))
        );
    }

    #[test]
    fn test_verify_shred_response() {
        fn new_test_data_shred(slot: Slot, index: u32) -> Shred {
            Shred::new_from_data(slot, index, 1, &[], ShredFlags::empty(), 0, 0, 0)
        }
        let repair = ShredRepairType::Orphan(9);
        // Ensure new options are addded to this test
        match repair {
            ShredRepairType::Orphan(_)
            | ShredRepairType::HighestShred(_, _)
            | ShredRepairType::Shred(_, _) => (),
        };

        let slot = 9;
        let index = 5;

        // Orphan
        let shred = new_test_data_shred(slot, 0);
        let request = ShredRepairType::Orphan(slot);
        assert!(request.verify_response(&shred));
        let shred = new_test_data_shred(slot - 1, 0);
        assert!(request.verify_response(&shred));
        let shred = new_test_data_shred(slot + 1, 0);
        assert!(!request.verify_response(&shred));

        // HighestShred
        let shred = new_test_data_shred(slot, index);
        let request = ShredRepairType::HighestShred(slot, index as u64);
        assert!(request.verify_response(&shred));
        let shred = new_test_data_shred(slot, index + 1);
        assert!(request.verify_response(&shred));
        let shred = new_test_data_shred(slot, index - 1);
        assert!(!request.verify_response(&shred));
        let shred = new_test_data_shred(slot - 1, index);
        assert!(!request.verify_response(&shred));
        let shred = new_test_data_shred(slot + 1, index);
        assert!(!request.verify_response(&shred));

        // Shred
        let shred = new_test_data_shred(slot, index);
        let request = ShredRepairType::Shred(slot, index as u64);
        assert!(request.verify_response(&shred));
        let shred = new_test_data_shred(slot, index + 1);
        assert!(!request.verify_response(&shred));
        let shred = new_test_data_shred(slot + 1, index);
        assert!(!request.verify_response(&shred));
    }

    fn verify_responses<'a>(request: &ShredRepairType, packets: impl Iterator<Item = &'a Packet>) {
        for packet in packets {
            let shred_payload = packet.data(..).unwrap().to_vec();
            let shred = Shred::new_from_serialized_shred(shred_payload).unwrap();
            request.verify_response(&shred);
        }
    }

    #[test]
    fn test_verify_ancestor_response() {
        let request_slot = MAX_ANCESTOR_RESPONSES as Slot;
        let repair = AncestorHashesRepairType(request_slot);
        let mut response: Vec<(Slot, Hash)> = (0..request_slot)
            .map(|slot| (slot, Hash::new_unique()))
            .collect();
        assert!(repair.verify_response(&AncestorHashesResponse::Hashes(response.clone())));

        // over the allowed limit, should fail
        response.push((request_slot, Hash::new_unique()));
        assert!(!repair.verify_response(&AncestorHashesResponse::Hashes(response)));
    }
}
