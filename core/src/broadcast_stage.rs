//! A stage to broadcast data from a leader node to validators
use self::{
    broadcast_fake_shreds_run::BroadcastFakeShredsRun, broadcast_metrics::*,
    fail_entry_verification_broadcast_run::FailEntryVerificationBroadcastRun,
    standard_broadcast_run::StandardBroadcastRun,
};
use crate::contact_info::ContactInfo;
use crate::crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS;
use crate::weighted_shuffle::weighted_best;
use crate::{
    cluster_info::{ClusterInfo, ClusterInfoError},
    poh_recorder::WorkingBankEntry,
    result::{Error, Result},
};
use crossbeam_channel::{
    Receiver as CrossbeamReceiver, RecvTimeoutError as CrossbeamRecvTimeoutError,
    Sender as CrossbeamSender,
};
use solana_ledger::{blockstore::Blockstore, shred::Shred, staking_utils};
use solana_measure::measure::Measure;
use solana_metrics::{inc_new_counter_error, inc_new_counter_info};
use solana_runtime::bank::Bank;
use solana_sdk::timing::timestamp;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use solana_streamer::sendmmsg::send_mmsg;
use std::sync::atomic::AtomicU64;
use std::{
    collections::HashMap,
    net::UdpSocket,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, RecvError, RecvTimeoutError, Sender},
    sync::{Arc, Mutex},
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

mod broadcast_fake_shreds_run;
pub(crate) mod broadcast_metrics;
pub(crate) mod broadcast_utils;
mod fail_entry_verification_broadcast_run;
mod standard_broadcast_run;

pub(crate) const NUM_INSERT_THREADS: usize = 2;
pub(crate) type RetransmitSlotsSender = CrossbeamSender<HashMap<Slot, Arc<Bank>>>;
pub(crate) type RetransmitSlotsReceiver = CrossbeamReceiver<HashMap<Slot, Arc<Bank>>>;
pub(crate) type RecordReceiver = Receiver<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>;
pub(crate) type TransmitReceiver = Receiver<(TransmitShreds, Option<BroadcastShredBatchInfo>)>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    ChannelDisconnected,
}

#[derive(PartialEq, Clone, Debug)]
pub enum BroadcastStageType {
    Standard,
    FailEntryVerification,
    BroadcastFakeShreds,
}

impl BroadcastStageType {
    pub fn new_broadcast_stage(
        &self,
        sock: Vec<UdpSocket>,
        cluster_info: Arc<ClusterInfo>,
        receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: RetransmitSlotsReceiver,
        exit_sender: &Arc<AtomicBool>,
        blockstore: &Arc<Blockstore>,
        shred_version: u16,
    ) -> BroadcastStage {
        let keypair = cluster_info.keypair.clone();
        match self {
            BroadcastStageType::Standard => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                StandardBroadcastRun::new(keypair, shred_version),
            ),

            BroadcastStageType::FailEntryVerification => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                FailEntryVerificationBroadcastRun::new(keypair, shred_version),
            ),

            BroadcastStageType::BroadcastFakeShreds => BroadcastStage::new(
                sock,
                cluster_info,
                receiver,
                retransmit_slots_receiver,
                exit_sender,
                blockstore,
                BroadcastFakeShredsRun::new(keypair, 0, shred_version),
            ),
        }
    }
}

pub type TransmitShreds = (Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>);
trait BroadcastRun {
    fn run(
        &mut self,
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()>;
    fn transmit(
        &mut self,
        receiver: &Arc<Mutex<TransmitReceiver>>,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
    ) -> Result<()>;
    fn record(
        &mut self,
        receiver: &Arc<Mutex<RecordReceiver>>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<()>;
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
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        mut broadcast_stage_run: impl BroadcastRun,
    ) -> BroadcastStageReturnType {
        loop {
            let res =
                broadcast_stage_run.run(blockstore, receiver, socket_sender, blockstore_sender);
            let res = Self::handle_error(res, "run");
            if let Some(res) = res {
                return res;
            }
        }
    }
    fn handle_error(r: Result<()>, name: &str) -> Option<BroadcastStageReturnType> {
        if let Err(e) = r {
            match e {
                Error::RecvTimeoutError(RecvTimeoutError::Disconnected)
                | Error::SendError
                | Error::RecvError(RecvError)
                | Error::CrossbeamRecvTimeoutError(CrossbeamRecvTimeoutError::Disconnected) => {
                    return Some(BroadcastStageReturnType::ChannelDisconnected);
                }
                Error::RecvTimeoutError(RecvTimeoutError::Timeout)
                | Error::CrossbeamRecvTimeoutError(CrossbeamRecvTimeoutError::Timeout) => (),
                Error::ClusterInfoError(ClusterInfoError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
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
        retransmit_slots_receiver: RetransmitSlotsReceiver,
        exit_sender: &Arc<AtomicBool>,
        blockstore: &Arc<Blockstore>,
        broadcast_stage_run: impl BroadcastRun + Send + 'static + Clone,
    ) -> Self {
        let btree = blockstore.clone();
        let exit = exit_sender.clone();
        let (socket_sender, socket_receiver) = channel();
        let (blockstore_sender, blockstore_receiver) = channel();
        let bs_run = broadcast_stage_run.clone();

        let socket_sender_ = socket_sender.clone();
        let thread_hdl = Builder::new()
            .name("solana-broadcaster".to_string())
            .spawn(move || {
                let _finalizer = Finalizer::new(exit);
                Self::run(
                    &btree,
                    &receiver,
                    &socket_sender_,
                    &blockstore_sender,
                    bs_run,
                )
            })
            .unwrap();
        let mut thread_hdls = vec![thread_hdl];
        let socket_receiver = Arc::new(Mutex::new(socket_receiver));
        for sock in socks.into_iter() {
            let socket_receiver = socket_receiver.clone();
            let mut bs_transmit = broadcast_stage_run.clone();
            let cluster_info = cluster_info.clone();
            let t = Builder::new()
                .name("solana-broadcaster-transmit".to_string())
                .spawn(move || loop {
                    let res = bs_transmit.transmit(&socket_receiver, &cluster_info, &sock);
                    let res = Self::handle_error(res, "solana-broadcaster-transmit");
                    if let Some(res) = res {
                        return res;
                    }
                })
                .unwrap();
            thread_hdls.push(t);
        }
        let blockstore_receiver = Arc::new(Mutex::new(blockstore_receiver));
        for _ in 0..NUM_INSERT_THREADS {
            let blockstore_receiver = blockstore_receiver.clone();
            let mut bs_record = broadcast_stage_run.clone();
            let btree = blockstore.clone();
            let t = Builder::new()
                .name("solana-broadcaster-record".to_string())
                .spawn(move || loop {
                    let res = bs_record.record(&blockstore_receiver, &btree);
                    let res = Self::handle_error(res, "solana-broadcaster-record");
                    if let Some(res) = res {
                        return res;
                    }
                })
                .unwrap();
            thread_hdls.push(t);
        }

        let blockstore = blockstore.clone();
        let retransmit_thread = Builder::new()
            .name("solana-broadcaster-retransmit".to_string())
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
        retransmit_slots_receiver: &RetransmitSlotsReceiver,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        let timer = Duration::from_millis(100);

        // Check for a retransmit signal
        let mut retransmit_slots = retransmit_slots_receiver.recv_timeout(timer)?;
        while let Ok(new_retransmit_slots) = retransmit_slots_receiver.try_recv() {
            retransmit_slots.extend(new_retransmit_slots);
        }

        for (_, bank) in retransmit_slots.iter() {
            let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
            let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
            let stakes = stakes.map(Arc::new);
            let data_shreds = Arc::new(
                blockstore
                    .get_data_shreds_for_slot(bank.slot(), 0)
                    .expect("My own shreds must be reconstructable"),
            );

            if !data_shreds.is_empty() {
                socket_sender.send(((stakes.clone(), data_shreds), None))?;
            }

            let coding_shreds = Arc::new(
                blockstore
                    .get_coding_shreds_for_slot(bank.slot(), 0)
                    .expect("My own shreds must be reconstructable"),
            );

            if !coding_shreds.is_empty() {
                socket_sender.send(((stakes.clone(), coding_shreds), None))?;
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
    num_live_peers: i64,
    broadcast_len: i64,
    last_datapoint_submit: &Arc<AtomicU64>,
) {
    let now = timestamp();
    let last = last_datapoint_submit.load(Ordering::Relaxed);
    if now - last > 1000
        && last_datapoint_submit.compare_and_swap(last, now, Ordering::Relaxed) == last
    {
        datapoint_info!(
            "cluster_info-num_nodes",
            ("live_count", num_live_peers, i64),
            ("broadcast_count", broadcast_len, i64)
        );
    }
}

pub fn get_broadcast_peers<S: std::hash::BuildHasher>(
    cluster_info: &ClusterInfo,
    stakes: Option<Arc<HashMap<Pubkey, u64, S>>>,
) -> (Vec<ContactInfo>, Vec<(u64, usize)>) {
    use crate::cluster_info;
    let mut peers = cluster_info.tvu_peers();
    let peers_and_stakes = cluster_info::stake_weight_peers(&mut peers, stakes);
    (peers, peers_and_stakes)
}

/// broadcast messages from the leader to layer 1 nodes
/// # Remarks
pub fn broadcast_shreds(
    s: &UdpSocket,
    shreds: &Arc<Vec<Shred>>,
    peers_and_stakes: &[(u64, usize)],
    peers: &[ContactInfo],
    last_datapoint_submit: &Arc<AtomicU64>,
    send_mmsg_total: &mut u64,
) -> Result<()> {
    let broadcast_len = peers_and_stakes.len();
    if broadcast_len == 0 {
        update_peer_stats(1, 1, last_datapoint_submit);
        return Ok(());
    }
    let packets: Vec<_> = shreds
        .iter()
        .map(|shred| {
            let broadcast_index = weighted_best(&peers_and_stakes, shred.seed());

            (&shred.payload, &peers[broadcast_index].tvu)
        })
        .collect();

    let mut sent = 0;
    let mut send_mmsg_time = Measure::start("send_mmsg");
    while sent < packets.len() {
        match send_mmsg(s, &packets[sent..]) {
            Ok(n) => sent += n,
            Err(e) => {
                return Err(Error::IO(e));
            }
        }
    }
    send_mmsg_time.stop();
    *send_mmsg_total += send_mmsg_time.as_us();

    let num_live_peers = num_live_peers(&peers);
    update_peer_stats(
        num_live_peers,
        broadcast_len as i64 + 1,
        last_datapoint_submit,
    );
    Ok(())
}

fn distance(a: u64, b: u64) -> u64 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

fn num_live_peers(peers: &[ContactInfo]) -> i64 {
    let mut num_live_peers = 1i64;
    peers.iter().for_each(|p| {
        // A peer is considered live if they generated their contact info recently
        if distance(timestamp(), p.wallclock) <= CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS {
            num_live_peers += 1;
        }
    });
    num_live_peers
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::cluster_info::{ClusterInfo, Node};
    use crossbeam_channel::unbounded;
    use solana_ledger::{
        blockstore::{make_slot_entries, Blockstore},
        entry::create_ticks,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
        shred::{max_ticks_per_n_shreds, Shredder, RECOMMENDED_FEC_RATE},
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    };
    use std::{
        path::Path, sync::atomic::AtomicBool, sync::mpsc::channel, sync::Arc, thread::sleep,
    };

    #[allow(clippy::implicit_hasher)]
    pub fn make_transmit_shreds(
        slot: Slot,
        num: u64,
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    ) -> (
        Vec<Shred>,
        Vec<Shred>,
        Vec<TransmitShreds>,
        Vec<TransmitShreds>,
    ) {
        let num_entries = max_ticks_per_n_shreds(num);
        let (data_shreds, _) = make_slot_entries(slot, 0, num_entries);
        let keypair = Arc::new(Keypair::new());
        let shredder = Shredder::new(slot, 0, RECOMMENDED_FEC_RATE, keypair, 0, 0)
            .expect("Expected to create a new shredder");

        let coding_shreds = shredder.data_shreds_to_coding_shreds(&data_shreds[0..]);
        (
            data_shreds.clone(),
            coding_shreds.clone(),
            data_shreds
                .into_iter()
                .map(|s| (stakes.clone(), Arc::new(vec![s])))
                .collect(),
            coding_shreds
                .into_iter()
                .map(|s| (stakes.clone(), Arc::new(vec![s])))
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
        while let Ok((new_retransmit_slots, _)) = transmit_receiver.try_recv() {
            if new_retransmit_slots.1[0].is_data() {
                for data_shred in new_retransmit_slots.1.iter() {
                    assert_eq!(data_shred.index() as u64, data_index);
                    data_index += 1;
                }
            } else {
                assert_eq!(new_retransmit_slots.1[0].index() as u64, coding_index);
                for coding_shred in new_retransmit_slots.1.iter() {
                    assert_eq!(coding_shred.index() as u64, coding_index);
                    coding_index += 1;
                }
            }
        }

        assert_eq!(num_expected_data_shreds, data_index);
        assert_eq!(num_expected_coding_shreds, coding_index);
    }

    #[test]
    fn test_num_live_peers() {
        let mut ci = ContactInfo::default();
        ci.wallclock = std::u64::MAX;
        assert_eq!(num_live_peers(&[ci.clone()]), 1);
        ci.wallclock = timestamp() - 1;
        assert_eq!(num_live_peers(&[ci.clone()]), 2);
        ci.wallclock = timestamp() - CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS - 1;
        assert_eq!(num_live_peers(&[ci]), 1);
    }

    #[test]
    fn test_duplicate_retransmit_signal() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let (transmit_sender, transmit_receiver) = channel();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

        // Make some shreds
        let updated_slot = 0;
        let (all_data_shreds, all_coding_shreds, _, _all_coding_transmit_shreds) =
            make_transmit_shreds(updated_slot, 10, None);
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
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0)].into_iter().collect())
            .unwrap();
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
        leader_pubkey: &Pubkey,
        ledger_path: &Path,
        entry_receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: RetransmitSlotsReceiver,
    ) -> MockBroadcastStage {
        // Make the database ledger
        let blockstore = Arc::new(Blockstore::open(ledger_path).unwrap());

        // Make the leader node and scheduler
        let leader_info = Node::new_localhost_with_pubkey(leader_pubkey);

        // Make a node to broadcast to
        let buddy_keypair = Keypair::new();
        let broadcast_buddy = Node::new_localhost_with_pubkey(&buddy_keypair.pubkey());

        // Fill the cluster_info with the buddy's info
        let cluster_info = ClusterInfo::new_with_invalid_keypair(leader_info.info.clone());
        cluster_info.insert_info(broadcast_buddy.info);
        let cluster_info = Arc::new(cluster_info);

        let exit_sender = Arc::new(AtomicBool::new(false));

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new(&genesis_config));

        let leader_keypair = cluster_info.keypair.clone();
        // Start up the broadcast stage
        let broadcast_service = BroadcastStage::new(
            leader_info.sockets.broadcast,
            cluster_info,
            entry_receiver,
            retransmit_slots_receiver,
            &exit_sender,
            &blockstore,
            StandardBroadcastRun::new(leader_keypair, 0),
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
            let leader_keypair = Keypair::new();

            let (entry_sender, entry_receiver) = channel();
            let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
            let broadcast_service = setup_dummy_broadcast_service(
                &leader_keypair.pubkey(),
                &ledger_path,
                entry_receiver,
                retransmit_slots_receiver,
            );
            let start_tick_height;
            let max_tick_height;
            let ticks_per_slot;
            let slot;
            {
                let bank = broadcast_service.bank.clone();
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

            sleep(Duration::from_millis(2000));

            trace!(
                "[broadcast_ledger] max_tick_height: {}, start_tick_height: {}, ticks_per_slot: {}",
                max_tick_height,
                start_tick_height,
                ticks_per_slot,
            );

            let blockstore = broadcast_service.blockstore;
            let entries = blockstore
                .get_slot_entries(slot, 0)
                .expect("Expect entries to be present");
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
