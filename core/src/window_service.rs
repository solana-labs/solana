//! `window_service` handles the data plane incoming shreds, storing them in
//!   blockstore and retransmitting where required
//!
use {
    crate::{
        ancestor_hashes_service::AncestorHashesReplayUpdateReceiver,
        cluster_info_vote_listener::VerifiedVoteReceiver,
        completed_data_sets_service::CompletedDataSetsSender,
        repair_response,
        repair_service::{DumpedSlotsReceiver, OutstandingShredRepairs, RepairInfo, RepairService},
        result::{Error, Result},
    },
    crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender},
    rayon::{prelude::*, ThreadPool},
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{
        blockstore::{Blockstore, BlockstoreInsertionMetrics},
        leader_schedule_cache::LeaderScheduleCache,
        shred::{self, Nonce, ReedSolomonCache, Shred},
    },
    solana_measure::measure::Measure,
    solana_metrics::inc_new_counter_error,
    solana_perf::packet::{Packet, PacketBatch},
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::clock::Slot,
    std::{
        cmp::Reverse,
        collections::{HashMap, HashSet},
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

type ShredPayload = Vec<u8>;
type DuplicateSlotSender = Sender<Slot>;
pub(crate) type DuplicateSlotReceiver = Receiver<Slot>;

#[derive(Default)]
struct WindowServiceMetrics {
    run_insert_count: u64,
    num_packets: usize,
    num_repairs: usize,
    num_shreds_received: usize,
    handle_packets_elapsed_us: u64,
    shred_receiver_elapsed_us: u64,
    prune_shreds_elapsed_us: u64,
    num_shreds_pruned_invalid_repair: usize,
    num_errors: u64,
    num_errors_blockstore: u64,
    num_errors_cross_beam_recv_timeout: u64,
    num_errors_other: u64,
    num_errors_try_crossbeam_send: u64,
    addrs: HashMap</*source:*/ SocketAddr, /*num packets:*/ usize>,
}

impl WindowServiceMetrics {
    fn report_metrics(&self, metric_name: &'static str) {
        const MAX_NUM_ADDRS: usize = 5;
        datapoint_info!(
            metric_name,
            (
                "handle_packets_elapsed_us",
                self.handle_packets_elapsed_us,
                i64
            ),
            ("run_insert_count", self.run_insert_count as i64, i64),
            ("num_packets", self.num_packets, i64),
            ("num_repairs", self.num_repairs, i64),
            ("num_shreds_received", self.num_shreds_received, i64),
            (
                "shred_receiver_elapsed_us",
                self.shred_receiver_elapsed_us as i64,
                i64
            ),
            (
                "prune_shreds_elapsed_us",
                self.prune_shreds_elapsed_us as i64,
                i64
            ),
            (
                "num_shreds_pruned_invalid_repair",
                self.num_shreds_pruned_invalid_repair,
                i64
            ),
            ("num_errors", self.num_errors, i64),
            ("num_errors_blockstore", self.num_errors_blockstore, i64),
            ("num_errors_other", self.num_errors_other, i64),
            (
                "num_errors_try_crossbeam_send",
                self.num_errors_try_crossbeam_send,
                i64
            ),
            (
                "num_errors_cross_beam_recv_timeout",
                self.num_errors_cross_beam_recv_timeout,
                i64
            ),
        );

        let mut addrs: Vec<_> = self.addrs.iter().collect();
        let reverse_count = |(_addr, count): &_| Reverse(*count);
        if addrs.len() > MAX_NUM_ADDRS {
            addrs.select_nth_unstable_by_key(MAX_NUM_ADDRS, reverse_count);
            addrs.truncate(MAX_NUM_ADDRS);
        }
        addrs.sort_unstable_by_key(reverse_count);
        info!(
            "num addresses: {}, top packets by source: {:?}",
            self.addrs.len(),
            addrs
        );
    }

    fn record_error(&mut self, err: &Error) {
        self.num_errors += 1;
        match err {
            Error::TrySend => self.num_errors_try_crossbeam_send += 1,
            Error::RecvTimeout(_) => self.num_errors_cross_beam_recv_timeout += 1,
            Error::Blockstore(err) => {
                self.num_errors_blockstore += 1;
                error!("blockstore error: {}", err);
            }
            _ => self.num_errors_other += 1,
        }
    }
}

fn run_check_duplicate(
    cluster_info: &ClusterInfo,
    blockstore: &Blockstore,
    shred_receiver: &Receiver<Shred>,
    duplicate_slots_sender: &DuplicateSlotSender,
) -> Result<()> {
    let check_duplicate = |shred: Shred| -> Result<()> {
        let shred_slot = shred.slot();
        if !blockstore.has_duplicate_shreds_in_slot(shred_slot) {
            if let Some(existing_shred_payload) =
                blockstore.is_shred_duplicate(shred.id(), shred.payload().clone())
            {
                cluster_info.push_duplicate_shred(&shred, &existing_shred_payload)?;
                blockstore.store_duplicate_slot(
                    shred_slot,
                    existing_shred_payload,
                    shred.into_payload(),
                )?;

                duplicate_slots_sender.send(shred_slot)?;
            }
        }

        Ok(())
    };
    const RECV_TIMEOUT: Duration = Duration::from_millis(200);
    std::iter::once(shred_receiver.recv_timeout(RECV_TIMEOUT)?)
        .chain(shred_receiver.try_iter())
        .try_for_each(check_duplicate)
}

fn verify_repair(
    outstanding_requests: &mut OutstandingShredRepairs,
    shred: &Shred,
    repair_meta: &Option<RepairMeta>,
) -> bool {
    repair_meta
        .as_ref()
        .map(|repair_meta| {
            outstanding_requests
                .register_response(
                    repair_meta.nonce,
                    shred,
                    solana_sdk::timing::timestamp(),
                    |_| (),
                )
                .is_some()
        })
        .unwrap_or(true)
}

fn prune_shreds_invalid_repair(
    shreds: &mut Vec<Shred>,
    repair_infos: &mut Vec<Option<RepairMeta>>,
    outstanding_requests: &RwLock<OutstandingShredRepairs>,
) {
    assert_eq!(shreds.len(), repair_infos.len());
    let mut i = 0;
    let mut removed = HashSet::new();
    {
        let mut outstanding_requests = outstanding_requests.write().unwrap();
        shreds.retain(|shred| {
            let should_keep = (
                verify_repair(&mut outstanding_requests, shred, &repair_infos[i]),
                i += 1,
            )
                .0;
            if !should_keep {
                removed.insert(i - 1);
            }
            should_keep
        });
    }
    i = 0;
    repair_infos.retain(|_repair_info| (!removed.contains(&i), i += 1).0);
    assert_eq!(shreds.len(), repair_infos.len());
}

#[allow(clippy::too_many_arguments)]
fn run_insert<F>(
    thread_pool: &ThreadPool,
    verified_receiver: &Receiver<Vec<PacketBatch>>,
    blockstore: &Blockstore,
    leader_schedule_cache: &LeaderScheduleCache,
    handle_duplicate: F,
    metrics: &mut BlockstoreInsertionMetrics,
    ws_metrics: &mut WindowServiceMetrics,
    completed_data_sets_sender: &CompletedDataSetsSender,
    retransmit_sender: &Sender<Vec<ShredPayload>>,
    outstanding_requests: &RwLock<OutstandingShredRepairs>,
    reed_solomon_cache: &ReedSolomonCache,
) -> Result<()>
where
    F: Fn(Shred),
{
    const RECV_TIMEOUT: Duration = Duration::from_millis(200);
    let mut shred_receiver_elapsed = Measure::start("shred_receiver_elapsed");
    let mut packets = verified_receiver.recv_timeout(RECV_TIMEOUT)?;
    packets.extend(verified_receiver.try_iter().flatten());
    shred_receiver_elapsed.stop();
    ws_metrics.shred_receiver_elapsed_us += shred_receiver_elapsed.as_us();
    ws_metrics.run_insert_count += 1;
    let handle_packet = |packet: &Packet| {
        if packet.meta().discard() {
            return None;
        }
        let shred = shred::layout::get_shred(packet)?;
        let shred = Shred::new_from_serialized_shred(shred.to_vec()).ok()?;
        if packet.meta().repair() {
            let repair_info = RepairMeta {
                _from_addr: packet.meta().socket_addr(),
                // If can't parse the nonce, dump the packet.
                nonce: repair_response::nonce(packet)?,
            };
            Some((shred, Some(repair_info)))
        } else {
            Some((shred, None))
        }
    };
    let now = Instant::now();
    let (mut shreds, mut repair_infos): (Vec<_>, Vec<_>) = thread_pool.install(|| {
        packets
            .par_iter()
            .flat_map_iter(|packets| packets.iter().filter_map(handle_packet))
            .unzip()
    });
    ws_metrics.handle_packets_elapsed_us += now.elapsed().as_micros() as u64;
    ws_metrics.num_packets += packets.iter().map(PacketBatch::len).sum::<usize>();
    ws_metrics.num_repairs += repair_infos.iter().filter(|r| r.is_some()).count();
    ws_metrics.num_shreds_received += shreds.len();
    for packet in packets.iter().flat_map(PacketBatch::iter) {
        let addr = packet.meta().socket_addr();
        *ws_metrics.addrs.entry(addr).or_default() += 1;
    }

    let mut prune_shreds_elapsed = Measure::start("prune_shreds_elapsed");
    let num_shreds = shreds.len();
    prune_shreds_invalid_repair(&mut shreds, &mut repair_infos, outstanding_requests);
    ws_metrics.num_shreds_pruned_invalid_repair = num_shreds - shreds.len();
    let repairs: Vec<_> = repair_infos
        .iter()
        .map(|repair_info| repair_info.is_some())
        .collect();
    prune_shreds_elapsed.stop();
    ws_metrics.prune_shreds_elapsed_us += prune_shreds_elapsed.as_us();

    let completed_data_sets = blockstore.insert_shreds_handle_duplicate(
        shreds,
        repairs,
        Some(leader_schedule_cache),
        false, // is_trusted
        Some(retransmit_sender),
        &handle_duplicate,
        reed_solomon_cache,
        metrics,
    )?;

    completed_data_sets_sender.try_send(completed_data_sets)?;
    Ok(())
}

struct RepairMeta {
    _from_addr: SocketAddr,
    nonce: Nonce,
}

pub(crate) struct WindowService {
    t_insert: JoinHandle<()>,
    t_check_duplicate: JoinHandle<()>,
    repair_service: RepairService,
}

impl WindowService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        blockstore: Arc<Blockstore>,
        verified_receiver: Receiver<Vec<PacketBatch>>,
        retransmit_sender: Sender<Vec<ShredPayload>>,
        repair_socket: Arc<UdpSocket>,
        ancestor_hashes_socket: Arc<UdpSocket>,
        exit: Arc<AtomicBool>,
        repair_info: RepairInfo,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        verified_vote_receiver: VerifiedVoteReceiver,
        completed_data_sets_sender: CompletedDataSetsSender,
        duplicate_slots_sender: DuplicateSlotSender,
        ancestor_hashes_replay_update_receiver: AncestorHashesReplayUpdateReceiver,
        dumped_slots_receiver: DumpedSlotsReceiver,
    ) -> WindowService {
        let outstanding_requests = Arc::<RwLock<OutstandingShredRepairs>>::default();

        let cluster_info = repair_info.cluster_info.clone();

        let repair_service = RepairService::new(
            blockstore.clone(),
            exit.clone(),
            repair_socket,
            ancestor_hashes_socket,
            repair_info,
            verified_vote_receiver,
            outstanding_requests.clone(),
            ancestor_hashes_replay_update_receiver,
            dumped_slots_receiver,
        );

        let (duplicate_sender, duplicate_receiver) = unbounded();

        let t_check_duplicate = Self::start_check_duplicate_thread(
            cluster_info,
            exit.clone(),
            blockstore.clone(),
            duplicate_receiver,
            duplicate_slots_sender,
        );

        let t_insert = Self::start_window_insert_thread(
            exit,
            blockstore,
            leader_schedule_cache,
            verified_receiver,
            duplicate_sender,
            completed_data_sets_sender,
            retransmit_sender,
            outstanding_requests,
        );

        WindowService {
            t_insert,
            t_check_duplicate,
            repair_service,
        }
    }

    fn start_check_duplicate_thread(
        cluster_info: Arc<ClusterInfo>,
        exit: Arc<AtomicBool>,
        blockstore: Arc<Blockstore>,
        duplicate_receiver: Receiver<Shred>,
        duplicate_slots_sender: DuplicateSlotSender,
    ) -> JoinHandle<()> {
        let handle_error = || {
            inc_new_counter_error!("solana-check-duplicate-error", 1, 1);
        };
        Builder::new()
            .name("solWinCheckDup".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    if let Err(e) = run_check_duplicate(
                        &cluster_info,
                        &blockstore,
                        &duplicate_receiver,
                        &duplicate_slots_sender,
                    ) {
                        if Self::should_exit_on_error(e, &handle_error) {
                            break;
                        }
                    }
                }
            })
            .unwrap()
    }

    fn start_window_insert_thread(
        exit: Arc<AtomicBool>,
        blockstore: Arc<Blockstore>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        verified_receiver: Receiver<Vec<PacketBatch>>,
        check_duplicate_sender: Sender<Shred>,
        completed_data_sets_sender: CompletedDataSetsSender,
        retransmit_sender: Sender<Vec<ShredPayload>>,
        outstanding_requests: Arc<RwLock<OutstandingShredRepairs>>,
    ) -> JoinHandle<()> {
        let handle_error = || {
            inc_new_counter_error!("solana-window-insert-error", 1, 1);
        };
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(get_thread_count().min(8))
            .thread_name(|i| format!("solWinInsert{i:02}"))
            .build()
            .unwrap();
        let reed_solomon_cache = ReedSolomonCache::default();
        Builder::new()
            .name("solWinInsert".to_string())
            .spawn(move || {
                let handle_duplicate = |shred| {
                    let _ = check_duplicate_sender.send(shred);
                };
                let mut metrics = BlockstoreInsertionMetrics::default();
                let mut ws_metrics = WindowServiceMetrics::default();
                let mut last_print = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    if let Err(e) = run_insert(
                        &thread_pool,
                        &verified_receiver,
                        &blockstore,
                        &leader_schedule_cache,
                        handle_duplicate,
                        &mut metrics,
                        &mut ws_metrics,
                        &completed_data_sets_sender,
                        &retransmit_sender,
                        &outstanding_requests,
                        &reed_solomon_cache,
                    ) {
                        ws_metrics.record_error(&e);
                        if Self::should_exit_on_error(e, &handle_error) {
                            break;
                        }
                    }

                    if last_print.elapsed().as_secs() > 2 {
                        metrics.report_metrics("blockstore-insert-shreds");
                        metrics = BlockstoreInsertionMetrics::default();
                        ws_metrics.report_metrics("recv-window-insert-shreds");
                        ws_metrics = WindowServiceMetrics::default();
                        last_print = Instant::now();
                    }
                }
            })
            .unwrap()
    }

    fn should_exit_on_error<H>(e: Error, handle_error: &H) -> bool
    where
        H: Fn(),
    {
        match e {
            Error::RecvTimeout(RecvTimeoutError::Disconnected) => true,
            Error::RecvTimeout(RecvTimeoutError::Timeout) => false,
            Error::Send => true,
            _ => {
                handle_error();
                error!("thread {:?} error {:?}", thread::current().name(), e);
                false
            }
        }
    }

    pub(crate) fn join(self) -> thread::Result<()> {
        self.t_insert.join()?;
        self.t_check_duplicate.join()?;
        self.repair_service.join()
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_entry::entry::{create_ticks, Entry},
        solana_gossip::contact_info::ContactInfo,
        solana_ledger::{
            blockstore::{make_many_slot_entries, Blockstore},
            get_tmp_ledger_path,
            shred::{ProcessShredsStats, Shredder},
        },
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
    };

    fn local_entries_to_shred(
        entries: &[Entry],
        slot: Slot,
        parent: Slot,
        keypair: &Keypair,
    ) -> Vec<Shred> {
        let shredder = Shredder::new(slot, parent, 0, 0).unwrap();
        let (data_shreds, _) = shredder.entries_to_shreds(
            keypair,
            entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        data_shreds
    }

    #[test]
    fn test_process_shred() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&blockstore_path).unwrap());
        let num_entries = 10;
        let original_entries = create_ticks(num_entries, 0, Hash::default());
        let mut shreds = local_entries_to_shred(&original_entries, 0, 0, &Keypair::new());
        shreds.reverse();
        blockstore
            .insert_shreds(shreds, None, false)
            .expect("Expect successful processing of shred");

        assert_eq!(blockstore.get_slot_entries(0, 0).unwrap(), original_entries);

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_run_check_duplicate() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&blockstore_path).unwrap());
        let (sender, receiver) = unbounded();
        let (duplicate_slot_sender, duplicate_slot_receiver) = unbounded();
        let (shreds, _) = make_many_slot_entries(5, 5, 10);
        blockstore
            .insert_shreds(shreds.clone(), None, false)
            .unwrap();
        let duplicate_shred = {
            let (mut shreds, _) = make_many_slot_entries(5, 1, 10);
            shreds.swap_remove(0)
        };
        assert_eq!(duplicate_shred.slot(), shreds[0].slot());
        let duplicate_shred_slot = duplicate_shred.slot();
        sender.send(duplicate_shred).unwrap();
        assert!(!blockstore.has_duplicate_shreds_in_slot(duplicate_shred_slot));
        let keypair = Keypair::new();
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), timestamp());
        let cluster_info = ClusterInfo::new(
            contact_info,
            Arc::new(keypair),
            SocketAddrSpace::Unspecified,
        );
        run_check_duplicate(
            &cluster_info,
            &blockstore,
            &receiver,
            &duplicate_slot_sender,
        )
        .unwrap();
        assert!(blockstore.has_duplicate_shreds_in_slot(duplicate_shred_slot));
        assert_eq!(
            duplicate_slot_receiver.try_recv().unwrap(),
            duplicate_shred_slot
        );
    }

    #[test]
    fn test_prune_shreds() {
        use {
            crate::serve_repair::ShredRepairType,
            std::net::{IpAddr, Ipv4Addr},
        };
        solana_logger::setup();
        let shred = Shred::new_from_parity_shard(
            5,   // slot
            5,   // index
            &[], // parity_shard
            5,   // fec_set_index
            6,   // num_data_shreds
            6,   // num_coding_shreds
            4,   // position
            0,   // version
        );
        let mut shreds = vec![shred.clone(), shred.clone(), shred];
        let _from_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let repair_meta = RepairMeta {
            _from_addr,
            nonce: 0,
        };
        let outstanding_requests = Arc::new(RwLock::new(OutstandingShredRepairs::default()));
        let repair_type = ShredRepairType::Orphan(9);
        let nonce = outstanding_requests
            .write()
            .unwrap()
            .add_request(repair_type, timestamp());
        let repair_meta1 = RepairMeta { _from_addr, nonce };
        let mut repair_infos = vec![None, Some(repair_meta), Some(repair_meta1)];
        prune_shreds_invalid_repair(&mut shreds, &mut repair_infos, &outstanding_requests);
        assert_eq!(repair_infos.len(), 2);
        assert!(repair_infos[0].is_none());
        assert_eq!(repair_infos[1].as_ref().unwrap().nonce, nonce);
    }
}
