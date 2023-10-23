//! A stage to broadcast data from a leader node to validators
#![allow(clippy::rc_buffer)]
use {
    self::{
        broadcast_duplicates_run::{BroadcastDuplicatesConfig, BroadcastDuplicatesRun},
        broadcast_fake_shreds_run::BroadcastFakeShredsRun,
        broadcast_metrics::*,
        fail_entry_verification_broadcast_run::FailEntryVerificationBroadcastRun,
        standard_broadcast_run::StandardBroadcastRun,
    },
    crate::cluster_nodes::{self, ClusterNodes, ClusterNodesCache},
    bytes::Bytes,
    crossbeam_channel::{unbounded, Receiver, RecvError, RecvTimeoutError, Sender},
    itertools::{Either, Itertools},
    solana_gossip::{
        cluster_info::{ClusterInfo, ClusterInfoError},
        contact_info::Protocol,
    },
    solana_ledger::{blockstore::Blockstore, shred::Shred},
    solana_measure::measure::Measure,
    solana_metrics::{inc_new_counter_error, inc_new_counter_info},
    solana_poh::poh_recorder::WorkingBankEntry,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{
        clock::Slot,
        pubkey::Pubkey,
        signature::Keypair,
        timing::{timestamp, AtomicInterval},
    },
    solana_streamer::{
        sendmmsg::{batch_send, SendPktsError},
        socket::SocketAddrSpace,
    },
    std::{
        collections::{HashMap, HashSet},
        iter::repeat_with,
        net::{SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    thiserror::Error,
    tokio::sync::mpsc::Sender as AsyncSender,
};

pub mod broadcast_duplicates_run;
mod broadcast_fake_shreds_run;
pub mod broadcast_metrics;
pub(crate) mod broadcast_utils;
mod fail_entry_verification_broadcast_run;
mod standard_broadcast_run;

const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;
const CLUSTER_NODES_CACHE_TTL: Duration = Duration::from_secs(5);

pub(crate) const NUM_INSERT_THREADS: usize = 2;
pub(crate) type RecordReceiver = Receiver<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>;
pub(crate) type TransmitReceiver = Receiver<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Blockstore(#[from] solana_ledger::blockstore::BlockstoreError),
    #[error(transparent)]
    ClusterInfo(#[from] solana_gossip::cluster_info::ClusterInfoError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Recv(#[from] crossbeam_channel::RecvError),
    #[error(transparent)]
    RecvTimeout(#[from] crossbeam_channel::RecvTimeoutError),
    #[error("Send")]
    Send,
    #[error(transparent)]
    Serialize(#[from] std::boxed::Box<bincode::ErrorKind>),
    #[error(transparent)]
    TransportError(#[from] solana_sdk::transport::TransportError),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    ChannelDisconnected,
}

#[derive(Clone, Debug)]
pub enum BroadcastStageType {
    Standard,
    FailEntryVerification,
    BroadcastFakeShreds,
    BroadcastDuplicates(BroadcastDuplicatesConfig),
}

impl BroadcastStageType {
    #[allow(clippy::too_many_arguments)]
    pub fn new_broadcast_stage(
        &self,
        sock: Vec<UdpSocket>,
        cluster_info: Arc<ClusterInfo>,
        receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: Receiver<Slot>,
        exit_sender: Arc<AtomicBool>,
        blockstore: Arc<Blockstore>,
        bank_forks: Arc<RwLock<BankForks>>,
        shred_version: u16,
        quic_endpoint_sender: AsyncSender<(SocketAddr, Bytes)>,
    ) -> BroadcastStage {
        match self {
            BroadcastStageType::Standard => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                bank_forks,
                quic_endpoint_sender,
                StandardBroadcastRun::new(shred_version),
            ),

            BroadcastStageType::FailEntryVerification => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                bank_forks,
                quic_endpoint_sender,
                FailEntryVerificationBroadcastRun::new(shred_version),
            ),

            BroadcastStageType::BroadcastFakeShreds => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                bank_forks,
                quic_endpoint_sender,
                BroadcastFakeShredsRun::new(0, shred_version),
            ),

            BroadcastStageType::BroadcastDuplicates(config) => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                bank_forks,
                quic_endpoint_sender,
                BroadcastDuplicatesRun::new(shred_version, config.clone()),
            ),
        }
    }
}

trait BroadcastRun {
    fn run(
        &mut self,
        keypair: &Keypair,
        blockstore: &Blockstore,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()>;
    fn transmit(
        &mut self,
        receiver: &TransmitReceiver,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
        bank_forks: &RwLock<BankForks>,
        quic_endpoint_sender: &AsyncSender<(SocketAddr, Bytes)>,
    ) -> Result<()>;
    fn record(&mut self, receiver: &RecordReceiver, blockstore: &Blockstore) -> Result<()>;
}

// Implement a destructor for the BroadcastStage thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct BroadcastStage {
    thread_hdls: Vec<JoinHandle<BroadcastStageReturnType>>,
}

impl BroadcastStage {
    #[allow(clippy::too_many_arguments)]
    fn run(
        cluster_info: Arc<ClusterInfo>,
        blockstore: &Blockstore,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        mut broadcast_stage_run: impl BroadcastRun,
    ) -> BroadcastStageReturnType {
        loop {
            let res = broadcast_stage_run.run(
                &cluster_info.keypair(),
                blockstore,
                receiver,
                socket_sender,
                blockstore_sender,
            );
            let res = Self::handle_error(res, "run");
            if let Some(res) = res {
                return res;
            }
        }
    }
    fn handle_error(r: Result<()>, name: &str) -> Option<BroadcastStageReturnType> {
        if let Err(e) = r {
            match e {
                Error::RecvTimeout(RecvTimeoutError::Disconnected)
                | Error::Send
                | Error::Recv(RecvError) => {
                    return Some(BroadcastStageReturnType::ChannelDisconnected);
                }
                Error::RecvTimeout(RecvTimeoutError::Timeout)
                | Error::ClusterInfo(ClusterInfoError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                _ => {
                    inc_new_counter_error!("streamer-broadcaster-error", 1, 1);
                    error!("{} broadcaster error: {:?}", name, e);
                }
            }
        }
        None
    }

    /// Service to broadcast messages from the leader to layer 1 nodes.
    /// See `cluster_info` for network layer definitions.
    /// # Arguments
    /// * `sock` - Socket to send from.
    /// * `exit` - Boolean to signal system exit.
    /// * `cluster_info` - ClusterInfo structure
    /// * `window` - Cache of Shreds that we have broadcast
    /// * `receiver` - Receive channel for Shreds to be retransmitted to all the layer 1 nodes.
    /// * `exit_sender` - Set to true when this service exits, allows rest of Tpu to exit cleanly.
    /// Otherwise, when a Tpu closes, it only closes the stages that come after it. The stages
    /// that come before could be blocked on a receive, and never notice that they need to
    /// exit. Now, if any stage of the Tpu closes, it will lead to closing the WriteStage (b/c
    /// WriteStage is the last stage in the pipeline), which will then close Broadcast service,
    /// which will then close FetchStage in the Tpu, and then the rest of the Tpu,
    /// completing the cycle.
    #[allow(clippy::too_many_arguments)]
    fn new(
        socks: Vec<UdpSocket>,
        cluster_info: Arc<ClusterInfo>,
        receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: Receiver<Slot>,
        exit: Arc<AtomicBool>,
        blockstore: Arc<Blockstore>,
        bank_forks: Arc<RwLock<BankForks>>,
        quic_endpoint_sender: AsyncSender<(SocketAddr, Bytes)>,
        broadcast_stage_run: impl BroadcastRun + Send + 'static + Clone,
    ) -> Self {
        let (socket_sender, socket_receiver) = unbounded();
        let (blockstore_sender, blockstore_receiver) = unbounded();
        let bs_run = broadcast_stage_run.clone();

        let socket_sender_ = socket_sender.clone();
        let thread_hdl = {
            let blockstore = blockstore.clone();
            let cluster_info = cluster_info.clone();
            Builder::new()
                .name("solBroadcast".to_string())
                .spawn(move || {
                    let _finalizer = Finalizer::new(exit);
                    Self::run(
                        cluster_info,
                        &blockstore,
                        &receiver,
                        &socket_sender_,
                        &blockstore_sender,
                        bs_run,
                    )
                })
                .unwrap()
        };
        let mut thread_hdls = vec![thread_hdl];
        thread_hdls.extend(socks.into_iter().map(|sock| {
            let socket_receiver = socket_receiver.clone();
            let mut bs_transmit = broadcast_stage_run.clone();
            let cluster_info = cluster_info.clone();
            let bank_forks = bank_forks.clone();
            let quic_endpoint_sender = quic_endpoint_sender.clone();
            let run_transmit = move || loop {
                let res = bs_transmit.transmit(
                    &socket_receiver,
                    &cluster_info,
                    &sock,
                    &bank_forks,
                    &quic_endpoint_sender,
                );
                let res = Self::handle_error(res, "solana-broadcaster-transmit");
                if let Some(res) = res {
                    return res;
                }
            };
            Builder::new()
                .name("solBroadcastTx".to_string())
                .spawn(run_transmit)
                .unwrap()
        }));
        thread_hdls.extend(
            repeat_with(|| {
                let blockstore_receiver = blockstore_receiver.clone();
                let mut bs_record = broadcast_stage_run.clone();
                let btree = blockstore.clone();
                let run_record = move || loop {
                    let res = bs_record.record(&blockstore_receiver, &btree);
                    let res = Self::handle_error(res, "solana-broadcaster-record");
                    if let Some(res) = res {
                        return res;
                    }
                };
                Builder::new()
                    .name("solBroadcastRec".to_string())
                    .spawn(run_record)
                    .unwrap()
            })
            .take(NUM_INSERT_THREADS),
        );
        let retransmit_thread = Builder::new()
            .name("solBroadcastRtx".to_string())
            .spawn(move || loop {
                if let Some(res) = Self::handle_error(
                    Self::check_retransmit_signals(
                        &blockstore,
                        &retransmit_slots_receiver,
                        &socket_sender,
                    ),
                    "solana-broadcaster-retransmit-check_retransmit_signals",
                ) {
                    return res;
                }
            })
            .unwrap();

        thread_hdls.push(retransmit_thread);
        Self { thread_hdls }
    }

    fn check_retransmit_signals(
        blockstore: &Blockstore,
        retransmit_slots_receiver: &Receiver<Slot>,
        socket_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        const RECV_TIMEOUT: Duration = Duration::from_millis(100);
        let retransmit_slots: HashSet<Slot> =
            std::iter::once(retransmit_slots_receiver.recv_timeout(RECV_TIMEOUT)?)
                .chain(retransmit_slots_receiver.try_iter())
                .collect();

        for new_retransmit_slot in retransmit_slots {
            let data_shreds = Arc::new(
                blockstore
                    .get_data_shreds_for_slot(new_retransmit_slot, 0)
                    .expect("My own shreds must be reconstructable"),
            );
            debug_assert!(data_shreds
                .iter()
                .all(|shred| shred.slot() == new_retransmit_slot));
            if !data_shreds.is_empty() {
                socket_sender.send((data_shreds, None))?;
            }

            let coding_shreds = Arc::new(
                blockstore
                    .get_coding_shreds_for_slot(new_retransmit_slot, 0)
                    .expect("My own shreds must be reconstructable"),
            );

            debug_assert!(coding_shreds
                .iter()
                .all(|shred| shred.slot() == new_retransmit_slot));
            if !coding_shreds.is_empty() {
                socket_sender.send((coding_shreds, None))?;
            }
        }

        Ok(())
    }

    pub fn join(self) -> thread::Result<BroadcastStageReturnType> {
        for thread_hdl in self.thread_hdls.into_iter() {
            let _ = thread_hdl.join();
        }
        Ok(BroadcastStageReturnType::ChannelDisconnected)
    }
}

fn update_peer_stats(
    cluster_nodes: &ClusterNodes<BroadcastStage>,
    last_datapoint_submit: &AtomicInterval,
) {
    if last_datapoint_submit.should_update(1000) {
        cluster_nodes.submit_metrics("cluster_nodes_broadcast", timestamp());
    }
}

/// Broadcasts shreds from the leader (i.e. this node) to the root of the
/// turbine retransmit tree for each shred.
pub fn broadcast_shreds(
    s: &UdpSocket,
    shreds: &[Shred],
    cluster_nodes_cache: &ClusterNodesCache<BroadcastStage>,
    last_datapoint_submit: &AtomicInterval,
    transmit_stats: &mut TransmitShredsStats,
    cluster_info: &ClusterInfo,
    bank_forks: &RwLock<BankForks>,
    socket_addr_space: &SocketAddrSpace,
    quic_endpoint_sender: &AsyncSender<(SocketAddr, Bytes)>,
) -> Result<()> {
    let mut result = Ok(());
    let mut shred_select = Measure::start("shred_select");
    let (root_bank, working_bank) = {
        let bank_forks = bank_forks.read().unwrap();
        (bank_forks.root_bank(), bank_forks.working_bank())
    };
    let (packets, quic_packets): (Vec<_>, Vec<_>) = shreds
        .iter()
        .group_by(|shred| shred.slot())
        .into_iter()
        .flat_map(|(slot, shreds)| {
            let cluster_nodes =
                cluster_nodes_cache.get(slot, &root_bank, &working_bank, cluster_info);
            update_peer_stats(&cluster_nodes, last_datapoint_submit);
            shreds.filter_map(move |shred| {
                let key = shred.id();
                let protocol = cluster_nodes::get_broadcast_protocol(&key);
                cluster_nodes
                    .get_broadcast_peer(&key)?
                    .tvu(protocol)
                    .ok()
                    .filter(|addr| socket_addr_space.check(addr))
                    .map(|addr| {
                        (match protocol {
                            Protocol::QUIC => Either::Right,
                            Protocol::UDP => Either::Left,
                        })((shred.payload(), addr))
                    })
            })
        })
        .partition_map(std::convert::identity);
    shred_select.stop();
    transmit_stats.shred_select += shred_select.as_us();

    let mut send_mmsg_time = Measure::start("send_mmsg");
    if let Err(SendPktsError::IoError(ioerr, num_failed)) = batch_send(s, &packets[..]) {
        transmit_stats.dropped_packets_udp += num_failed;
        result = Err(Error::Io(ioerr));
    }
    send_mmsg_time.stop();
    transmit_stats.send_mmsg_elapsed += send_mmsg_time.as_us();
    transmit_stats.total_packets += packets.len() + quic_packets.len();
    for (shred, addr) in quic_packets {
        let shred = Bytes::from(shred.clone());
        if let Err(err) = quic_endpoint_sender.blocking_send((addr, shred)) {
            transmit_stats.dropped_packets_quic += 1;
            result = Err(Error::from(err));
        }
    }
    result
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(_: crossbeam_channel::SendError<T>) -> Error {
        Error::Send
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(_: tokio::sync::mpsc::error::SendError<T>) -> Error {
        Error::Send
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crossbeam_channel::unbounded,
        solana_entry::entry::create_ticks,
        solana_gossip::cluster_info::{ClusterInfo, Node},
        solana_ledger::{
            blockstore::Blockstore,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path,
            shred::{max_ticks_per_n_shreds, ProcessShredsStats, ReedSolomonCache, Shredder},
        },
        solana_runtime::bank::Bank,
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
        },
        std::{
            path::Path,
            sync::{atomic::AtomicBool, Arc},
            thread::sleep,
        },
    };

    #[allow(clippy::implicit_hasher)]
    #[allow(clippy::type_complexity)]
    fn make_transmit_shreds(
        slot: Slot,
        num: u64,
    ) -> (
        Vec<Shred>,
        Vec<Shred>,
        Vec<Arc<Vec<Shred>>>,
        Vec<Arc<Vec<Shred>>>,
    ) {
        let num_entries = max_ticks_per_n_shreds(num, None);
        let entries = create_ticks(num_entries, /*hashes_per_tick:*/ 0, Hash::default());
        let shredder = Shredder::new(
            slot, /*parent_slot:*/ 0, /*reference_tick:*/ 0, /*version:*/ 0,
        )
        .unwrap();
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &Keypair::new(),
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index,
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        (
            data_shreds.clone(),
            coding_shreds.clone(),
            data_shreds
                .into_iter()
                .map(|shred| Arc::new(vec![shred]))
                .collect(),
            coding_shreds
                .into_iter()
                .map(|shred| Arc::new(vec![shred]))
                .collect(),
        )
    }

    fn check_all_shreds_received(
        transmit_receiver: &TransmitReceiver,
        mut data_index: u64,
        mut coding_index: u64,
        num_expected_data_shreds: u64,
        num_expected_coding_shreds: u64,
    ) {
        while let Ok((shreds, _)) = transmit_receiver.try_recv() {
            if shreds[0].is_data() {
                for data_shred in shreds.iter() {
                    assert_eq!(data_shred.index() as u64, data_index);
                    data_index += 1;
                }
            } else {
                assert_eq!(shreds[0].index() as u64, coding_index);
                for coding_shred in shreds.iter() {
                    assert_eq!(coding_shred.index() as u64, coding_index);
                    coding_index += 1;
                }
            }
        }

        assert_eq!(num_expected_data_shreds, data_index);
        assert_eq!(num_expected_coding_shreds, coding_index);
    }

    #[test]
    fn test_duplicate_retransmit_signal() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let (transmit_sender, transmit_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();

        // Make some shreds
        let updated_slot = 0;
        let (all_data_shreds, all_coding_shreds, _, _all_coding_transmit_shreds) =
            make_transmit_shreds(updated_slot, 10);
        let num_data_shreds = all_data_shreds.len();
        let num_coding_shreds = all_coding_shreds.len();
        assert!(num_data_shreds >= 10);

        // Insert all the shreds
        blockstore
            .insert_shreds(all_data_shreds, None, true)
            .unwrap();
        blockstore
            .insert_shreds(all_coding_shreds, None, true)
            .unwrap();

        // Insert duplicate retransmit signal, blocks should
        // only be retransmitted once
        retransmit_slots_sender.send(updated_slot).unwrap();
        retransmit_slots_sender.send(updated_slot).unwrap();
        BroadcastStage::check_retransmit_signals(
            &blockstore,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .unwrap();
        // Check all the data shreds were received only once
        check_all_shreds_received(
            &transmit_receiver,
            0,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    struct MockBroadcastStage {
        blockstore: Arc<Blockstore>,
        broadcast_service: BroadcastStage,
        bank: Arc<Bank>,
    }

    fn setup_dummy_broadcast_service(
        leader_keypair: Arc<Keypair>,
        ledger_path: &Path,
        entry_receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: Receiver<Slot>,
    ) -> MockBroadcastStage {
        // Make the database ledger
        let blockstore = Arc::new(Blockstore::open(ledger_path).unwrap());
        let (quic_endpoint_sender, _quic_endpoint_receiver) =
            tokio::sync::mpsc::channel(/*capacity:*/ 128);

        // Make the leader node and scheduler
        let leader_info = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());

        // Make a node to broadcast to
        let buddy_keypair = Keypair::new();
        let broadcast_buddy = Node::new_localhost_with_pubkey(&buddy_keypair.pubkey());

        // Fill the cluster_info with the buddy's info
        let cluster_info = ClusterInfo::new(
            leader_info.info.clone(),
            leader_keypair,
            SocketAddrSpace::Unspecified,
        );
        cluster_info.insert_info(broadcast_buddy.info);
        let cluster_info = Arc::new(cluster_info);

        let exit_sender = Arc::new(AtomicBool::new(false));

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().root_bank();

        // Start up the broadcast stage
        let broadcast_service = BroadcastStage::new(
            leader_info.sockets.broadcast,
            cluster_info,
            entry_receiver,
            retransmit_slots_receiver,
            exit_sender,
            blockstore.clone(),
            bank_forks,
            quic_endpoint_sender,
            StandardBroadcastRun::new(0),
        );

        MockBroadcastStage {
            blockstore,
            broadcast_service,
            bank,
        }
    }

    #[test]
    fn test_broadcast_ledger() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path!();

        {
            // Create the leader scheduler
            let leader_keypair = Arc::new(Keypair::new());

            let (entry_sender, entry_receiver) = unbounded();
            let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
            let broadcast_service = setup_dummy_broadcast_service(
                leader_keypair,
                &ledger_path,
                entry_receiver,
                retransmit_slots_receiver,
            );
            let start_tick_height;
            let max_tick_height;
            let ticks_per_slot;
            let slot;
            {
                let bank = broadcast_service.bank;
                start_tick_height = bank.tick_height();
                max_tick_height = bank.max_tick_height();
                ticks_per_slot = bank.ticks_per_slot();
                slot = bank.slot();
                let ticks = create_ticks(max_tick_height - start_tick_height, 0, Hash::default());
                for (i, tick) in ticks.into_iter().enumerate() {
                    entry_sender
                        .send((bank.clone(), (tick, i as u64 + 1)))
                        .expect("Expect successful send to broadcast service");
                }
            }

            trace!(
                "[broadcast_ledger] max_tick_height: {}, start_tick_height: {}, ticks_per_slot: {}",
                max_tick_height,
                start_tick_height,
                ticks_per_slot,
            );

            let mut entries = vec![];
            for _ in 0..10 {
                entries = broadcast_service
                    .blockstore
                    .get_slot_entries(slot, 0)
                    .expect("Expect entries to be present");
                if entries.len() >= max_tick_height as usize {
                    break;
                }
                sleep(Duration::from_millis(1000));
            }
            assert_eq!(entries.len(), max_tick_height as usize);

            drop(entry_sender);
            drop(retransmit_slots_sender);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
