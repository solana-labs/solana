use {
    super::*,
    crate::cluster_nodes::ClusterNodesCache,
    solana_ledger::shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
    solana_sdk::{hash::Hash, signature::Keypair},
    std::{thread::sleep, time::Duration},
    tokio::sync::mpsc::Sender as AsyncSender,
};

pub const NUM_BAD_SLOTS: u64 = 10;
pub const SLOT_TO_RESOLVE: u64 = 32;

#[derive(Clone)]
pub(super) struct FailEntryVerificationBroadcastRun {
    shred_version: u16,
    good_shreds: Vec<Shred>,
    current_slot: Slot,
    next_shred_index: u32,
    next_code_index: u32,
    cluster_nodes_cache: Arc<ClusterNodesCache<BroadcastStage>>,
    reed_solomon_cache: Arc<ReedSolomonCache>,
}

impl FailEntryVerificationBroadcastRun {
    pub(super) fn new(shred_version: u16) -> Self {
        let cluster_nodes_cache = Arc::new(ClusterNodesCache::<BroadcastStage>::new(
            CLUSTER_NODES_CACHE_NUM_EPOCH_CAP,
            CLUSTER_NODES_CACHE_TTL,
        ));
        Self {
            shred_version,
            good_shreds: vec![],
            current_slot: 0,
            next_shred_index: 0,
            next_code_index: 0,
            cluster_nodes_cache,
            reed_solomon_cache: Arc::<ReedSolomonCache>::default(),
        }
    }
}

impl BroadcastRun for FailEntryVerificationBroadcastRun {
    fn run(
        &mut self,
        keypair: &Keypair,
        blockstore: &Blockstore,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;

        if bank.slot() != self.current_slot {
            self.next_shred_index = 0;
            self.next_code_index = 0;
            self.current_slot = bank.slot();
        }

        // 2) If we're past SLOT_TO_RESOLVE, insert the correct shreds so validators can repair
        // and make progress
        if bank.slot() > SLOT_TO_RESOLVE && !self.good_shreds.is_empty() {
            info!("Resolving bad shreds");
            let shreds = std::mem::take(&mut self.good_shreds);
            blockstore_sender.send((Arc::new(shreds), None))?;
        }

        // 3) Convert entries to shreds + generate coding shreds. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        let last_entries = {
            if last_tick_height == bank.max_tick_height() && bank.slot() < NUM_BAD_SLOTS {
                let good_last_entry = receive_results.entries.pop().unwrap();
                let mut bad_last_entry = good_last_entry.clone();
                bad_last_entry.hash = Hash::default();
                Some((good_last_entry, bad_last_entry))
            } else {
                None
            }
        };

        let shredder = Shredder::new(
            bank.slot(),
            bank.parent().unwrap().slot(),
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
            self.shred_version,
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            keypair,
            &receive_results.entries,
            last_tick_height == bank.max_tick_height() && last_entries.is_none(),
            self.next_shred_index,
            self.next_code_index,
            true, // merkle_variant
            &self.reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );

        self.next_shred_index += data_shreds.len() as u32;
        if let Some(index) = coding_shreds.iter().map(Shred::index).max() {
            self.next_code_index = index + 1;
        }
        let last_shreds = last_entries.map(|(good_last_entry, bad_last_entry)| {
            let (good_last_data_shred, _) = shredder.entries_to_shreds(
                keypair,
                &[good_last_entry],
                true,
                self.next_shred_index,
                self.next_code_index,
                true, // merkle_variant
                &self.reed_solomon_cache,
                &mut ProcessShredsStats::default(),
            );
            // Don't mark the last shred as last so that validators won't know
            // that they've gotten all the shreds, and will continue trying to
            // repair.
            let (bad_last_data_shred, _) = shredder.entries_to_shreds(
                keypair,
                &[bad_last_entry],
                false,
                self.next_shred_index,
                self.next_code_index,
                true, // merkle_variant
                &self.reed_solomon_cache,
                &mut ProcessShredsStats::default(),
            );
            self.next_shred_index += 1;
            (good_last_data_shred, bad_last_data_shred)
        });

        let data_shreds = Arc::new(data_shreds);
        blockstore_sender.send((data_shreds.clone(), None))?;
        // 4) Start broadcast step
        socket_sender.send((data_shreds, None))?;
        if let Some((good_last_data_shred, bad_last_data_shred)) = last_shreds {
            // Stash away the good shred so we can rewrite them later
            self.good_shreds.extend(good_last_data_shred.clone());
            let good_last_data_shred = Arc::new(good_last_data_shred);
            let bad_last_data_shred = Arc::new(bad_last_data_shred);
            // Store the good shred so that blockstore will signal ClusterSlots
            // that the slot is complete
            blockstore_sender.send((good_last_data_shred, None))?;
            loop {
                // Wait for slot to be complete
                if blockstore.is_full(bank.slot()) {
                    break;
                }
                sleep(Duration::from_millis(10));
            }
            // Store the bad shred so we serve bad repairs to validators catching up
            blockstore_sender.send((bad_last_data_shred.clone(), None))?;
            // Send bad shreds to rest of network
            socket_sender.send((bad_last_data_shred, None))?;
        }
        Ok(())
    }
    fn transmit(
        &mut self,
        receiver: &TransmitReceiver,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
        bank_forks: &RwLock<BankForks>,
        quic_endpoint_sender: &AsyncSender<(SocketAddr, Bytes)>,
    ) -> Result<()> {
        let (shreds, _) = receiver.recv()?;
        broadcast_shreds(
            sock,
            &shreds,
            &self.cluster_nodes_cache,
            &AtomicInterval::default(),
            &mut TransmitShredsStats::default(),
            cluster_info,
            bank_forks,
            cluster_info.socket_addr_space(),
            quic_endpoint_sender,
        )
    }
    fn record(&mut self, receiver: &RecordReceiver, blockstore: &Blockstore) -> Result<()> {
        let (all_shreds, _) = receiver.recv()?;
        blockstore
            .insert_shreds(all_shreds.to_vec(), None, true)
            .expect("Failed to insert shreds in blockstore");
        Ok(())
    }
}
