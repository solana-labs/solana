//! A stage to broadcast data from a leader node to validators
use self::{
    broadcast_fake_shreds_run::BroadcastFakeShredsRun,
    fail_entry_verification_broadcast_run::FailEntryVerificationBroadcastRun,
    standard_broadcast_run::StandardBroadcastRun,
};
use crate::{
    cluster_info::{ClusterInfo, ClusterInfoError},
    poh_recorder::WorkingBankEntry,
    replay_stage::MAX_UNCONFIRMED_SLOTS,
    result::{Error, Result},
};
use crossbeam_channel::{
    unbounded, Receiver as CrossbeamReceiver, RecvTimeoutError as CrossbeamRecvTimeoutError,
    Sender as CrossbeamSender,
};
use slot_transmit_shreds_cache::*;
use solana_ledger::{blockstore::Blockstore, shred::Shred, staking_utils};
use solana_metrics::{inc_new_counter_error, inc_new_counter_info};
use solana_runtime::bank::Bank;
use solana_sdk::clock::Slot;
use std::{
    collections::{HashMap, HashSet},
    net::UdpSocket,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, RecvError, RecvTimeoutError, Sender},
    sync::{Arc, Mutex, RwLock},
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

mod broadcast_fake_shreds_run;
pub(crate) mod broadcast_utils;
mod fail_entry_verification_broadcast_run;
mod slot_transmit_shreds_cache;
mod standard_broadcast_run;

pub const NUM_INSERT_THREADS: usize = 2;
pub type RetransmitCacheSender = CrossbeamSender<(Slot, TransmitShreds)>;
pub type RetransmitCacheReceiver = CrossbeamReceiver<(Slot, TransmitShreds)>;
pub type RetransmitSlotsSender = CrossbeamSender<HashMap<Slot, Arc<Bank>>>;
pub type RetransmitSlotsReceiver = CrossbeamReceiver<HashMap<Slot, Arc<Bank>>>;

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
        cluster_info: Arc<RwLock<ClusterInfo>>,
        receiver: Receiver<WorkingBankEntry>,
        retransmit_slots_receiver: RetransmitSlotsReceiver,
        exit_sender: &Arc<AtomicBool>,
        blockstore: &Arc<Blockstore>,
        shred_version: u16,
    ) -> BroadcastStage {
        let keypair = cluster_info.read().unwrap().keypair.clone();
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

trait BroadcastRun {
    fn run(
        &mut self,
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<TransmitShreds>,
        blockstore_sender: &Sender<Arc<Vec<Shred>>>,
        retransmit_cache_sender: &RetransmitCacheSender,
    ) -> Result<()>;
    fn transmit(
        &self,
        receiver: &Arc<Mutex<Receiver<TransmitShreds>>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
    ) -> Result<()>;
    fn record(
        &self,
        receiver: &Arc<Mutex<Receiver<Arc<Vec<Shred>>>>>,
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
        socket_sender: &Sender<TransmitShreds>,
        blockstore_sender: &Sender<Arc<Vec<Shred>>>,
        retransmit_cache_sender: &RetransmitCacheSender,
        mut broadcast_stage_run: impl BroadcastRun,
    ) -> BroadcastStageReturnType {
        loop {
            let res = broadcast_stage_run.run(
                blockstore,
                receiver,
                socket_sender,
                blockstore_sender,
                retransmit_cache_sender,
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
        cluster_info: Arc<RwLock<ClusterInfo>>,
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
        let (retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
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
                    &retransmit_cache_sender,
                    bs_run,
                )
            })
            .unwrap();
        let mut thread_hdls = vec![thread_hdl];
        let socket_receiver = Arc::new(Mutex::new(socket_receiver));
        for sock in socks.into_iter() {
            let socket_receiver = socket_receiver.clone();
            let bs_transmit = broadcast_stage_run.clone();
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
            let bs_record = broadcast_stage_run.clone();
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
            .spawn(move || {
                // Cache of most recently transmitted MAX_UNCONFIRMED_SLOTS number of
                // leader blocks
                let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
                // `unfinished_retransmit_slots` is the set of blocks
                // we got a retransmit signal for from ReplayStage, but didn't
                // have all the shreds in blockstore to retransmit, due to
                // arbitrary latency between the broadcast thread and the thread
                // writing to blockstore.
                let mut unfinished_retransmit_slots = HashSet::new();
                loop {
                    if let Some(res) = Self::run_retransmit(
                        &blockstore,
                        &mut transmit_shreds_cache,
                        &mut unfinished_retransmit_slots,
                        &retransmit_cache_receiver,
                        &retransmit_slots_receiver,
                        &socket_sender,
                    ) {
                        return res;
                    }
                }
            })
            .unwrap();

        thread_hdls.push(retransmit_thread);
        Self { thread_hdls }
    }

    fn run_retransmit(
        blockstore: &Blockstore,
        transmit_shreds_cache: &mut SlotTransmitShredsCache,
        unfinished_retransmit_slots: &mut HashSet<Slot>,
        retransmit_cache_receiver: &RetransmitCacheReceiver,
        retransmit_slots_receiver: &RetransmitSlotsReceiver,
        socket_sender: &Sender<TransmitShreds>,
    ) -> Option<BroadcastStageReturnType> {
        let mut new_updates = HashMap::new();
        // Update the cache with the newest shreds
        let res = Self::handle_error(
            transmit_shreds_cache
                .update_retransmit_cache(retransmit_cache_receiver, &mut new_updates),
            "solana-broadcaster-retransmit-update_retransmit_cache",
        );

        if res.is_some() {
            return res;
        }

        // Retry any unfinished retransmits
        let res = Self::handle_error(
            Self::retry_unfinished_retransmit_slots(
                blockstore,
                new_updates,
                transmit_shreds_cache,
                unfinished_retransmit_slots,
                socket_sender,
            ),
            "solana-broadcaster-retransmit-retry_unfinished_retransmit_slots",
        );
        if res.is_some() {
            return res;
        }

        // Check for new retransmit signals from ReplayStage
        let res = Self::handle_error(
            Self::check_retransmit_signals(
                transmit_shreds_cache,
                unfinished_retransmit_slots,
                blockstore,
                retransmit_slots_receiver,
                socket_sender,
            ),
            "solana-broadcaster-retransmit-check_retransmit_signals",
        );
        if res.is_some() {
            return res;
        }

        None
    }

    fn retry_unfinished_retransmit_slots(
        blockstore: &Blockstore,
        updates: HashMap<Slot, Vec<TransmitShreds>>,
        transmit_shreds_cache: &mut SlotTransmitShredsCache,
        unfinished_retransmit_slots: &mut HashSet<Slot>,
        socket_sender: &Sender<TransmitShreds>,
    ) -> Result<()> {
        // If this block is outdated (no longer in the cache),
        // then there's no longer any need to retransmit the block,
        // so remove it from the `unfinished_retransmit_slots` set
        unfinished_retransmit_slots.retain(|unfinished_retransmit_slot| {
            let slot_cached_shreds = transmit_shreds_cache.get(*unfinished_retransmit_slot);
            slot_cached_shreds.is_some()
        });

        for (updated_slot, all_transmit_shreds) in updates {
            // If there's been no signal to retransmit this slot,
            // then continue
            if !unfinished_retransmit_slots.contains(&updated_slot) {
                continue;
            }

            // Safe to unwrap because:
            // 1) `updated_slot` is a member of `unfinished_retransmit_slots`
            // due to the check above,
            //
            // 2) `unfinished_retransmit_slots` is a subset of
            // `transmit_shreds_cache` because of the `retain()` run at the
            // beginning of this function
            let slot_cached_shreds = transmit_shreds_cache.get(updated_slot).unwrap();
            for transmit_shreds in all_transmit_shreds {
                if transmit_shreds.1.is_empty() {
                    continue;
                }
                socket_sender.send(transmit_shreds)?;
            }

            if slot_cached_shreds.contains_last_shreds() {
                // If the block is now complete, then we've retransmitted
                // all the updates, so we can remove this slot from
                // the `unfinished_retransmit_slots` set
                unfinished_retransmit_slots.remove(&updated_slot);
            }
        }

        // Fetch potential updates from blockstore, necessary for slots that
        // ReplayStage sent a retransmit signal for after that slot was already
        // removed from the `transmit_shreds_cache` (so no updates coming from broadcast thread),
        // but before updates had been written to blockstore
        let updates = transmit_shreds_cache
            .update_cache_from_blockstore(blockstore, &unfinished_retransmit_slots);
        for (slot, cached_updates) in updates {
            // If we got all the shreds, remove this slot's entries
            // from `unfinished_retransmit_slots`, as we now have all
            // the shreds needed for retransmit
            if transmit_shreds_cache
                .get(slot)
                .map(|cache_entry| cache_entry.contains_last_shreds())
                .unwrap_or(false)
            {
                unfinished_retransmit_slots.remove(&slot);
            }
            let all_data_transmit_shreds = cached_updates.to_transmit_shreds();

            for transmit_shreds in all_data_transmit_shreds {
                socket_sender.send(transmit_shreds)?;
            }
        }

        Ok(())
    }

    fn check_retransmit_signals(
        transmit_shreds_cache: &mut SlotTransmitShredsCache,
        unfinished_retransmit_slots: &mut HashSet<Slot>,
        blockstore: &Blockstore,
        retransmit_slots_receiver: &RetransmitSlotsReceiver,
        socket_sender: &Sender<TransmitShreds>,
    ) -> Result<()> {
        let timer = Duration::from_millis(100);

        // Check for a retransmit signal
        let mut retransmit_slots = retransmit_slots_receiver.recv_timeout(timer)?;
        while let Ok(new_retransmit_slots) = retransmit_slots_receiver.try_recv() {
            retransmit_slots.extend(new_retransmit_slots);
        }

        for (_, bank) in retransmit_slots.iter() {
            unfinished_retransmit_slots.remove(&bank.slot());
            let cached_shreds = transmit_shreds_cache.get_or_update(bank, blockstore);
            // If the cached shreds are missing any shreds (broadcast
            // hasn't written them to blockstore yet), add this slot
            // to the `unfinished_retransmit_slots` so we can retry broadcasting
            // the missing shreds later.
            if !cached_shreds.contains_last_shreds() {
                unfinished_retransmit_slots.insert(bank.slot());
            }
            let all_data_transmit_shreds = cached_shreds.to_transmit_shreds();
            for transmit_shreds in all_data_transmit_shreds {
                socket_sender.send(transmit_shreds)?;
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        cluster_info::{ClusterInfo, Node},
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use solana_ledger::{
        blockstore::{make_slot_entries, Blockstore},
        entry::create_ticks,
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
        path::Path,
        sync::atomic::AtomicBool,
        sync::mpsc::channel,
        sync::{Arc, RwLock},
        thread::sleep,
    };

    fn make_transmit_shreds(
        slot: Slot,
        num: u64,
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
                .map(|s| (None, Arc::new(vec![s])))
                .collect(),
            coding_shreds
                .into_iter()
                .map(|s| (None, Arc::new(vec![s])))
                .collect(),
        )
    }

    fn check_all_shreds_received(
        transmit_receiver: &Receiver<TransmitShreds>,
        mut data_index: u64,
        mut coding_index: u64,
        num_expected_data_shreds: u64,
        num_expected_coding_shreds: u64,
    ) {
        while let Ok(new_retransmit_slots) = transmit_receiver.try_recv() {
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
    fn test_empty_signal_updated() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (_retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

        // Ask for retransmit of a slot before any shreds have arrived in the
        // cache or been written to blockstore
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());

        // Check nothing was received
        check_all_shreds_received(&transmit_receiver, 0, 0, 0, 0);

        // Make some shreds
        let updated_slot = 0;
        let (all_data_shreds, all_coding_shreds, _, _) = make_transmit_shreds(updated_slot, 10);
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

        // Signal for retransmit
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();

        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());

        // Check retransmit occurred
        check_all_shreds_received(
            &transmit_receiver,
            0,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    #[test]
    fn test_all_updated() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

        // Make some shreds
        let updated_slot = 0;
        let (
            all_data_shreds,
            all_coding_shreds,
            all_data_transmit_shreds,
            all_coding_transmit_shreds,
        ) = make_transmit_shreds(updated_slot, 10);
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
        for data_transmit_shred in all_data_transmit_shreds {
            retransmit_cache_sender
                .send((updated_slot, data_transmit_shred))
                .unwrap();
        }
        for data_transmit_shred in all_coding_transmit_shreds {
            retransmit_cache_sender
                .send((updated_slot, data_transmit_shred))
                .unwrap();
        }

        // Signal for retransmit
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        check_all_shreds_received(
            &transmit_receiver,
            0,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    #[test]
    fn test_duplicate_retransmit_signal() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (_retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

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
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        // Check all the data shreds were received only once
        check_all_shreds_received(
            &transmit_receiver,
            0,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    #[test]
    fn test_run_retransmit_blockstore_updates() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (_retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

        // Make some shreds
        let updated_slot = 0;
        let (all_data_shreds, all_coding_shreds, _, _all_coding_transmit_shreds) =
            make_transmit_shreds(updated_slot, 10);
        let num_data_shreds = all_data_shreds.len();
        let num_coding_shreds = all_coding_shreds.len();
        assert!(num_data_shreds >= 10);

        // Write the data shreds to blockstore for slot 10, and then a retransmit signal
        // for slot `updated_slot`. The cache should attempt to fill from blockstore, and find it's
        // missing the coding shreds, so `unfinished_retransmit_slots` should contain `updated_slot`.
        blockstore
            .insert_shreds(all_data_shreds, None, true)
            .unwrap();
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        assert!(unfinished_retransmit_slots.contains(&updated_slot));
        // Check all the data shreds were received
        check_all_shreds_received(&transmit_receiver, 0, 0, num_data_shreds as u64, 0);

        // Now write all missing coding shreds to blockstore, the updates should
        // be picked up and broadcasted, the `updated_slot` should be removed
        // from `unfinished_retransmit_slots`
        blockstore
            .insert_shreds(all_coding_shreds, None, true)
            .unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());

        assert!(unfinished_retransmit_slots.is_empty());
        // Check all the coding shreds were received
        check_all_shreds_received(
            &transmit_receiver,
            num_data_shreds as u64,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    #[test]
    fn test_interleaved_blockstore_updates() {
        struct BankShreds {
            bank: Arc<Bank>,
            data_shreds: Vec<Shred>,
            coding_shreds: Vec<Shred>,
            data_transmit_shreds: Vec<TransmitShreds>,
        }

        impl BankShreds {
            fn new(bank: Arc<Bank>) -> Self {
                let (data_shreds, coding_shreds, data_transmit_shreds, _) =
                    make_transmit_shreds(bank.slot(), 10);

                BankShreds {
                    bank,
                    data_shreds,
                    data_transmit_shreds,
                    coding_shreds,
                }
            }
        }

        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();

        // Make 1 more block than the `transmit_shreds_cache` can hold
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let mut banks_and_shreds = HashMap::new();
        banks_and_shreds.insert(0, BankShreds::new(bank0.clone()));
        (1..=MAX_UNCONFIRMED_SLOTS as u64).fold(bank0, |last_bank, slot| {
            let new_bank = Arc::new(Bank::new_from_parent(&last_bank, &Pubkey::default(), slot));
            banks_and_shreds.insert(slot, BankShreds::new(new_bank.clone()));
            new_bank
        });

        // Push and process some partial updates from broadcast for slot 0
        for data_transmit_shred in
            banks_and_shreds.get(&0).unwrap().data_transmit_shreds[0..5].iter()
        {
            retransmit_cache_sender
                .send((0, data_transmit_shred.clone()))
                .unwrap();
        }
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        assert!(transmit_shreds_cache.get(0).is_some());

        // Simulate retransmit signals from ReplayStage for all slots between
        // 1..=MAX_UNCONFIRMED_SLOTS, should remove slot 0 from the cache
        let retransmit_signals: HashMap<Slot, Arc<Bank>> = (1..=MAX_UNCONFIRMED_SLOTS as u64)
            .map(|slot| (slot, banks_and_shreds.get(&slot).unwrap().bank.clone()))
            .collect();
        retransmit_slots_sender.send(retransmit_signals).unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        assert!(transmit_shreds_cache.get(0).is_none());

        // Now broadcast pushes the rest of the updates for slot 0, and push a
        // retransmit signal for slot 0
        for data_transmit_shred in
            banks_and_shreds.get(&0).unwrap().data_transmit_shreds[5..].iter()
        {
            retransmit_cache_sender
                .send((0, data_transmit_shred.clone()))
                .unwrap();
        }
        let slot0_signal: HashMap<Slot, Arc<Bank>> =
            vec![(0, banks_and_shreds.get(&0).unwrap().bank.clone())]
                .into_iter()
                .collect();

        retransmit_slots_sender.send(slot0_signal.clone()).unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        assert!(unfinished_retransmit_slots.contains(&0));

        // Now write updates for slot 0 to blockstore, no retransmits
        // should happen as slot 0 was considered "outdated"
        let data_shreds = banks_and_shreds.get(&0).unwrap().data_shreds.clone();
        let coding_shreds = banks_and_shreds.get(&0).unwrap().coding_shreds.clone();
        let num_data_shreds = data_shreds.len() as u64;
        let num_coding_shreds = coding_shreds.len() as u64;
        blockstore.insert_shreds(data_shreds, None, true).unwrap();
        blockstore.insert_shreds(coding_shreds, None, true).unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());
        check_all_shreds_received(&transmit_receiver, 0, 0, num_data_shreds, num_coding_shreds);
    }

    #[test]
    fn test_run_retransmit_updates() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let mut unfinished_retransmit_slots = HashSet::new();
        let (transmit_sender, transmit_receiver) = channel();
        let (retransmit_cache_sender, retransmit_cache_receiver) = unbounded();
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));

        // Make some shreds
        let updated_slot = 0;
        let (
            all_data_shreds,
            all_coding_shreds,
            all_data_transmit_shreds,
            all_coding_transmit_shreds,
        ) = make_transmit_shreds(updated_slot, 10);
        let num_data_shreds = all_data_shreds.len();
        let num_coding_shreds = all_coding_shreds.len();
        assert!(num_data_shreds >= 10);

        // Send all the data shreds for slot `updated_slot`, and then a retransmit signal
        // for slot `updated_slot`. The cache for slot `updated_slot` is missing coding shreds so the
        // `unfinished_retransmit_slots` should contain slot `updated_slot`
        for data_transmit_shred in all_data_transmit_shreds {
            retransmit_cache_sender
                .send((updated_slot, data_transmit_shred))
                .unwrap();
        }
        retransmit_slots_sender
            .send(vec![(updated_slot, bank0.clone())].into_iter().collect())
            .unwrap();
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());

        assert!(unfinished_retransmit_slots.contains(&updated_slot));

        // Now send all missing coding shreds so the `unfinished_retransmit_slots`
        // should now be empty
        for code_transmit_shred in all_coding_transmit_shreds {
            retransmit_cache_sender
                .send((updated_slot, code_transmit_shred))
                .unwrap();
        }
        assert!(BroadcastStage::run_retransmit(
            &blockstore,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &retransmit_cache_receiver,
            &retransmit_slots_receiver,
            &transmit_sender,
        )
        .is_none());

        assert!(unfinished_retransmit_slots.is_empty());
        check_all_shreds_received(
            &transmit_receiver,
            0,
            0,
            num_data_shreds as u64,
            num_coding_shreds as u64,
        );
    }

    #[test]
    fn test_retry_unfinished_retransmit_slots_with_updates() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        let (transmit_sender, transmit_receiver) = channel();

        // Make some updates
        let updated_slot = 1;
        let (
            all_data_shreds,
            all_coding_shreds,
            all_data_transmit_shreds,
            all_coding_transmit_shreds,
        ) = make_transmit_shreds(updated_slot, 10);
        let mut updates = HashMap::new();
        let num_data_shreds = all_data_shreds.len() as u64;
        let num_coding_shreds = all_coding_shreds.len() as u64;
        assert!(num_data_shreds >= 10);
        updates.insert(updated_slot, all_data_transmit_shreds.clone());

        // `unfinished_retransmit_slots` is empty, so there should be nothing sent, even
        // if there were updates
        let mut unfinished_retransmit_slots = HashSet::new();
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            updates.clone(),
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();
        assert!(transmit_receiver.try_recv().is_err());

        // `unfinished_retransmit_slots` contains the slot that was updated,
        // but `transmit_shreds_cache` doesn't have that slot (implies outdated slot),
        // so no updates will be sent, and the outdated slot should be removed from
        // `unfinished_retransmit_slots`
        unfinished_retransmit_slots.insert(updated_slot);
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            updates.clone(),
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();
        assert!(transmit_receiver.try_recv().is_err());
        assert!(unfinished_retransmit_slots.is_empty());

        // Now `transmit_shreds_cache` contains the slot that was updated,
        // so any updates passed in should be sent. For now, only
        // pass in the data shreds. This means all the updates for the slot
        // were not yet received (missing coding shreds), so the slot should
        // not be purged from `unfinished_retransmit_slots`
        unfinished_retransmit_slots.insert(updated_slot);
        for transmit_shreds in all_data_transmit_shreds.into_iter() {
            transmit_shreds_cache.push(updated_slot, transmit_shreds);
        }
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            updates.clone(),
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();
        check_all_shreds_received(&transmit_receiver, 0, 0, num_data_shreds, 0);
        assert!(unfinished_retransmit_slots.contains(&updated_slot));

        // Now push the coding shreds into the cache.
        // All the updates for the slot have now been received,
        // so the slot should be purged from `unfinished_retransmit_slots`
        for transmit_shreds in all_coding_transmit_shreds.clone() {
            transmit_shreds_cache.push(updated_slot, transmit_shreds);
        }
        updates.clear();
        updates.insert(updated_slot, all_coding_transmit_shreds);
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            updates.clone(),
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();
        check_all_shreds_received(
            &transmit_receiver,
            num_data_shreds,
            0,
            num_data_shreds,
            num_coding_shreds,
        );
        assert!(unfinished_retransmit_slots.is_empty());

        // If `unfinished_retransmit_slots` is non-empty but doesn't contain the slot to
        // be updated, should get no updates
        unfinished_retransmit_slots.insert(updated_slot + 1);
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            updates,
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();
        assert!(transmit_receiver.try_recv().is_err());
    }

    #[test]
    fn test_retry_unfinished_retransmit_slots_with_blockstore_updates() {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let (transmit_sender, transmit_receiver) = channel();

        // Make some updates
        let updated_slot = 1;
        let (all_data_shreds, all_coding_shreds, all_data_transmit_shreds, _) =
            make_transmit_shreds(updated_slot, 10);
        let num_data_shreds = all_data_shreds.len() as u64;
        assert!(num_data_shreds >= 10);
        let num_coding_shreds = all_coding_shreds.len() as u64;

        let mut unfinished_retransmit_slots = HashSet::new();
        unfinished_retransmit_slots.insert(updated_slot);
        // `transmit_shreds_cache` contains the first shred, but is
        // missing the rest
        let mut transmit_shreds_cache = SlotTransmitShredsCache::new(MAX_UNCONFIRMED_SLOTS);
        transmit_shreds_cache.push(updated_slot, all_data_transmit_shreds[0].clone());

        // Write all the shreds to blockstore
        blockstore
            .insert_shreds(all_data_shreds, None, true)
            .unwrap();
        blockstore
            .insert_shreds(all_coding_shreds, None, true)
            .unwrap();

        // Now even though there are no updates passed in, the slot should be
        // updated by the blockstore and those updates sent.
        BroadcastStage::retry_unfinished_retransmit_slots(
            &blockstore,
            HashMap::new(),
            &mut transmit_shreds_cache,
            &mut unfinished_retransmit_slots,
            &transmit_sender,
        )
        .unwrap();

        // We have the first data shred in the cache already, but everything else
        // should be sent
        let start_data_index = 1;
        let start_coding_index = 0;
        check_all_shreds_received(
            &transmit_receiver,
            start_data_index,
            start_coding_index,
            num_data_shreds,
            num_coding_shreds,
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
        let mut cluster_info = ClusterInfo::new_with_invalid_keypair(leader_info.info.clone());
        cluster_info.insert_info(broadcast_buddy.info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let exit_sender = Arc::new(AtomicBool::new(false));

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new(&genesis_config));

        let leader_keypair = cluster_info.read().unwrap().keypair.clone();
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
            let (_retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
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
            let (entries, _, _) = blockstore
                .get_slot_entries_with_shred_info(slot, 0)
                .expect("Expect entries to be present");
            assert_eq!(entries.len(), max_tick_height as usize);

            drop(entry_sender);
            broadcast_service
                .broadcast_service
                .join()
                .expect("Expect successful join of broadcast service");
        }

        Blockstore::destroy(&ledger_path).expect("Expected successful database destruction");
    }
}
