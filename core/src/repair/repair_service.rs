//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing shreds in the ledger and sends repair requests for those shreds
#[cfg(test)]
use {
    crate::repair::duplicate_repair_status::DuplicateSlotRepairStatus,
    solana_sdk::clock::DEFAULT_MS_PER_SLOT,
};
use {
    crate::{
        cluster_info_vote_listener::VerifiedVoteReceiver,
        cluster_slots_service::cluster_slots::ClusterSlots,
        repair::{
            ancestor_hashes_service::{AncestorHashesReplayUpdateReceiver, AncestorHashesService},
            duplicate_repair_status::AncestorDuplicateSlotToRepair,
            outstanding_requests::OutstandingRequests,
            quic_endpoint::LocalRequest,
            repair_weight::RepairWeight,
            serve_repair::{self, ServeRepair, ShredRepairType, REPAIR_PEERS_CACHE_CAPACITY},
        },
    },
    crossbeam_channel::{Receiver as CrossbeamReceiver, Sender as CrossbeamSender},
    lru::LruCache,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{
        blockstore::{Blockstore, SlotMeta},
        shred,
    },
    solana_measure::measure::Measure,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{
        clock::{Slot, DEFAULT_TICKS_PER_SECOND, MS_PER_TICK},
        epoch_schedule::EpochSchedule,
        hash::Hash,
        pubkey::Pubkey,
        signer::keypair::Keypair,
        timing::timestamp,
    },
    solana_streamer::sendmmsg::{batch_send, SendPktsError},
    std::{
        collections::{HashMap, HashSet},
        iter::Iterator,
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    tokio::sync::mpsc::Sender as AsyncSender,
};

// Time to defer repair requests to allow for turbine propagation
const DEFER_REPAIR_THRESHOLD: Duration = Duration::from_millis(200);
const DEFER_REPAIR_THRESHOLD_TICKS: u64 = DEFER_REPAIR_THRESHOLD.as_millis() as u64 / MS_PER_TICK;

pub type AncestorDuplicateSlotsSender = CrossbeamSender<AncestorDuplicateSlotToRepair>;
pub type AncestorDuplicateSlotsReceiver = CrossbeamReceiver<AncestorDuplicateSlotToRepair>;
pub type ConfirmedSlotsSender = CrossbeamSender<Vec<Slot>>;
pub type ConfirmedSlotsReceiver = CrossbeamReceiver<Vec<Slot>>;
pub type DumpedSlotsSender = CrossbeamSender<Vec<(Slot, Hash)>>;
pub type DumpedSlotsReceiver = CrossbeamReceiver<Vec<(Slot, Hash)>>;
pub type OutstandingShredRepairs = OutstandingRequests<ShredRepairType>;
pub type PopularPrunedForksSender = CrossbeamSender<Vec<Slot>>;
pub type PopularPrunedForksReceiver = CrossbeamReceiver<Vec<Slot>>;

#[derive(Default, Debug)]
pub struct SlotRepairs {
    highest_shred_index: u64,
    // map from pubkey to total number of requests
    pubkey_repairs: HashMap<Pubkey, u64>,
}

impl SlotRepairs {
    pub fn pubkey_repairs(&self) -> &HashMap<Pubkey, u64> {
        &self.pubkey_repairs
    }
}

#[derive(Default, Debug)]
pub struct RepairStatsGroup {
    pub count: u64,
    pub min: u64,
    pub max: u64,
    pub slot_pubkeys: HashMap<Slot, SlotRepairs>,
}

impl RepairStatsGroup {
    pub fn update(&mut self, repair_peer_id: &Pubkey, slot: Slot, shred_index: u64) {
        self.count += 1;
        let slot_repairs = self.slot_pubkeys.entry(slot).or_default();
        // Increment total number of repairs of this type for this pubkey by 1
        *slot_repairs
            .pubkey_repairs
            .entry(*repair_peer_id)
            .or_default() += 1;
        // Update the max requested shred index for this slot
        slot_repairs.highest_shred_index =
            std::cmp::max(slot_repairs.highest_shred_index, shred_index);
        if self.min == 0 {
            self.min = slot;
        } else {
            self.min = std::cmp::min(self.min, slot);
        }
        self.max = std::cmp::max(self.max, slot);
    }
}

#[derive(Default, Debug)]
pub struct RepairStats {
    pub shred: RepairStatsGroup,
    pub highest_shred: RepairStatsGroup,
    pub orphan: RepairStatsGroup,
    pub get_best_orphans_us: u64,
    pub get_best_shreds_us: u64,
}

#[derive(Default, Debug)]
pub struct RepairTiming {
    pub set_root_elapsed: u64,
    pub dump_slots_elapsed: u64,
    pub get_votes_elapsed: u64,
    pub add_votes_elapsed: u64,
    pub get_best_orphans_elapsed: u64,
    pub get_best_shreds_elapsed: u64,
    pub get_unknown_last_index_elapsed: u64,
    pub get_closest_completion_elapsed: u64,
    pub send_repairs_elapsed: u64,
    pub build_repairs_batch_elapsed: u64,
    pub batch_send_repairs_elapsed: u64,
}

impl RepairTiming {
    fn update(
        &mut self,
        set_root_elapsed: u64,
        dump_slots_elapsed: u64,
        get_votes_elapsed: u64,
        add_votes_elapsed: u64,
        build_repairs_batch_elapsed: u64,
        batch_send_repairs_elapsed: u64,
    ) {
        self.set_root_elapsed += set_root_elapsed;
        self.dump_slots_elapsed += dump_slots_elapsed;
        self.get_votes_elapsed += get_votes_elapsed;
        self.add_votes_elapsed += add_votes_elapsed;
        self.build_repairs_batch_elapsed += build_repairs_batch_elapsed;
        self.batch_send_repairs_elapsed += batch_send_repairs_elapsed;
        self.send_repairs_elapsed += build_repairs_batch_elapsed + batch_send_repairs_elapsed;
    }
}

#[derive(Default, Debug)]
pub struct BestRepairsStats {
    pub call_count: u64,
    pub num_orphan_slots: u64,
    pub num_orphan_repairs: u64,
    pub num_best_shreds_slots: u64,
    pub num_best_shreds_repairs: u64,
    pub num_unknown_last_index_slots: u64,
    pub num_unknown_last_index_repairs: u64,
    pub num_closest_completion_slots: u64,
    pub num_closest_completion_slots_path: u64,
    pub num_closest_completion_repairs: u64,
    pub num_repair_trees: u64,
}

impl BestRepairsStats {
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        num_orphan_slots: u64,
        num_orphan_repairs: u64,
        num_best_shreds_slots: u64,
        num_best_shreds_repairs: u64,
        num_unknown_last_index_slots: u64,
        num_unknown_last_index_repairs: u64,
        num_closest_completion_slots: u64,
        num_closest_completion_slots_path: u64,
        num_closest_completion_repairs: u64,
        num_repair_trees: u64,
    ) {
        self.call_count += 1;
        self.num_orphan_slots += num_orphan_slots;
        self.num_orphan_repairs += num_orphan_repairs;
        self.num_best_shreds_slots += num_best_shreds_slots;
        self.num_best_shreds_repairs += num_best_shreds_repairs;
        self.num_unknown_last_index_slots += num_unknown_last_index_slots;
        self.num_unknown_last_index_repairs += num_unknown_last_index_repairs;
        self.num_closest_completion_slots += num_closest_completion_slots;
        self.num_closest_completion_slots_path += num_closest_completion_slots_path;
        self.num_closest_completion_repairs += num_closest_completion_repairs;
        self.num_repair_trees += num_repair_trees;
    }
}

pub const MAX_REPAIR_LENGTH: usize = 512;
pub const MAX_REPAIR_PER_DUPLICATE: usize = 20;
pub const MAX_DUPLICATE_WAIT_MS: usize = 10_000;
pub const REPAIR_MS: u64 = 100;
pub const MAX_ORPHANS: usize = 5;
pub const MAX_UNKNOWN_LAST_INDEX_REPAIRS: usize = 10;
pub const MAX_CLOSEST_COMPLETION_REPAIRS: usize = 100;

#[derive(Clone)]
pub struct RepairInfo {
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub cluster_info: Arc<ClusterInfo>,
    pub cluster_slots: Arc<ClusterSlots>,
    pub epoch_schedule: EpochSchedule,
    pub ancestor_duplicate_slots_sender: AncestorDuplicateSlotsSender,
    // Validators from which repairs are requested
    pub repair_validators: Option<HashSet<Pubkey>>,
    // Validators which should be given priority when serving
    pub repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,
}

pub struct RepairSlotRange {
    pub start: Slot,
    pub end: Slot,
}

impl Default for RepairSlotRange {
    fn default() -> Self {
        RepairSlotRange {
            start: 0,
            end: std::u64::MAX,
        }
    }
}

pub struct RepairService {
    t_repair: JoinHandle<()>,
    ancestor_hashes_service: AncestorHashesService,
}

impl RepairService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
        repair_socket: Arc<UdpSocket>,
        ancestor_hashes_socket: Arc<UdpSocket>,
        quic_endpoint_sender: AsyncSender<LocalRequest>,
        quic_endpoint_response_sender: CrossbeamSender<(SocketAddr, Vec<u8>)>,
        repair_info: RepairInfo,
        verified_vote_receiver: VerifiedVoteReceiver,
        outstanding_requests: Arc<RwLock<OutstandingShredRepairs>>,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
        dumped_slots_receiver: DumpedSlotsReceiver,
        popular_pruned_forks_sender: PopularPrunedForksSender,
    ) -> Self {
        let t_repair = {
            let blockstore = blockstore.clone();
            let exit = exit.clone();
            let repair_info = repair_info.clone();
            let quic_endpoint_sender = quic_endpoint_sender.clone();
            Builder::new()
                .name("solRepairSvc".to_string())
                .spawn(move || {
                    Self::run(
                        &blockstore,
                        &exit,
                        &repair_socket,
                        &quic_endpoint_sender,
                        &quic_endpoint_response_sender,
                        repair_info,
                        verified_vote_receiver,
                        &outstanding_requests,
                        dumped_slots_receiver,
                        popular_pruned_forks_sender,
                    )
                })
                .unwrap()
        };

        let ancestor_hashes_service = AncestorHashesService::new(
            exit,
            blockstore,
            ancestor_hashes_socket,
            quic_endpoint_sender,
            repair_info,
            ancestor_hashes_replay_update_receiver,
        );

        RepairService {
            t_repair,
            ancestor_hashes_service,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        blockstore: &Blockstore,
        exit: &AtomicBool,
        repair_socket: &UdpSocket,
        quic_endpoint_sender: &AsyncSender<LocalRequest>,
        quic_endpoint_response_sender: &CrossbeamSender<(SocketAddr, Vec<u8>)>,
        repair_info: RepairInfo,
        verified_vote_receiver: VerifiedVoteReceiver,
        outstanding_requests: &RwLock<OutstandingShredRepairs>,
        dumped_slots_receiver: DumpedSlotsReceiver,
        popular_pruned_forks_sender: PopularPrunedForksSender,
    ) {
        let mut repair_weight = RepairWeight::new(repair_info.bank_forks.read().unwrap().root());
        let serve_repair = ServeRepair::new(
            repair_info.cluster_info.clone(),
            repair_info.bank_forks.clone(),
            repair_info.repair_whitelist.clone(),
        );
        let id = repair_info.cluster_info.id();
        let mut repair_stats = RepairStats::default();
        let mut repair_timing = RepairTiming::default();
        let mut best_repairs_stats = BestRepairsStats::default();
        let mut last_stats = Instant::now();
        let mut peers_cache = LruCache::new(REPAIR_PEERS_CACHE_CAPACITY);
        let mut popular_pruned_forks_requests = HashSet::new();

        while !exit.load(Ordering::Relaxed) {
            let mut set_root_elapsed;
            let mut dump_slots_elapsed;
            let mut get_votes_elapsed;
            let mut add_votes_elapsed;

            let root_bank = repair_info.bank_forks.read().unwrap().root_bank();
            let repair_protocol = serve_repair::get_repair_protocol(root_bank.cluster_type());
            let repairs = {
                let new_root = root_bank.slot();

                // Purge outdated slots from the weighting heuristic
                set_root_elapsed = Measure::start("set_root_elapsed");
                repair_weight.set_root(new_root);
                set_root_elapsed.stop();

                // Remove dumped slots from the weighting heuristic
                dump_slots_elapsed = Measure::start("dump_slots_elapsed");
                dumped_slots_receiver
                    .try_iter()
                    .for_each(|slot_hash_keys_to_dump| {
                        // Currently we don't use the correct_hash in repair. Since this dumped
                        // slot is DuplicateConfirmed, we have a >= 52% chance on receiving the
                        // correct version.
                        for (slot, _correct_hash) in slot_hash_keys_to_dump {
                            // `slot` is dumped in blockstore wanting to be repaired, we orphan it along with
                            // descendants while copying the weighting heuristic so that it can be
                            // repaired with correct ancestor information
                            //
                            // We still check to see if this slot is too old, as bank forks root
                            // might have been updated in between the send and our receive. If that
                            // is the case we can safely ignore this dump request as the slot in
                            // question would have already been purged in `repair_weight.set_root`
                            // and there is no chance of it being part of the rooted path.
                            if slot >= repair_weight.root() {
                                let dumped_slots = repair_weight.split_off(slot);
                                // Remove from outstanding ancestor hashes requests. Also clean any
                                // requests that might have been since fixed
                                popular_pruned_forks_requests.retain(|slot| {
                                    !dumped_slots.contains(slot) && repair_weight.is_pruned(*slot)
                                });
                            }
                        }
                    });
                dump_slots_elapsed.stop();

                // Add new votes to the weighting heuristic
                get_votes_elapsed = Measure::start("get_votes_elapsed");
                let mut slot_to_vote_pubkeys: HashMap<Slot, Vec<Pubkey>> = HashMap::new();
                verified_vote_receiver
                    .try_iter()
                    .for_each(|(vote_pubkey, vote_slots)| {
                        for slot in vote_slots {
                            slot_to_vote_pubkeys
                                .entry(slot)
                                .or_default()
                                .push(vote_pubkey);
                        }
                    });
                get_votes_elapsed.stop();

                add_votes_elapsed = Measure::start("add_votes");
                repair_weight.add_votes(
                    blockstore,
                    slot_to_vote_pubkeys.into_iter(),
                    root_bank.epoch_stakes_map(),
                    root_bank.epoch_schedule(),
                );
                add_votes_elapsed.stop();

                let repairs = repair_weight.get_best_weighted_repairs(
                    blockstore,
                    root_bank.epoch_stakes_map(),
                    root_bank.epoch_schedule(),
                    MAX_ORPHANS,
                    MAX_REPAIR_LENGTH,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut repair_timing,
                    &mut best_repairs_stats,
                );

                let mut popular_pruned_forks = repair_weight.get_popular_pruned_forks(
                    root_bank.epoch_stakes_map(),
                    root_bank.epoch_schedule(),
                );
                // Check if we've already sent a request along this pruned fork
                popular_pruned_forks.retain(|slot| {
                    if popular_pruned_forks_requests
                        .iter()
                        .any(|prev_req_slot| repair_weight.same_tree(*slot, *prev_req_slot))
                    {
                        false
                    } else {
                        popular_pruned_forks_requests.insert(*slot);
                        true
                    }
                });
                if !popular_pruned_forks.is_empty() {
                    warn!(
                        "Notifying repair of popular pruned forks {:?}",
                        popular_pruned_forks
                    );
                    popular_pruned_forks_sender
                        .send(popular_pruned_forks)
                        .unwrap_or_else(|err| error!("failed to send popular pruned forks {err}"));
                }

                repairs
            };

            let identity_keypair: &Keypair = &repair_info.cluster_info.keypair().clone();

            let mut build_repairs_batch_elapsed = Measure::start("build_repairs_batch_elapsed");
            let batch: Vec<(Vec<u8>, SocketAddr)> = {
                let mut outstanding_requests = outstanding_requests.write().unwrap();
                repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        let (to, req) = serve_repair
                            .repair_request(
                                &repair_info.cluster_slots,
                                repair_request,
                                &mut peers_cache,
                                &mut repair_stats,
                                &repair_info.repair_validators,
                                &mut outstanding_requests,
                                identity_keypair,
                                quic_endpoint_sender,
                                quic_endpoint_response_sender,
                                repair_protocol,
                            )
                            .ok()??;
                        Some((req, to))
                    })
                    .collect()
            };
            build_repairs_batch_elapsed.stop();

            let mut batch_send_repairs_elapsed = Measure::start("batch_send_repairs_elapsed");
            if !batch.is_empty() {
                if let Err(SendPktsError::IoError(err, num_failed)) =
                    batch_send(repair_socket, &batch)
                {
                    error!(
                        "{} batch_send failed to send {}/{} packets first error {:?}",
                        id,
                        num_failed,
                        batch.len(),
                        err
                    );
                }
            }
            batch_send_repairs_elapsed.stop();

            repair_timing.update(
                set_root_elapsed.as_us(),
                dump_slots_elapsed.as_us(),
                get_votes_elapsed.as_us(),
                add_votes_elapsed.as_us(),
                build_repairs_batch_elapsed.as_us(),
                batch_send_repairs_elapsed.as_us(),
            );

            if last_stats.elapsed().as_secs() > 2 {
                let repair_total = repair_stats.shred.count
                    + repair_stats.highest_shred.count
                    + repair_stats.orphan.count;
                let slot_to_count: Vec<_> = repair_stats
                    .shred
                    .slot_pubkeys
                    .iter()
                    .chain(repair_stats.highest_shred.slot_pubkeys.iter())
                    .chain(repair_stats.orphan.slot_pubkeys.iter())
                    .map(|(slot, slot_repairs)| {
                        (slot, slot_repairs.pubkey_repairs.values().sum::<u64>())
                    })
                    .collect();
                info!("repair_stats: {:?}", slot_to_count);
                if repair_total > 0 {
                    let nonzero_num = |x| if x == 0 { None } else { Some(x) };
                    datapoint_info!(
                        "repair_service-my_requests",
                        ("repair-total", repair_total, i64),
                        ("shred-count", repair_stats.shred.count, i64),
                        ("highest-shred-count", repair_stats.highest_shred.count, i64),
                        ("orphan-count", repair_stats.orphan.count, i64),
                        ("shred-slot-max", nonzero_num(repair_stats.shred.max), Option<i64>),
                        ("shred-slot-min", nonzero_num(repair_stats.shred.min), Option<i64>),
                        ("repair-highest-slot", repair_stats.highest_shred.max, i64), // deprecated
                        ("highest-shred-slot-max", nonzero_num(repair_stats.highest_shred.max), Option<i64>),
                        ("highest-shred-slot-min", nonzero_num(repair_stats.highest_shred.min), Option<i64>),
                        ("repair-orphan", repair_stats.orphan.max, i64), // deprecated
                        ("orphan-slot-max", nonzero_num(repair_stats.orphan.max), Option<i64>),
                        ("orphan-slot-min", nonzero_num(repair_stats.orphan.min), Option<i64>),
                    );
                }
                datapoint_info!(
                    "repair_service-repair_timing",
                    ("set-root-elapsed", repair_timing.set_root_elapsed, i64),
                    ("dump-slots-elapsed", repair_timing.dump_slots_elapsed, i64),
                    ("get-votes-elapsed", repair_timing.get_votes_elapsed, i64),
                    ("add-votes-elapsed", repair_timing.add_votes_elapsed, i64),
                    (
                        "get-best-orphans-elapsed",
                        repair_timing.get_best_orphans_elapsed,
                        i64
                    ),
                    (
                        "get-best-shreds-elapsed",
                        repair_timing.get_best_shreds_elapsed,
                        i64
                    ),
                    (
                        "get-unknown-last-index-elapsed",
                        repair_timing.get_unknown_last_index_elapsed,
                        i64
                    ),
                    (
                        "get-closest-completion-elapsed",
                        repair_timing.get_closest_completion_elapsed,
                        i64
                    ),
                    (
                        "send-repairs-elapsed",
                        repair_timing.send_repairs_elapsed,
                        i64
                    ),
                    (
                        "build-repairs-batch-elapsed",
                        repair_timing.build_repairs_batch_elapsed,
                        i64
                    ),
                    (
                        "batch-send-repairs-elapsed",
                        repair_timing.batch_send_repairs_elapsed,
                        i64
                    ),
                );
                datapoint_info!(
                    "serve_repair-best-repairs",
                    ("call-count", best_repairs_stats.call_count, i64),
                    ("orphan-slots", best_repairs_stats.num_orphan_slots, i64),
                    ("orphan-repairs", best_repairs_stats.num_orphan_repairs, i64),
                    (
                        "best-shreds-slots",
                        best_repairs_stats.num_best_shreds_slots,
                        i64
                    ),
                    (
                        "best-shreds-repairs",
                        best_repairs_stats.num_best_shreds_repairs,
                        i64
                    ),
                    (
                        "unknown-last-index-slots",
                        best_repairs_stats.num_unknown_last_index_slots,
                        i64
                    ),
                    (
                        "unknown-last-index-repairs",
                        best_repairs_stats.num_unknown_last_index_repairs,
                        i64
                    ),
                    (
                        "closest-completion-slots",
                        best_repairs_stats.num_closest_completion_slots,
                        i64
                    ),
                    (
                        "closest-completion-slots-path",
                        best_repairs_stats.num_closest_completion_slots_path,
                        i64
                    ),
                    (
                        "closest-completion-repairs",
                        best_repairs_stats.num_closest_completion_repairs,
                        i64
                    ),
                    ("repair-trees", best_repairs_stats.num_repair_trees, i64),
                );
                repair_stats = RepairStats::default();
                repair_timing = RepairTiming::default();
                best_repairs_stats = BestRepairsStats::default();
                last_stats = Instant::now();
            }
            sleep(Duration::from_millis(REPAIR_MS));
        }
    }

    /// If this slot is missing shreds generate repairs
    pub fn generate_repairs_for_slot(
        blockstore: &Blockstore,
        slot: Slot,
        slot_meta: &SlotMeta,
        max_repairs: usize,
    ) -> Vec<ShredRepairType> {
        if max_repairs == 0 || slot_meta.is_full() {
            vec![]
        } else if slot_meta.consumed == slot_meta.received {
            // check delay time of last shred
            if let Some(reference_tick) = slot_meta
                .received
                .checked_sub(1)
                .and_then(|index| blockstore.get_data_shred(slot, index).ok()?)
                .and_then(|shred| shred::layout::get_reference_tick(&shred).ok())
                .map(u64::from)
            {
                // System time is not monotonic
                let ticks_since_first_insert = DEFAULT_TICKS_PER_SECOND
                    * timestamp().saturating_sub(slot_meta.first_shred_timestamp)
                    / 1_000;
                if ticks_since_first_insert
                    < reference_tick.saturating_add(DEFER_REPAIR_THRESHOLD_TICKS)
                {
                    return vec![];
                }
            }
            vec![ShredRepairType::HighestShred(slot, slot_meta.received)]
        } else {
            blockstore
                .find_missing_data_indexes(
                    slot,
                    slot_meta.first_shred_timestamp,
                    DEFER_REPAIR_THRESHOLD_TICKS,
                    slot_meta.consumed,
                    slot_meta.received,
                    max_repairs,
                )
                .into_iter()
                .map(|i| ShredRepairType::Shred(slot, i))
                .collect()
        }
    }

    /// Repairs any fork starting at the input slot (uses blockstore for fork info)
    pub fn generate_repairs_for_fork(
        blockstore: &Blockstore,
        repairs: &mut Vec<ShredRepairType>,
        max_repairs: usize,
        slot: Slot,
    ) {
        let mut pending_slots = vec![slot];
        while repairs.len() < max_repairs && !pending_slots.is_empty() {
            let slot = pending_slots.pop().unwrap();
            if let Some(slot_meta) = blockstore.meta(slot).unwrap() {
                let new_repairs = Self::generate_repairs_for_slot(
                    blockstore,
                    slot,
                    &slot_meta,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
                let next_slots = slot_meta.next_slots;
                pending_slots.extend(next_slots);
            } else {
                break;
            }
        }
    }

    /// Generate repairs for all slots `x` in the repair_range.start <= x <= repair_range.end
    #[cfg(test)]
    pub fn generate_repairs_in_range(
        blockstore: &Blockstore,
        max_repairs: usize,
        repair_range: &RepairSlotRange,
    ) -> Vec<ShredRepairType> {
        // Slot height and shred indexes for shreds we want to repair
        let mut repairs: Vec<ShredRepairType> = vec![];
        for slot in repair_range.start..=repair_range.end {
            if repairs.len() >= max_repairs {
                break;
            }

            let meta = blockstore
                .meta(slot)
                .expect("Unable to lookup slot meta")
                .unwrap_or(SlotMeta {
                    slot,
                    ..SlotMeta::default()
                });

            let new_repairs = Self::generate_repairs_for_slot(
                blockstore,
                slot,
                &meta,
                max_repairs - repairs.len(),
            );
            repairs.extend(new_repairs);
        }

        repairs
    }

    #[cfg(test)]
    fn generate_duplicate_repairs_for_slot(
        blockstore: &Blockstore,
        slot: Slot,
    ) -> Option<Vec<ShredRepairType>> {
        if let Some(slot_meta) = blockstore.meta(slot).unwrap() {
            if slot_meta.is_full() {
                // If the slot is full, no further need to repair this slot
                None
            } else {
                Some(Self::generate_repairs_for_slot(
                    blockstore,
                    slot,
                    &slot_meta,
                    MAX_REPAIR_PER_DUPLICATE,
                ))
            }
        } else {
            error!("Slot meta for duplicate slot does not exist, cannot generate repairs");
            // Filter out this slot from the set of duplicates to be repaired as
            // the SlotMeta has to exist for duplicates to be generated
            None
        }
    }

    #[cfg(test)]
    fn generate_and_send_duplicate_repairs(
        duplicate_slot_repair_statuses: &mut HashMap<Slot, DuplicateSlotRepairStatus>,
        cluster_slots: &ClusterSlots,
        blockstore: &Blockstore,
        serve_repair: &ServeRepair,
        repair_stats: &mut RepairStats,
        repair_socket: &UdpSocket,
        repair_validators: &Option<HashSet<Pubkey>>,
        outstanding_requests: &RwLock<OutstandingShredRepairs>,
        identity_keypair: &Keypair,
    ) {
        duplicate_slot_repair_statuses.retain(|slot, status| {
            Self::update_duplicate_slot_repair_addr(
                *slot,
                status,
                cluster_slots,
                serve_repair,
                repair_validators,
            );
            if let Some((repair_pubkey, repair_addr)) = status.repair_pubkey_and_addr {
                let repairs = Self::generate_duplicate_repairs_for_slot(blockstore, *slot);

                if let Some(repairs) = repairs {
                    let mut outstanding_requests = outstanding_requests.write().unwrap();
                    for repair_type in repairs {
                        let nonce = outstanding_requests.add_request(repair_type, timestamp());

                        match serve_repair.map_repair_request(
                            &repair_type,
                            &repair_pubkey,
                            repair_stats,
                            nonce,
                            identity_keypair,
                        ) {
                            Ok(req) => {
                                if let Err(e) = repair_socket.send_to(&req, repair_addr) {
                                    info!(
                                        "repair req send_to {} ({}) error {:?}",
                                        repair_pubkey, repair_addr, e
                                    );
                                }
                            }
                            Err(e) => info!("map_repair_request err={e}"),
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                true
            }
        })
    }

    #[cfg(test)]
    fn update_duplicate_slot_repair_addr(
        slot: Slot,
        status: &mut DuplicateSlotRepairStatus,
        cluster_slots: &ClusterSlots,
        serve_repair: &ServeRepair,
        repair_validators: &Option<HashSet<Pubkey>>,
    ) {
        let now = timestamp();
        if status.repair_pubkey_and_addr.is_none()
            || now.saturating_sub(status.start_ts) >= MAX_DUPLICATE_WAIT_MS as u64
        {
            let repair_pubkey_and_addr = serve_repair.repair_request_duplicate_compute_best_peer(
                slot,
                cluster_slots,
                repair_validators,
            );
            status.repair_pubkey_and_addr = repair_pubkey_and_addr.ok();
            status.start_ts = timestamp();
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    fn initiate_repair_for_duplicate_slot(
        slot: Slot,
        duplicate_slot_repair_statuses: &mut HashMap<Slot, DuplicateSlotRepairStatus>,
        cluster_slots: &ClusterSlots,
        serve_repair: &ServeRepair,
        repair_validators: &Option<HashSet<Pubkey>>,
    ) {
        // If we're already in the middle of repairing this, ignore the signal.
        if duplicate_slot_repair_statuses.contains_key(&slot) {
            return;
        }
        // Mark this slot as special repair, try to download from single
        // validator to avoid corruption
        let repair_pubkey_and_addr = serve_repair
            .repair_request_duplicate_compute_best_peer(slot, cluster_slots, repair_validators)
            .ok();
        let new_duplicate_slot_repair_status = DuplicateSlotRepairStatus {
            correct_ancestor_to_repair: (slot, Hash::default()),
            repair_pubkey_and_addr,
            start_ts: timestamp(),
        };
        duplicate_slot_repair_statuses.insert(slot, new_duplicate_slot_repair_status);
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_repair.join()?;
        self.ancestor_hashes_service.join()
    }
}

#[cfg(test)]
pub(crate) fn sleep_shred_deferment_period() {
    // sleep to bypass shred deferment window
    sleep(Duration::from_millis(
        DEFAULT_MS_PER_SLOT + DEFER_REPAIR_THRESHOLD.as_millis() as u64,
    ));
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_gossip::{cluster_info::Node, contact_info::ContactInfo},
        solana_ledger::{
            blockstore::{
                make_chaining_slot_entries, make_many_slot_entries, make_slot_entries, Blockstore,
            },
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path,
            shred::max_ticks_per_n_shreds,
        },
        solana_runtime::bank::Bank,
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::collections::HashSet,
    };

    fn new_test_cluster_info() -> ClusterInfo {
        let keypair = Arc::new(Keypair::new());
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), timestamp());
        ClusterInfo::new(contact_info, keypair, SocketAddrSpace::Unspecified)
    }

    #[test]
    pub fn test_repair_orphan() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            // Create some orphan slots
            let (mut shreds, _) = make_slot_entries(1, 0, 1, /*merkle_variant:*/ true);
            let (shreds2, _) = make_slot_entries(5, 2, 1, /*merkle_variant:*/ true);
            shreds.extend(shreds2);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let mut repair_weight = RepairWeight::new(0);
            assert_eq!(
                repair_weight.get_best_weighted_repairs(
                    &blockstore,
                    &HashMap::new(),
                    &EpochSchedule::default(),
                    MAX_ORPHANS,
                    MAX_REPAIR_LENGTH,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut RepairTiming::default(),
                    &mut BestRepairsStats::default(),
                ),
                vec![
                    ShredRepairType::Orphan(2),
                    ShredRepairType::HighestShred(0, 0)
                ]
            );
        }

        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_empty_slot() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let (shreds, _) = make_slot_entries(2, 0, 1, /*merkle_variant:*/ true);

            // Write this shred to slot 2, should chain to slot 0, which we haven't received
            // any shreds for
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let mut repair_weight = RepairWeight::new(0);

            // Check that repair tries to patch the empty slot
            assert_eq!(
                repair_weight.get_best_weighted_repairs(
                    &blockstore,
                    &HashMap::new(),
                    &EpochSchedule::default(),
                    MAX_ORPHANS,
                    MAX_REPAIR_LENGTH,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut RepairTiming::default(),
                    &mut BestRepairsStats::default(),
                ),
                vec![ShredRepairType::HighestShred(0, 0)]
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_repairs() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let nth = 3;
            let num_slots = 2;

            // Create some shreds
            let (mut shreds, _) = make_many_slot_entries(0, num_slots, 150);
            let num_shreds = shreds.len() as u64;
            let num_shreds_per_slot = num_shreds / num_slots;

            // write every nth shred
            let mut shreds_to_write = vec![];
            let mut missing_indexes_per_slot = vec![];
            for i in (0..num_shreds).rev() {
                let index = i % num_shreds_per_slot;
                // get_best_repair_shreds only returns missing shreds in
                // between shreds received; So this should either insert the
                // last shred in each slot, or exclude missing shreds after the
                // last inserted shred from expected repairs.
                if index % nth == 0 || index + 1 == num_shreds_per_slot {
                    shreds_to_write.insert(0, shreds.remove(i as usize));
                } else if i < num_shreds_per_slot {
                    missing_indexes_per_slot.insert(0, index);
                }
            }
            blockstore
                .insert_shreds(shreds_to_write, None, false)
                .unwrap();
            let expected: Vec<ShredRepairType> = (0..num_slots)
                .flat_map(|slot| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |shred_index| ShredRepairType::Shred(slot, *shred_index))
                })
                .collect();

            let mut repair_weight = RepairWeight::new(0);
            sleep_shred_deferment_period();
            assert_eq!(
                repair_weight.get_best_weighted_repairs(
                    &blockstore,
                    &HashMap::new(),
                    &EpochSchedule::default(),
                    MAX_ORPHANS,
                    MAX_REPAIR_LENGTH,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut RepairTiming::default(),
                    &mut BestRepairsStats::default(),
                ),
                expected
            );

            assert_eq!(
                repair_weight.get_best_weighted_repairs(
                    &blockstore,
                    &HashMap::new(),
                    &EpochSchedule::default(),
                    MAX_ORPHANS,
                    expected.len() - 2,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut RepairTiming::default(),
                    &mut BestRepairsStats::default(),
                )[..],
                expected[0..expected.len() - 2]
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_highest_repair() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let num_entries_per_slot = 100;

            // Create some shreds
            let (mut shreds, _) = make_slot_entries(
                0, // slot
                0, // parent_slot
                num_entries_per_slot as u64,
                true, // merkle_variant
            );
            let num_shreds_per_slot = shreds.len() as u64;

            // Remove last shred (which is also last in slot) so that slot is not complete
            shreds.pop();

            blockstore.insert_shreds(shreds, None, false).unwrap();

            // We didn't get the last shred for this slot, so ask for the highest shred for that slot
            let expected: Vec<ShredRepairType> =
                vec![ShredRepairType::HighestShred(0, num_shreds_per_slot - 1)];

            sleep_shred_deferment_period();
            let mut repair_weight = RepairWeight::new(0);
            assert_eq!(
                repair_weight.get_best_weighted_repairs(
                    &blockstore,
                    &HashMap::new(),
                    &EpochSchedule::default(),
                    MAX_ORPHANS,
                    MAX_REPAIR_LENGTH,
                    MAX_UNKNOWN_LAST_INDEX_REPAIRS,
                    MAX_CLOSEST_COMPLETION_REPAIRS,
                    &mut RepairTiming::default(),
                    &mut BestRepairsStats::default(),
                ),
                expected
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_range() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let slots: Vec<u64> = vec![1, 3, 5, 7, 8];
            let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;

            let shreds = make_chaining_slot_entries(&slots, num_entries_per_slot);
            for (mut slot_shreds, _) in shreds.into_iter() {
                slot_shreds.remove(0);
                blockstore.insert_shreds(slot_shreds, None, false).unwrap();
            }

            // Iterate through all possible combinations of start..end (inclusive on both
            // sides of the range)
            for start in 0..slots.len() {
                for end in start..slots.len() {
                    let repair_slot_range = RepairSlotRange {
                        start: slots[start],
                        end: slots[end],
                    };
                    let expected: Vec<ShredRepairType> = (repair_slot_range.start
                        ..=repair_slot_range.end)
                        .map(|slot_index| {
                            if slots.contains(&slot_index) {
                                ShredRepairType::Shred(slot_index, 0)
                            } else {
                                ShredRepairType::HighestShred(slot_index, 0)
                            }
                        })
                        .collect();

                    sleep_shred_deferment_period();
                    assert_eq!(
                        RepairService::generate_repairs_in_range(
                            &blockstore,
                            std::usize::MAX,
                            &repair_slot_range,
                        ),
                        expected
                    );
                }
            }
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_range_highest() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();

            let num_entries_per_slot = 10;

            let num_slots = 1;
            let start = 5;

            // Create some shreds in slots 0..num_slots
            for i in start..start + num_slots {
                let parent = if i > 0 { i - 1 } else { 0 };
                let (shreds, _) = make_slot_entries(
                    i, // slot
                    parent,
                    num_entries_per_slot as u64,
                    true, // merkle_variant
                );

                blockstore.insert_shreds(shreds, None, false).unwrap();
            }

            let end = 4;
            let expected: Vec<ShredRepairType> = vec![
                ShredRepairType::HighestShred(end - 2, 0),
                ShredRepairType::HighestShred(end - 1, 0),
                ShredRepairType::HighestShred(end, 0),
            ];

            let repair_slot_range = RepairSlotRange { start: 2, end };

            assert_eq!(
                RepairService::generate_repairs_in_range(
                    &blockstore,
                    std::usize::MAX,
                    &repair_slot_range,
                ),
                expected
            );
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_duplicate_repairs_for_slot() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let dead_slot = 9;

        // SlotMeta doesn't exist, should make no repairs
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_none()
        );

        // Insert some shreds to create a SlotMeta, should make repairs
        let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;
        let (mut shreds, _) = make_slot_entries(
            dead_slot,     // slot
            dead_slot - 1, // parent_slot
            num_entries_per_slot,
            true, // merkle_variant
        );
        blockstore
            .insert_shreds(shreds[..shreds.len() - 1].to_vec(), None, false)
            .unwrap();
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_some()
        );

        // SlotMeta is full, should make no repairs
        blockstore
            .insert_shreds(vec![shreds.pop().unwrap()], None, false)
            .unwrap();
        assert!(
            RepairService::generate_duplicate_repairs_for_slot(&blockstore, dead_slot,).is_none()
        );
    }

    #[test]
    pub fn test_generate_and_send_duplicate_repairs() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let cluster_slots = ClusterSlots::default();
        let cluster_info = Arc::new(new_test_cluster_info());
        let identity_keypair = cluster_info.keypair().clone();
        let serve_repair = ServeRepair::new(
            cluster_info,
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );
        let mut duplicate_slot_repair_statuses = HashMap::new();
        let dead_slot = 9;
        let receive_socket = &UdpSocket::bind("0.0.0.0:0").unwrap();
        let duplicate_status = DuplicateSlotRepairStatus {
            correct_ancestor_to_repair: (dead_slot, Hash::default()),
            start_ts: std::u64::MAX,
            repair_pubkey_and_addr: None,
        };

        // Insert some shreds to create a SlotMeta,
        let num_entries_per_slot = max_ticks_per_n_shreds(1, None) + 1;
        let (mut shreds, _) = make_slot_entries(
            dead_slot,
            dead_slot - 1,
            num_entries_per_slot,
            true, // merkle_variant
        );
        blockstore
            .insert_shreds(shreds[..shreds.len() - 1].to_vec(), None, false)
            .unwrap();

        duplicate_slot_repair_statuses.insert(dead_slot, duplicate_status);

        // There is no repair_addr, so should not get filtered because the timeout
        // `std::u64::MAX` has not expired
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
            &None,
            &RwLock::new(OutstandingRequests::default()),
            &identity_keypair,
        );
        assert!(duplicate_slot_repair_statuses
            .get(&dead_slot)
            .unwrap()
            .repair_pubkey_and_addr
            .is_none());
        assert!(duplicate_slot_repair_statuses.get(&dead_slot).is_some());

        // Give the slot a repair address
        duplicate_slot_repair_statuses
            .get_mut(&dead_slot)
            .unwrap()
            .repair_pubkey_and_addr =
            Some((Pubkey::default(), receive_socket.local_addr().unwrap()));

        // Slot is not yet full, should not get filtered from `duplicate_slot_repair_statuses`
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
            &None,
            &RwLock::new(OutstandingRequests::default()),
            &identity_keypair,
        );
        assert_eq!(duplicate_slot_repair_statuses.len(), 1);
        assert!(duplicate_slot_repair_statuses.get(&dead_slot).is_some());

        // Insert rest of shreds. Slot is full, should get filtered from
        // `duplicate_slot_repair_statuses`
        blockstore
            .insert_shreds(vec![shreds.pop().unwrap()], None, false)
            .unwrap();
        RepairService::generate_and_send_duplicate_repairs(
            &mut duplicate_slot_repair_statuses,
            &cluster_slots,
            &blockstore,
            &serve_repair,
            &mut RepairStats::default(),
            &UdpSocket::bind("0.0.0.0:0").unwrap(),
            &None,
            &RwLock::new(OutstandingRequests::default()),
            &identity_keypair,
        );
        assert!(duplicate_slot_repair_statuses.is_empty());
    }

    #[test]
    pub fn test_update_duplicate_slot_repair_addr() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let dummy_addr = Some((
            Pubkey::default(),
            UdpSocket::bind("0.0.0.0:0").unwrap().local_addr().unwrap(),
        ));
        let cluster_info = Arc::new(new_test_cluster_info());
        let serve_repair = ServeRepair::new(
            cluster_info.clone(),
            bank_forks,
            Arc::new(RwLock::new(HashSet::default())),
        );
        let valid_repair_peer = Node::new_localhost().info;

        // Signal that this peer has confirmed the dead slot, and is thus
        // a valid target for repair
        let dead_slot = 9;
        let cluster_slots = ClusterSlots::default();
        cluster_slots.insert_node_id(dead_slot, *valid_repair_peer.pubkey());
        cluster_info.insert_info(valid_repair_peer);

        // Not enough time has passed, should not update the
        // address
        let mut duplicate_status = DuplicateSlotRepairStatus {
            correct_ancestor_to_repair: (dead_slot, Hash::default()),
            start_ts: std::u64::MAX,
            repair_pubkey_and_addr: dummy_addr,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
            &None,
        );
        assert_eq!(duplicate_status.repair_pubkey_and_addr, dummy_addr);

        // If the repair address is None, should try to update
        let mut duplicate_status = DuplicateSlotRepairStatus {
            correct_ancestor_to_repair: (dead_slot, Hash::default()),
            start_ts: std::u64::MAX,
            repair_pubkey_and_addr: None,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
            &None,
        );
        assert!(duplicate_status.repair_pubkey_and_addr.is_some());

        // If sufficient time has passed, should try to update
        let mut duplicate_status = DuplicateSlotRepairStatus {
            correct_ancestor_to_repair: (dead_slot, Hash::default()),
            start_ts: timestamp() - MAX_DUPLICATE_WAIT_MS as u64,
            repair_pubkey_and_addr: dummy_addr,
        };
        RepairService::update_duplicate_slot_repair_addr(
            dead_slot,
            &mut duplicate_status,
            &cluster_slots,
            &serve_repair,
            &None,
        );
        assert_ne!(duplicate_status.repair_pubkey_and_addr, dummy_addr);
    }
}
