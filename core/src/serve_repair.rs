use {
    crate::{
        cluster_slots::ClusterSlots,
        duplicate_repair_status::ANCESTOR_HASH_REPAIR_SAMPLE_SIZE,
        packet_threshold::DynamicPacketToProcessThreshold,
        repair_response,
        repair_service::{OutstandingShredRepairs, RepairStats},
        request_response::RequestResponse,
        result::{Error, Result},
    },
    bincode::serialize,
    lru::LruCache,
    rand::{
        distributions::{Distribution, WeightedError, WeightedIndex},
        Rng,
    },
    solana_gossip::{
        cluster_info::{ClusterInfo, ClusterInfoError},
        contact_info::ContactInfo,
        ping_pong::{self, PingCache, Pong},
        weighted_shuffle::WeightedShuffle,
    },
    solana_ledger::{
        ancestor_iterator::{AncestorIterator, AncestorIteratorWithHash},
        blockstore::Blockstore,
        shred::{Nonce, Shred, ShredFetchStats, SIZE_OF_NONCE},
    },
    solana_metrics::inc_new_counter_debug,
    solana_perf::packet::{Packet, PacketBatch, PacketBatchRecycler},
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        clock::Slot,
        feature_set::{check_ping_ancestor_requests, sign_repair_requests},
        hash::{Hash, HASH_BYTES},
        packet::PACKET_DATA_SIZE,
        pubkey::{Pubkey, PUBKEY_BYTES},
        signature::{Signable, Signature, Signer, SIGNATURE_BYTES},
        signer::keypair::Keypair,
        stake_history::Epoch,
        timing::{duration_as_ms, timestamp},
    },
    solana_streamer::{
        sendmmsg::{batch_send, SendPktsError},
        streamer::{PacketBatchReceiver, PacketBatchSender},
    },
    std::{
        collections::HashSet,
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

type SlotHash = (Slot, Hash);

/// the number of slots to respond with when responding to `Orphan` requests
pub const MAX_ORPHAN_REPAIR_RESPONSES: usize = 10;
// Number of slots to cache their respective repair peers and sampling weights.
pub(crate) const REPAIR_PEERS_CACHE_CAPACITY: usize = 128;
// Limit cache entries ttl in order to avoid re-using outdated data.
const REPAIR_PEERS_CACHE_TTL: Duration = Duration::from_secs(10);
pub const MAX_ANCESTOR_BYTES_IN_PACKET: usize =
    PACKET_DATA_SIZE -
    SIZE_OF_NONCE -
    4 /*(response version enum discriminator)*/ -
    4 /*slot_hash length*/;
pub const MAX_ANCESTOR_RESPONSES: usize =
    MAX_ANCESTOR_BYTES_IN_PACKET / std::mem::size_of::<SlotHash>();
/// Number of bytes in the randomly generated token sent with ping messages.
pub(crate) const REPAIR_PING_TOKEN_SIZE: usize = HASH_BYTES;
pub const REPAIR_PING_CACHE_CAPACITY: usize = 65536;
pub const REPAIR_PING_CACHE_TTL: Duration = Duration::from_secs(1280);
pub(crate) const REPAIR_RESPONSE_SERIALIZED_PING_BYTES: usize =
    4 /*enum discriminator*/ + PUBKEY_BYTES + REPAIR_PING_TOKEN_SIZE + SIGNATURE_BYTES;
const SIGNED_REPAIR_TIME_WINDOW: Duration = Duration::from_secs(60 * 10); // 10 min

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_ANCESTOR_RESPONSES, 30);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum ShredRepairType {
    Orphan(Slot),
    HighestShred(Slot, u64),
    Shred(Slot, u64),
}

impl ShredRepairType {
    pub fn slot(&self) -> Slot {
        match self {
            ShredRepairType::Orphan(slot) => *slot,
            ShredRepairType::HighestShred(slot, _) => *slot,
            ShredRepairType::Shred(slot, _) => *slot,
        }
    }
}

impl RequestResponse for ShredRepairType {
    type Response = Shred;
    fn num_expected_responses(&self) -> u32 {
        match self {
            ShredRepairType::Orphan(_) => (MAX_ORPHAN_REPAIR_RESPONSES + 1) as u32, // run_orphan uses <= MAX_ORPHAN_REPAIR_RESPONSES
            ShredRepairType::HighestShred(_, _) => 1,
            ShredRepairType::Shred(_, _) => 1,
        }
    }
    fn verify_response(&self, response_shred: &Shred) -> bool {
        match self {
            ShredRepairType::Orphan(slot) => response_shred.slot() <= *slot,
            ShredRepairType::HighestShred(slot, index) => {
                response_shred.slot() as u64 == *slot && response_shred.index() as u64 >= *index
            }
            ShredRepairType::Shred(slot, index) => {
                response_shred.slot() as u64 == *slot && response_shred.index() as u64 == *index
            }
        }
    }
}

pub struct AncestorHashesRepairType(pub Slot);
impl AncestorHashesRepairType {
    pub fn slot(&self) -> Slot {
        self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AncestorHashesResponse {
    Hashes(Vec<SlotHash>),
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
    dropped_requests: usize,
    total_response_packets: usize,
    processed: usize,
    self_repair: usize,
    window_index: usize,
    highest_window_index: usize,
    orphan: usize,
    ancestor_hashes: usize,
    pong: usize,
    pings_required: usize,
    err_time_skew: usize,
    err_malformed: usize,
    err_sig_verify: usize,
    err_unsigned: usize,
    err_id_mismatch: usize,
}

#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize, Debug)]
pub enum RepairProtocol {
    LegacyWindowIndex(ContactInfo, Slot, u64),
    LegacyHighestWindowIndex(ContactInfo, Slot, u64),
    LegacyOrphan(ContactInfo, Slot),
    LegacyWindowIndexWithNonce(ContactInfo, Slot, u64, Nonce),
    LegacyHighestWindowIndexWithNonce(ContactInfo, Slot, u64, Nonce),
    LegacyOrphanWithNonce(ContactInfo, Slot, Nonce),
    LegacyAncestorHashes(ContactInfo, Slot, Nonce),
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

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum RepairResponse {
    Ping(Ping),
}

impl RepairProtocol {
    fn sender(&self) -> &Pubkey {
        match self {
            Self::LegacyWindowIndex(ci, _, _) => &ci.id,
            Self::LegacyHighestWindowIndex(ci, _, _) => &ci.id,
            Self::LegacyOrphan(ci, _) => &ci.id,
            Self::LegacyWindowIndexWithNonce(ci, _, _, _) => &ci.id,
            Self::LegacyHighestWindowIndexWithNonce(ci, _, _, _) => &ci.id,
            Self::LegacyOrphanWithNonce(ci, _, _) => &ci.id,
            Self::LegacyAncestorHashes(ci, _, _) => &ci.id,
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
}

#[derive(Clone)]
pub struct ServeRepair {
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
}

// Cache entry for repair peers for a slot.
pub(crate) struct RepairPeers {
    asof: Instant,
    peers: Vec<(Pubkey, /*ContactInfo.serve_repair:*/ SocketAddr)>,
    weighted_index: WeightedIndex<u64>,
}

impl RepairPeers {
    fn new(asof: Instant, peers: &[ContactInfo], weights: &[u64]) -> Result<Self> {
        if peers.is_empty() {
            return Err(Error::from(ClusterInfoError::NoPeers));
        }
        if peers.len() != weights.len() {
            return Err(Error::from(WeightedError::InvalidWeight));
        }
        let weighted_index = WeightedIndex::new(weights)?;
        let peers = peers
            .iter()
            .map(|peer| (peer.id, peer.serve_repair))
            .collect();
        Ok(Self {
            asof,
            peers,
            weighted_index,
        })
    }

    fn sample<R: Rng>(&self, rng: &mut R) -> (Pubkey, SocketAddr) {
        let index = self.weighted_index.sample(rng);
        self.peers[index]
    }
}

impl ServeRepair {
    pub fn new(cluster_info: Arc<ClusterInfo>, bank_forks: Arc<RwLock<BankForks>>) -> Self {
        Self {
            cluster_info,
            bank_forks,
        }
    }

    fn my_info(&self) -> ContactInfo {
        self.cluster_info.my_contact_info()
    }

    pub(crate) fn my_id(&self) -> Pubkey {
        self.cluster_info.id()
    }

    fn handle_repair(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
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
                }
                | RepairProtocol::LegacyWindowIndexWithNonce(_, slot, shred_index, nonce) => {
                    stats.window_index += 1;
                    (
                        Self::run_window_request(
                            recycler,
                            from_addr,
                            blockstore,
                            *slot,
                            *shred_index,
                            *nonce,
                        ),
                        "WindowIndexWithNonce",
                    )
                }
                RepairProtocol::HighestWindowIndex {
                    header: RepairRequestHeader { nonce, .. },
                    slot,
                    shred_index: highest_index,
                }
                | RepairProtocol::LegacyHighestWindowIndexWithNonce(
                    _,
                    slot,
                    highest_index,
                    nonce,
                ) => {
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
                }
                | RepairProtocol::LegacyOrphanWithNonce(_, slot, nonce) => {
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
                }
                | RepairProtocol::LegacyAncestorHashes(_, slot, nonce) => {
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
                | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
                | RepairProtocol::LegacyOrphan(_, _) => (None, "Unsupported repair type"),
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

    pub(crate) fn sign_repair_requests_activated_epoch(root_bank: &Bank) -> Option<Epoch> {
        root_bank
            .feature_set
            .activated_slot(&sign_repair_requests::id())
            .map(|slot| root_bank.epoch_schedule().get_epoch(slot))
    }

    pub(crate) fn should_sign_repair_request(
        slot: Slot,
        root_bank: &Bank,
        sign_repairs_epoch: Option<Epoch>,
    ) -> bool {
        match sign_repairs_epoch {
            None => false,
            Some(feature_epoch) => feature_epoch < root_bank.epoch_schedule().get_epoch(slot),
        }
    }

    fn check_ping_ancestor_requests_activated_epoch(root_bank: &Bank) -> Option<Epoch> {
        root_bank
            .feature_set
            .activated_slot(&check_ping_ancestor_requests::id())
            .map(|slot| root_bank.epoch_schedule().get_epoch(slot))
    }

    fn should_check_ping_ancestor_request(
        slot: Slot,
        root_bank: &Bank,
        check_ping_ancestor_request_epoch: Option<Epoch>,
    ) -> bool {
        match check_ping_ancestor_request_epoch {
            None => false,
            Some(feature_epoch) => feature_epoch < root_bank.epoch_schedule().get_epoch(slot),
        }
    }

    /// Process messages from the network
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        ping_cache: &mut PingCache,
        recycler: &PacketBatchRecycler,
        blockstore: Option<&Arc<Blockstore>>,
        requests_receiver: &PacketBatchReceiver,
        response_sender: &PacketBatchSender,
        stats: &mut ServeRepairStats,
        packet_threshold: &mut DynamicPacketToProcessThreshold,
    ) -> Result<()> {
        //TODO cache connections
        let timeout = Duration::new(1, 0);
        let mut reqs_v = vec![requests_receiver.recv_timeout(timeout)?];
        let mut total_requests = reqs_v[0].len();

        let mut dropped_requests = 0;
        while let Ok(more) = requests_receiver.try_recv() {
            total_requests += more.len();
            if packet_threshold.should_drop(total_requests) {
                dropped_requests += more.len();
            } else {
                reqs_v.push(more);
            }
        }

        stats.dropped_requests += dropped_requests;
        stats.total_requests += total_requests;

        let timer = Instant::now();
        let root_bank = obj.read().unwrap().bank_forks.read().unwrap().root_bank();
        for reqs in reqs_v {
            Self::handle_packets(
                obj,
                ping_cache,
                recycler,
                blockstore,
                reqs,
                response_sender,
                &root_bank,
                stats,
            );
        }
        packet_threshold.update(total_requests, timer.elapsed());
        Ok(())
    }

    fn report_reset_stats(me: &Arc<RwLock<Self>>, stats: &mut ServeRepairStats) {
        if stats.self_repair > 0 {
            let my_id = me.read().unwrap().cluster_info.id();
            warn!(
                "{}: Ignored received repair requests from ME: {}",
                my_id, stats.self_repair,
            );
            inc_new_counter_debug!("serve_repair-handle-repair--eq", stats.self_repair);
        }

        datapoint_info!(
            "serve_repair-requests_received",
            ("total_requests", stats.total_requests, i64),
            ("dropped_requests", stats.dropped_requests, i64),
            ("total_response_packets", stats.total_response_packets, i64),
            ("self_repair", stats.self_repair, i64),
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
            ("pings_required", stats.pings_required, i64),
            ("err_time_skew", stats.err_time_skew, i64),
            ("err_malformed", stats.err_malformed, i64),
            ("err_sig_verify", stats.err_sig_verify, i64),
            ("err_unsigned", stats.err_unsigned, i64),
            ("err_id_mismatch", stats.err_id_mismatch, i64),
        );

        *stats = ServeRepairStats::default();
    }

    pub fn listen(
        me: Arc<RwLock<Self>>,
        blockstore: Option<Arc<Blockstore>>,
        requests_receiver: PacketBatchReceiver,
        response_sender: PacketBatchSender,
        exit: &Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let mut ping_cache = PingCache::new(REPAIR_PING_CACHE_TTL, REPAIR_PING_CACHE_CAPACITY);
        let exit = exit.clone();
        let recycler = PacketBatchRecycler::default();
        Builder::new()
            .name("solana-repair-listen".to_string())
            .spawn(move || {
                let mut last_print = Instant::now();
                let mut stats = ServeRepairStats::default();
                let mut packet_threshold = DynamicPacketToProcessThreshold::default();
                loop {
                    let result = Self::run_listen(
                        &me,
                        &mut ping_cache,
                        &recycler,
                        blockstore.as_ref(),
                        &requests_receiver,
                        &response_sender,
                        &mut stats,
                        &mut packet_threshold,
                    );
                    match result {
                        Err(Error::RecvTimeout(_)) | Ok(_) => {}
                        Err(err) => info!("repair listener error: {:?}", err),
                    };
                    if exit.load(Ordering::Relaxed) {
                        return;
                    }
                    if last_print.elapsed().as_secs() > 2 {
                        Self::report_reset_stats(&me, &mut stats);
                        last_print = Instant::now();
                    }
                }
            })
            .unwrap()
    }

    fn verify_signed_packet(
        my_id: &Pubkey,
        packet: &Packet,
        request: &RepairProtocol,
        stats: &mut ServeRepairStats,
    ) -> bool {
        match request {
            RepairProtocol::LegacyWindowIndex(_, _, _)
            | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
            | RepairProtocol::LegacyOrphan(_, _)
            | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
            | RepairProtocol::LegacyAncestorHashes(_, _, _) => {
                debug_assert!(false); // expecting only signed request types
                stats.err_unsigned += 1;
                return false;
            }
            RepairProtocol::Pong(pong) => {
                if !pong.verify() {
                    stats.err_sig_verify += 1;
                    return false;
                }
            }
            RepairProtocol::WindowIndex { header, .. }
            | RepairProtocol::HighestWindowIndex { header, .. }
            | RepairProtocol::Orphan { header, .. }
            | RepairProtocol::AncestorHashes { header, .. } => {
                if &header.recipient != my_id {
                    stats.err_id_mismatch += 1;
                    return false;
                }
                let time_diff_ms = {
                    let ts = timestamp();
                    if ts < header.timestamp {
                        header.timestamp - ts
                    } else {
                        ts - header.timestamp
                    }
                };
                if u128::from(time_diff_ms) > SIGNED_REPAIR_TIME_WINDOW.as_millis() {
                    stats.err_time_skew += 1;
                    return false;
                }
                let leading_buf = match packet.data(..4) {
                    Some(buf) => buf,
                    None => {
                        debug_assert!(false); // should have failed deserialize
                        stats.err_malformed += 1;
                        return false;
                    }
                };
                let trailing_buf = match packet.data(4 + SIGNATURE_BYTES..) {
                    Some(buf) => buf,
                    None => {
                        debug_assert!(false); // should have failed deserialize
                        stats.err_malformed += 1;
                        return false;
                    }
                };
                let from_id = request.sender();
                let signed_data = [leading_buf, trailing_buf].concat();
                if !header.signature.verify(from_id.as_ref(), &signed_data) {
                    stats.err_sig_verify += 1;
                    return false;
                }
            }
        }
        true
    }

    fn check_ping_cache(
        request: &RepairProtocol,
        from_addr: &SocketAddr,
        identity_keypair: &Keypair,
        ping_cache: &mut PingCache,
    ) -> (bool, Option<Ping>) {
        let mut rng = rand::thread_rng();
        let mut pingf = move || Ping::new_rand(&mut rng, identity_keypair).ok();
        ping_cache.check(Instant::now(), (*request.sender(), *from_addr), &mut pingf)
    }

    fn requires_signature_check(
        request: &RepairProtocol,
        root_bank: &Bank,
        sign_repairs_epoch: Option<Epoch>,
    ) -> bool {
        match request {
            RepairProtocol::LegacyWindowIndex(_, slot, _)
            | RepairProtocol::LegacyHighestWindowIndex(_, slot, _)
            | RepairProtocol::LegacyOrphan(_, slot)
            | RepairProtocol::LegacyWindowIndexWithNonce(_, slot, _, _)
            | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, slot, _, _)
            | RepairProtocol::LegacyOrphanWithNonce(_, slot, _)
            | RepairProtocol::LegacyAncestorHashes(_, slot, _)
            | RepairProtocol::WindowIndex { slot, .. }
            | RepairProtocol::HighestWindowIndex { slot, .. }
            | RepairProtocol::Orphan { slot, .. }
            | RepairProtocol::AncestorHashes { slot, .. } => {
                Self::should_sign_repair_request(*slot, root_bank, sign_repairs_epoch)
            }
            RepairProtocol::Pong(_) => true,
        }
    }

    fn ping_to_packet_mapper_by_request_variant(
        request: &RepairProtocol,
        dest_addr: SocketAddr,
        root_bank: &Bank,
        check_ping_ancestor_request_epoch: Option<Epoch>,
    ) -> Option<Box<dyn FnOnce(Ping) -> Option<Packet>>> {
        match request {
            RepairProtocol::LegacyWindowIndex(_, _, _)
            | RepairProtocol::LegacyHighestWindowIndex(_, _, _)
            | RepairProtocol::LegacyOrphan(_, _)
            | RepairProtocol::LegacyWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyHighestWindowIndexWithNonce(_, _, _, _)
            | RepairProtocol::LegacyOrphanWithNonce(_, _, _)
            | RepairProtocol::LegacyAncestorHashes(_, _, _)
            | RepairProtocol::Pong(_) => None,
            RepairProtocol::WindowIndex { .. }
            | RepairProtocol::HighestWindowIndex { .. }
            | RepairProtocol::Orphan { .. } => Some(Box::new(move |ping| {
                let ping = RepairResponse::Ping(ping);
                Packet::from_data(Some(&dest_addr), ping).ok()
            })),
            RepairProtocol::AncestorHashes { slot, .. } => {
                if Self::should_check_ping_ancestor_request(
                    *slot,
                    root_bank,
                    check_ping_ancestor_request_epoch,
                ) {
                    Some(Box::new(move |ping| {
                        let ping = AncestorHashesResponse::Ping(ping);
                        Packet::from_data(Some(&dest_addr), ping).ok()
                    }))
                } else {
                    None
                }
            }
        }
    }

    fn handle_packets(
        me: &Arc<RwLock<Self>>,
        ping_cache: &mut PingCache,
        recycler: &PacketBatchRecycler,
        blockstore: Option<&Arc<Blockstore>>,
        packet_batch: PacketBatch,
        response_sender: &PacketBatchSender,
        root_bank: &Bank,
        stats: &mut ServeRepairStats,
    ) {
        let sign_repairs_epoch = Self::sign_repair_requests_activated_epoch(root_bank);
        let check_ping_ancestor_request_epoch =
            Self::check_ping_ancestor_requests_activated_epoch(root_bank);
        let (identity_keypair, socket_addr_space) = {
            let me_r = me.read().unwrap();
            let keypair = me_r.cluster_info.keypair().clone();
            let socket_addr_space = *me_r.cluster_info.socket_addr_space();
            (keypair, socket_addr_space)
        };
        let my_id = identity_keypair.pubkey();
        let mut pending_pings = Vec::default();

        // iter over the packets
        for packet in packet_batch.iter() {
            let request: RepairProtocol = match packet.deserialize_slice(..) {
                Ok(request) => request,
                Err(_) => {
                    stats.err_malformed += 1;
                    continue;
                }
            };

            if request.sender() == &my_id {
                stats.self_repair += 1;
                continue;
            }

            let require_signature_check =
                Self::requires_signature_check(&request, root_bank, sign_repairs_epoch);
            if require_signature_check && !request.supports_signature() {
                stats.err_unsigned += 1;
                continue;
            }
            if request.supports_signature()
                && !Self::verify_signed_packet(&my_id, packet, &request, stats)
            {
                continue;
            }

            let from_addr = packet.meta.socket_addr();
            if let Some(ping_to_pkt) = Self::ping_to_packet_mapper_by_request_variant(
                &request,
                from_addr,
                root_bank,
                check_ping_ancestor_request_epoch,
            ) {
                if !ContactInfo::is_valid_address(&from_addr, &socket_addr_space) {
                    stats.err_malformed += 1;
                    continue;
                }
                let (check, ping) =
                    Self::check_ping_cache(&request, &from_addr, &identity_keypair, ping_cache);
                if let Some(ping) = ping {
                    if let Some(pkt) = ping_to_pkt(ping) {
                        pending_pings.push(pkt);
                    }
                }
                if !check {
                    stats.pings_required += 1;
                    continue;
                }
            }

            stats.processed += 1;
            let rsp =
                Self::handle_repair(recycler, &from_addr, blockstore, request, stats, ping_cache);
            stats.total_response_packets += rsp.as_ref().map(PacketBatch::len).unwrap_or(0);
            if let Some(rsp) = rsp {
                let _ignore_disconnect = response_sender.send(rsp);
            }
        }

        if !pending_pings.is_empty() {
            let batch = PacketBatch::new(pending_pings);
            let _ignore = response_sender.send(batch);
        }
    }

    pub fn ancestor_repair_request_bytes(
        &self,
        keypair: &Keypair,
        root_bank: &Bank,
        repair_peer_id: &Pubkey,
        request_slot: Slot,
        nonce: Nonce,
    ) -> Result<Vec<u8>> {
        let sign_repairs_epoch = Self::sign_repair_requests_activated_epoch(root_bank);
        let require_sig =
            Self::should_sign_repair_request(request_slot, root_bank, sign_repairs_epoch);

        let (request_proto, maybe_keypair) = if require_sig {
            let header = RepairRequestHeader {
                signature: Signature::default(),
                sender: self.my_id(),
                recipient: *repair_peer_id,
                timestamp: timestamp(),
                nonce,
            };
            (
                RepairProtocol::AncestorHashes {
                    header,
                    slot: request_slot,
                },
                Some(keypair),
            )
        } else {
            (
                RepairProtocol::LegacyAncestorHashes(self.my_info(), request_slot, nonce),
                None,
            )
        };

        Self::repair_proto_to_bytes(&request_proto, maybe_keypair)
    }

    pub(crate) fn repair_request(
        &self,
        cluster_slots: &ClusterSlots,
        repair_request: ShredRepairType,
        peers_cache: &mut LruCache<Slot, RepairPeers>,
        repair_stats: &mut RepairStats,
        repair_validators: &Option<HashSet<Pubkey>>,
        outstanding_requests: &mut OutstandingShredRepairs,
        identity_keypair: Option<&Keypair>,
    ) -> Result<(SocketAddr, Vec<u8>)> {
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
        let (peer, addr) = repair_peers.sample(&mut rand::thread_rng());
        let nonce = outstanding_requests.add_request(repair_request, timestamp());
        let out = self.map_repair_request(
            &repair_request,
            &peer,
            repair_stats,
            nonce,
            identity_keypair,
        )?;
        Ok((addr, out))
    }

    pub(crate) fn repair_request_ancestor_hashes_sample_peers(
        &self,
        slot: Slot,
        cluster_slots: &ClusterSlots,
        repair_validators: &Option<HashSet<Pubkey>>,
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
            .take(ANCESTOR_HASH_REPAIR_SAMPLE_SIZE)
            .map(|i| index[i])
            .map(|i| (repair_peers[i].id, repair_peers[i].serve_repair))
            .collect();
        Ok(peers)
    }

    pub fn repair_request_duplicate_compute_best_peer(
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
        Ok((repair_peers[n].id, repair_peers[n].serve_repair))
    }

    pub(crate) fn map_repair_request(
        &self,
        repair_request: &ShredRepairType,
        repair_peer_id: &Pubkey,
        repair_stats: &mut RepairStats,
        nonce: Nonce,
        identity_keypair: Option<&Keypair>,
    ) -> Result<Vec<u8>> {
        let header = if identity_keypair.is_some() {
            Some(RepairRequestHeader {
                signature: Signature::default(),
                sender: self.my_id(),
                recipient: *repair_peer_id,
                timestamp: timestamp(),
                nonce,
            })
        } else {
            None
        };
        let request_proto = match repair_request {
            ShredRepairType::Shred(slot, shred_index) => {
                repair_stats
                    .shred
                    .update(repair_peer_id, *slot, *shred_index);
                if let Some(header) = header {
                    RepairProtocol::WindowIndex {
                        header,
                        slot: *slot,
                        shred_index: *shred_index,
                    }
                } else {
                    RepairProtocol::LegacyWindowIndexWithNonce(
                        self.my_info(),
                        *slot,
                        *shred_index,
                        nonce,
                    )
                }
            }
            ShredRepairType::HighestShred(slot, shred_index) => {
                repair_stats
                    .highest_shred
                    .update(repair_peer_id, *slot, *shred_index);
                if let Some(header) = header {
                    RepairProtocol::HighestWindowIndex {
                        header,
                        slot: *slot,
                        shred_index: *shred_index,
                    }
                } else {
                    RepairProtocol::LegacyHighestWindowIndexWithNonce(
                        self.my_info(),
                        *slot,
                        *shred_index,
                        nonce,
                    )
                }
            }
            ShredRepairType::Orphan(slot) => {
                repair_stats.orphan.update(repair_peer_id, *slot, 0);
                if let Some(header) = header {
                    RepairProtocol::Orphan {
                        header,
                        slot: *slot,
                    }
                } else {
                    RepairProtocol::LegacyOrphanWithNonce(self.my_info(), *slot, nonce)
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
            if packet.meta.size != REPAIR_RESPONSE_SERIALIZED_PING_BYTES {
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
                packet.meta.set_discard(true);
                stats.ping_count += 1;
                if let Ok(pong) = Pong::new(&ping, keypair) {
                    let pong = RepairProtocol::Pong(pong);
                    if let Ok(pong_bytes) = serialize(&pong) {
                        let from_addr = packet.meta.socket_addr();
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

    pub fn repair_proto_to_bytes(
        request: &RepairProtocol,
        keypair: Option<&Keypair>,
    ) -> Result<Vec<u8>> {
        let mut payload = serialize(&request)?;
        if let Some(keypair) = keypair {
            debug_assert!(request.supports_signature());
            let signable_data = [&payload[..4], &payload[4 + SIGNATURE_BYTES..]].concat();
            let signature = keypair.sign_message(&signable_data[..]);
            payload[4..4 + SIGNATURE_BYTES].copy_from_slice(signature.as_ref());
        }
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
        blockstore: Option<&Arc<Blockstore>>,
        slot: Slot,
        shred_index: u64,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        if let Some(blockstore) = blockstore {
            // Try to find the requested index in one of the slots
            let packet = repair_response::repair_response_packet(
                blockstore,
                slot,
                shred_index,
                from_addr,
                nonce,
            );

            if let Some(packet) = packet {
                inc_new_counter_debug!("serve_repair-window-request-ledger", 1);
                return Some(PacketBatch::new_unpinned_with_recycler_data(
                    recycler,
                    "run_window_request",
                    vec![packet],
                ));
            }
        }

        inc_new_counter_debug!("serve_repair-window-request-fail", 1);
        None
    }

    fn run_highest_window_request(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        slot: Slot,
        highest_index: u64,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        let blockstore = blockstore?;
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
        blockstore: Option<&Arc<Blockstore>>,
        mut slot: Slot,
        max_responses: usize,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        let mut res = PacketBatch::new_unpinned_with_recycler(recycler.clone(), 64, "run_orphan");
        if let Some(blockstore) = blockstore {
            // Try to find the next "n" parent slots of the input slot
            while let Ok(Some(meta)) = blockstore.meta(slot) {
                if meta.received == 0 {
                    break;
                }
                let packet = repair_response::repair_response_packet(
                    blockstore,
                    slot,
                    meta.received - 1,
                    from_addr,
                    nonce,
                );
                if let Some(packet) = packet {
                    res.push(packet);
                } else {
                    break;
                }

                if meta.parent_slot.is_some() && res.len() <= max_responses {
                    slot = meta.parent_slot.unwrap();
                } else {
                    break;
                }
            }
        }
        if res.is_empty() {
            return None;
        }
        Some(res)
    }

    fn run_ancestor_hashes(
        recycler: &PacketBatchRecycler,
        from_addr: &SocketAddr,
        blockstore: Option<&Arc<Blockstore>>,
        slot: Slot,
        nonce: Nonce,
    ) -> Option<PacketBatch> {
        let blockstore = blockstore?;
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{repair_response, result::Error},
        bincode::deserialize_from,
        solana_gossip::{socketaddr, socketaddr_any},
        solana_ledger::{
            blockstore::make_many_slot_entries,
            blockstore_processor::fill_blockstore_slot_with_ticks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path,
            shred::{max_ticks_per_n_shreds, Shred, Shredder},
        },
        solana_perf::packet::Packet,
        solana_runtime::bank::Bank,
        solana_sdk::{
            feature_set::FeatureSet, hash::Hash, pubkey::Pubkey, signature::Keypair,
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::io::Cursor,
    };

    #[test]
    fn test_serialized_ping_size() {
        let mut rng = rand::thread_rng();
        let keypair = Keypair::new();
        let ping = Ping::new_rand(&mut rng, &keypair).unwrap();
        let ping = RepairResponse::Ping(ping);
        let pkt = Packet::from_data(None, ping).unwrap();
        assert_eq!(pkt.meta.size, REPAIR_RESPONSE_SERIALIZED_PING_BYTES);
    }

    #[test]
    fn test_deserialize_shred_as_ping() {
        let data_buf = vec![7u8, 44]; // REPAIR_RESPONSE_SERIALIZED_PING_BYTES - SIZE_OF_DATA_SHRED_HEADERS
        let keypair = Keypair::new();
        let mut shred = Shred::new_from_data(
            123, // slot
            456, // index
            111, // parent_offset
            Some(&data_buf),
            true, // is_last_data
            true, // is_last_in_slot
            222,  // reference_tick
            333,  // version
            444,  // fec_set_index
        );
        Shredder::sign_shred(&keypair, &mut shred);
        let mut pkt = Packet::default();
        shred.copy_to_packet(&mut pkt);
        pkt.meta.size = REPAIR_RESPONSE_SERIALIZED_PING_BYTES;
        let res = pkt.deserialize_slice::<RepairResponse, _>(..);
        if let Ok(RepairResponse::Ping(ping)) = res {
            assert!(!ping.verify());
        } else {
            assert!(res.is_err());
        }
    }

    #[test]
    fn test_serialize_deserialize_signed_request() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(new_test_cluster_info(me));
        let serve_repair = ServeRepair::new(cluster_info.clone(), bank_forks);
        let keypair = cluster_info.keypair().clone();
        let repair_peer_id = solana_sdk::pubkey::new_rand();
        let repair_request = ShredRepairType::Orphan(123);

        let rsp = serve_repair
            .map_repair_request(
                &repair_request,
                &repair_peer_id,
                &mut RepairStats::default(),
                456,
                Some(&keypair),
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
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
        let slot = 50;
        let nonce = 70;
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(new_test_cluster_info(me));
        let repair_peer_id = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let keypair = cluster_info.keypair().clone();

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let serve_repair = ServeRepair::new(cluster_info.clone(), bank_forks.clone());

        let root_bank = bank_forks.read().unwrap().root_bank();
        let request_bytes = serve_repair
            .ancestor_repair_request_bytes(&keypair, &root_bank, &repair_peer_id, slot, nonce)
            .unwrap();
        let mut cursor = Cursor::new(&request_bytes[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
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

        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&sign_repair_requests::id());
        bank.feature_set = Arc::new(feature_set);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let serve_repair = ServeRepair::new(cluster_info, bank_forks.clone());

        let root_bank = bank_forks.read().unwrap().root_bank();
        let request_bytes = serve_repair
            .ancestor_repair_request_bytes(&keypair, &root_bank, &repair_peer_id, slot, nonce)
            .unwrap();
        let mut cursor = Cursor::new(&request_bytes[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
        assert_eq!(cursor.position(), request_bytes.len() as u64);
        if let RepairProtocol::LegacyAncestorHashes(ci, deserialized_slot, deserialized_nonce) =
            deserialized_request
        {
            assert_eq!(slot, deserialized_slot);
            assert_eq!(nonce, deserialized_nonce);
            assert_eq!(&serve_repair.my_id(), &ci.id);
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }
    }

    #[test]
    fn test_map_requests_signed_unsigned() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(new_test_cluster_info(me));
        let serve_repair = ServeRepair::new(cluster_info.clone(), bank_forks);
        let keypair = cluster_info.keypair().clone();
        let repair_peer_id = solana_sdk::pubkey::new_rand();

        let slot = 50;
        let shred_index = 60;
        let nonce = 70;

        let request = ShredRepairType::Shred(slot, shred_index);
        let rsp = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                Some(&keypair),
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
        assert_eq!(cursor.position(), rsp.len() as u64);
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
            let signed_data = [&rsp[..4], &rsp[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }

        let rsp = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                None,
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
        assert_eq!(cursor.position(), rsp.len() as u64);
        if let RepairProtocol::LegacyWindowIndexWithNonce(
            ci,
            deserialized_slot,
            deserialized_shred_index,
            deserialized_nonce,
        ) = deserialized_request
        {
            assert_eq!(slot, deserialized_slot);
            assert_eq!(shred_index, deserialized_shred_index);
            assert_eq!(nonce, deserialized_nonce);
            assert_eq!(&serve_repair.my_id(), &ci.id);
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }

        let request = ShredRepairType::HighestShred(slot, shred_index);
        let rsp = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                Some(&keypair),
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
        assert_eq!(cursor.position(), rsp.len() as u64);
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
            let signed_data = [&rsp[..4], &rsp[4 + SIGNATURE_BYTES..]].concat();
            assert!(header
                .signature
                .verify(keypair.pubkey().as_ref(), &signed_data));
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }

        let rsp = serve_repair
            .map_repair_request(
                &request,
                &repair_peer_id,
                &mut RepairStats::default(),
                nonce,
                None,
            )
            .unwrap();

        let mut cursor = Cursor::new(&rsp[..]);
        let deserialized_request: RepairProtocol = deserialize_from(&mut cursor).unwrap();
        assert_eq!(cursor.position(), rsp.len() as u64);
        if let RepairProtocol::LegacyHighestWindowIndexWithNonce(
            ci,
            deserialized_slot,
            deserialized_shred_index,
            deserialized_nonce,
        ) = deserialized_request
        {
            assert_eq!(slot, deserialized_slot);
            assert_eq!(shred_index, deserialized_shred_index);
            assert_eq!(nonce, deserialized_nonce);
            assert_eq!(&serve_repair.my_id(), &ci.id);
        } else {
            panic!("unexpected request type {:?}", &deserialized_request);
        }
    }

    #[test]
    fn test_verify_signed_packet() {
        let keypair = Keypair::new();
        let other_keypair = Keypair::new();
        let my_id = Pubkey::new_unique();
        let other_id = Pubkey::new_unique();

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
            let header = RepairRequestHeader::new(keypair.pubkey(), my_id, timestamp(), 678);
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, &request).unwrap();
            sign_packet(&mut packet, &keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        assert!(ServeRepair::verify_signed_packet(
            &my_id,
            &packet,
            &request,
            &mut ServeRepairStats::default(),
        ));

        // recipient mismatch
        let packet = {
            let header = RepairRequestHeader::new(keypair.pubkey(), other_id, timestamp(), 678);
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, &request).unwrap();
            sign_packet(&mut packet, &keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        let mut stats = ServeRepairStats::default();
        assert!(!ServeRepair::verify_signed_packet(
            &my_id, &packet, &request, &mut stats,
        ));
        assert_eq!(stats.err_id_mismatch, 1);

        // outside time window
        let packet = {
            let time_diff_ms = u64::try_from(SIGNED_REPAIR_TIME_WINDOW.as_millis() * 2).unwrap();
            let old_timestamp = timestamp().saturating_sub(time_diff_ms);
            let header = RepairRequestHeader::new(keypair.pubkey(), my_id, old_timestamp, 678);
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, &request).unwrap();
            sign_packet(&mut packet, &keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        let mut stats = ServeRepairStats::default();
        assert!(!ServeRepair::verify_signed_packet(
            &my_id, &packet, &request, &mut stats,
        ));
        assert_eq!(stats.err_time_skew, 1);

        // bad signature
        let packet = {
            let header = RepairRequestHeader::new(keypair.pubkey(), my_id, timestamp(), 678);
            let slot = 239847;
            let request = RepairProtocol::Orphan { header, slot };
            let mut packet = Packet::from_data(None, &request).unwrap();
            sign_packet(&mut packet, &other_keypair);
            packet
        };
        let request: RepairProtocol = packet.deserialize_slice(..).unwrap();
        let mut stats = ServeRepairStats::default();
        assert!(!ServeRepair::verify_signed_packet(
            &my_id, &packet, &request, &mut stats,
        ));
        assert_eq!(stats.err_sig_verify, 1);
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
                Some(&blockstore),
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
                Some(&blockstore),
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
                Some(&blockstore),
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
                Some(&blockstore),
                slot,
                0,
                nonce,
            );
            assert!(rv.is_none());
            let shred = Shred::new_from_data(slot, 1, 1, None, false, false, 0, 2, 0);

            blockstore
                .insert_shreds(vec![shred], None, false)
                .expect("Expect successful ledger write");

            let index = 1;
            let rv = ServeRepair::run_window_request(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
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

    fn new_test_cluster_info(contact_info: ContactInfo) -> ClusterInfo {
        ClusterInfo::new(
            contact_info,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        )
    }

    #[test]
    fn window_index_request() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let cluster_slots = ClusterSlots::default();
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(new_test_cluster_info(me));
        let serve_repair = ServeRepair::new(cluster_info.clone(), bank_forks);
        let mut outstanding_requests = OutstandingShredRepairs::default();
        let rv = serve_repair.repair_request(
            &cluster_slots,
            ShredRepairType::Shred(0, 0),
            &mut LruCache::new(100),
            &mut RepairStats::default(),
            &None,
            &mut outstanding_requests,
            None,
        );
        assert_matches!(rv, Err(Error::ClusterInfo(ClusterInfoError::NoPeers)));

        let serve_repair_addr = socketaddr!([127, 0, 0, 1], 1243);
        let nxt = ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip: socketaddr!([127, 0, 0, 1], 1234),
            tvu: socketaddr!([127, 0, 0, 1], 1235),
            tvu_forwards: socketaddr!([127, 0, 0, 1], 1236),
            repair: socketaddr!([127, 0, 0, 1], 1237),
            tpu: socketaddr!([127, 0, 0, 1], 1238),
            tpu_forwards: socketaddr!([127, 0, 0, 1], 1239),
            tpu_vote: socketaddr!([127, 0, 0, 1], 1240),
            rpc: socketaddr!([127, 0, 0, 1], 1241),
            rpc_pubsub: socketaddr!([127, 0, 0, 1], 1242),
            serve_repair: serve_repair_addr,
            wallclock: 0,
            shred_version: 0,
        };
        cluster_info.insert_info(nxt.clone());
        let rv = serve_repair
            .repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &None,
                &mut outstanding_requests,
                None,
            )
            .unwrap();
        assert_eq!(nxt.serve_repair, serve_repair_addr);
        assert_eq!(rv.0, nxt.serve_repair);

        let serve_repair_addr2 = socketaddr!([127, 0, 0, 2], 1243);
        let nxt = ContactInfo {
            id: solana_sdk::pubkey::new_rand(),
            gossip: socketaddr!([127, 0, 0, 1], 1234),
            tvu: socketaddr!([127, 0, 0, 1], 1235),
            tvu_forwards: socketaddr!([127, 0, 0, 1], 1236),
            repair: socketaddr!([127, 0, 0, 1], 1237),
            tpu: socketaddr!([127, 0, 0, 1], 1238),
            tpu_forwards: socketaddr!([127, 0, 0, 1], 1239),
            tpu_vote: socketaddr!([127, 0, 0, 1], 1240),
            rpc: socketaddr!([127, 0, 0, 1], 1241),
            rpc_pubsub: socketaddr!([127, 0, 0, 1], 1242),
            serve_repair: serve_repair_addr2,
            wallclock: 0,
            shred_version: 0,
        };
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
                    None,
                )
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
            let rv = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                slot,
                0,
                nonce,
            );
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
                Some(&blockstore),
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
                Some(&blockstore),
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

            // Make shred for slot 1 too large
            assert_eq!(shreds[0].slot(), 1);
            assert_eq!(shreds[0].index(), 0);
            shreds[0].payload.push(10);
            shreds[0].data_header.size = shreds[0].payload.len() as u16;
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
            let rv: Vec<_> = ServeRepair::run_orphan(
                &recycler,
                &socketaddr_any!(),
                Some(&blockstore),
                2,
                5,
                nonce,
            )
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
                .deserialize_slice(..packet.meta.size - SIZE_OF_NONCE)
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
                Some(&blockstore),
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
                Some(&blockstore),
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
                Some(&blockstore),
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
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let cluster_slots = ClusterSlots::default();
        let me = ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let cluster_info = Arc::new(new_test_cluster_info(me.clone()));

        // Insert two peers on the network
        let contact_info2 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        let contact_info3 =
            ContactInfo::new_localhost(&solana_sdk::pubkey::new_rand(), timestamp());
        cluster_info.insert_info(contact_info2.clone());
        cluster_info.insert_info(contact_info3.clone());
        let serve_repair = ServeRepair::new(cluster_info, bank_forks);

        // If:
        // 1) repair validator set doesn't exist in gossip
        // 2) repair validator set only includes our own id
        // then no repairs should be generated
        for pubkey in &[solana_sdk::pubkey::new_rand(), me.id] {
            let known_validators = Some(vec![*pubkey].into_iter().collect());
            assert!(serve_repair.repair_peers(&known_validators, 1).is_empty());
            assert!(serve_repair
                .repair_request(
                    &cluster_slots,
                    ShredRepairType::Shred(0, 0),
                    &mut LruCache::new(100),
                    &mut RepairStats::default(),
                    &known_validators,
                    &mut OutstandingShredRepairs::default(),
                    None,
                )
                .is_err());
        }

        // If known validator exists in gossip, should return repair successfully
        let known_validators = Some(vec![contact_info2.id].into_iter().collect());
        let repair_peers = serve_repair.repair_peers(&known_validators, 1);
        assert_eq!(repair_peers.len(), 1);
        assert_eq!(repair_peers[0].id, contact_info2.id);
        assert!(serve_repair
            .repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &known_validators,
                &mut OutstandingShredRepairs::default(),
                None,
            )
            .is_ok());

        // Using no known validators should default to all
        // validator's available in gossip, excluding myself
        let repair_peers: HashSet<Pubkey> = serve_repair
            .repair_peers(&None, 1)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(repair_peers.len(), 2);
        assert!(repair_peers.contains(&contact_info2.id));
        assert!(repair_peers.contains(&contact_info3.id));
        assert!(serve_repair
            .repair_request(
                &cluster_slots,
                ShredRepairType::Shred(0, 0),
                &mut LruCache::new(100),
                &mut RepairStats::default(),
                &None,
                &mut OutstandingShredRepairs::default(),
                None,
            )
            .is_ok());
    }

    #[test]
    fn test_verify_shred_response() {
        let repair = ShredRepairType::Orphan(9);
        // Ensure new options are addded to this test
        match repair {
            ShredRepairType::Orphan(_) => (),
            ShredRepairType::HighestShred(_, _) => (),
            ShredRepairType::Shred(_, _) => (),
        };

        let slot = 9;
        let index = 5;

        // Orphan
        let mut shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        let request = ShredRepairType::Orphan(slot);
        assert!(request.verify_response(&shred));
        shred.set_slot(slot - 1);
        assert!(request.verify_response(&shred));
        shred.set_slot(slot + 1);
        assert!(!request.verify_response(&shred));

        // HighestShred
        shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        shred.set_index(index);
        let request = ShredRepairType::HighestShred(slot, index as u64);
        assert!(request.verify_response(&shred));
        shred.set_index(index + 1);
        assert!(request.verify_response(&shred));
        shred.set_index(index - 1);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot - 1);
        shred.set_index(index);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot + 1);
        assert!(!request.verify_response(&shred));

        // Shred
        shred = Shred::new_empty_data_shred();
        shred.set_slot(slot);
        shred.set_index(index);
        let request = ShredRepairType::Shred(slot, index as u64);
        assert!(request.verify_response(&shred));
        shred.set_index(index + 1);
        assert!(!request.verify_response(&shred));
        shred.set_slot(slot + 1);
        shred.set_index(index);
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
        let mut response: Vec<SlotHash> = (0..request_slot)
            .into_iter()
            .map(|slot| (slot, Hash::new_unique()))
            .collect();
        assert!(repair.verify_response(&AncestorHashesResponse::Hashes(response.clone())));

        // over the allowed limit, should fail
        response.push((request_slot, Hash::new_unique()));
        assert!(!repair.verify_response(&AncestorHashesResponse::Hashes(response)));
    }
}
