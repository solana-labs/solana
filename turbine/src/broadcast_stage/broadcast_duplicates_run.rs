use {
    super::*,
    crate::cluster_nodes::ClusterNodesCache,
    crossbeam_channel::Sender,
    itertools::Itertools,
    solana_entry::entry::Entry,
    solana_ledger::shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
    solana_sdk::{
        hash::Hash,
        signature::{Keypair, Signature, Signer},
        system_transaction,
    },
    std::collections::HashSet,
};

pub const MINIMUM_DUPLICATE_SLOT: Slot = 20;
pub const DUPLICATE_RATE: usize = 10;

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ClusterPartition {
    Stake(u64),
    Pubkey(Vec<Pubkey>),
}

#[derive(Clone, Debug)]
pub struct BroadcastDuplicatesConfig {
    /// Amount of stake (excluding the leader) or a set of validator pubkeys
    /// to send a duplicate version of some slots to.
    /// Note this is sampled from a list of stakes sorted least to greatest.
    pub partition: ClusterPartition,
    /// If passed `Some(receiver)`, will signal all the duplicate slots via the given
    /// `receiver`
    pub duplicate_slot_sender: Option<Sender<Slot>>,
}

#[derive(Clone)]
pub(super) struct BroadcastDuplicatesRun {
    config: BroadcastDuplicatesConfig,
    current_slot: Slot,
    next_shred_index: u32,
    next_code_index: u32,
    shred_version: u16,
    recent_blockhash: Option<Hash>,
    prev_entry_hash: Option<Hash>,
    num_slots_broadcasted: usize,
    cluster_nodes_cache: Arc<ClusterNodesCache<BroadcastStage>>,
    original_last_data_shreds: Arc<Mutex<HashSet<Signature>>>,
    partition_last_data_shreds: Arc<Mutex<HashSet<Signature>>>,
    reed_solomon_cache: Arc<ReedSolomonCache>,
}

impl BroadcastDuplicatesRun {
    pub(super) fn new(shred_version: u16, config: BroadcastDuplicatesConfig) -> Self {
        let cluster_nodes_cache = Arc::new(ClusterNodesCache::<BroadcastStage>::new(
            CLUSTER_NODES_CACHE_NUM_EPOCH_CAP,
            CLUSTER_NODES_CACHE_TTL,
        ));
        Self {
            config,
            next_shred_index: u32::MAX,
            next_code_index: 0,
            shred_version,
            current_slot: 0,
            recent_blockhash: None,
            prev_entry_hash: None,
            num_slots_broadcasted: 0,
            cluster_nodes_cache,
            original_last_data_shreds: Arc::<Mutex<HashSet<Signature>>>::default(),
            partition_last_data_shreds: Arc::<Mutex<HashSet<Signature>>>::default(),
            reed_solomon_cache: Arc::<ReedSolomonCache>::default(),
        }
    }
}

impl BroadcastRun for BroadcastDuplicatesRun {
    fn run(
        &mut self,
        keypair: &Keypair,
        _blockstore: &Blockstore,
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
            self.prev_entry_hash = None;
            self.num_slots_broadcasted += 1;
        }

        if receive_results.entries.is_empty() {
            return Ok(());
        }

        // Update the recent blockhash based on transactions in the entries
        for entry in &receive_results.entries {
            if !entry.transactions.is_empty() {
                self.recent_blockhash = Some(*entry.transactions[0].message.recent_blockhash());
                break;
            }
        }

        // 2) Convert entries to shreds + generate coding shreds. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        let last_entries = {
            if last_tick_height == bank.max_tick_height()
                && bank.slot() > MINIMUM_DUPLICATE_SLOT
                && self.num_slots_broadcasted % DUPLICATE_RATE == 0
                && self.recent_blockhash.is_some()
            {
                let entry_batch_len = receive_results.entries.len();
                let prev_entry_hash =
                    // Try to get second-to-last entry before last tick
                    if entry_batch_len > 1 {
                        Some(receive_results.entries[entry_batch_len - 2].hash)
                    } else {
                        self.prev_entry_hash
                    };

                if let Some(prev_entry_hash) = prev_entry_hash {
                    let original_last_entry = receive_results.entries.pop().unwrap();

                    // Last entry has to be a tick
                    assert!(original_last_entry.is_tick());

                    // Inject an extra entry before the last tick
                    let extra_tx = system_transaction::transfer(
                        keypair,
                        &Pubkey::new_unique(),
                        1,
                        self.recent_blockhash.unwrap(),
                    );
                    let new_extra_entry = Entry::new(&prev_entry_hash, 1, vec![extra_tx]);

                    // This will only work with sleepy tick producer where the hashing
                    // checks in replay are turned off, because we're introducing an extra
                    // hash for the last tick in the `new_extra_entry`.
                    let new_last_entry = Entry::new(
                        &new_extra_entry.hash,
                        original_last_entry.num_hashes,
                        vec![],
                    );

                    Some((original_last_entry, vec![new_extra_entry, new_last_entry]))
                } else {
                    None
                }
            } else {
                None
            }
        };

        self.prev_entry_hash = last_entries
            .as_ref()
            .map(|(original_last_entry, _)| original_last_entry.hash)
            .or_else(|| Some(receive_results.entries.last().unwrap().hash));

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
            false, // merkle_variant
            &self.reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );

        self.next_shred_index += data_shreds.len() as u32;
        if let Some(index) = coding_shreds.iter().map(Shred::index).max() {
            self.next_code_index = index + 1;
        }
        let last_shreds =
            last_entries.map(|(original_last_entry, duplicate_extra_last_entries)| {
                let (original_last_data_shred, _) = shredder.entries_to_shreds(
                    keypair,
                    &[original_last_entry],
                    true,
                    self.next_shred_index,
                    self.next_code_index,
                    false, // merkle_variant
                    &self.reed_solomon_cache,
                    &mut ProcessShredsStats::default(),
                );
                // Don't mark the last shred as last so that validators won't
                // know that they've gotten all the shreds, and will continue
                // trying to repair.
                let (partition_last_data_shred, _) = shredder.entries_to_shreds(
                    keypair,
                    &duplicate_extra_last_entries,
                    true,
                    self.next_shred_index,
                    self.next_code_index,
                    false, // merkle_variant
                    &self.reed_solomon_cache,
                    &mut ProcessShredsStats::default(),
                );
                let sigs: Vec<_> = partition_last_data_shred
                    .iter()
                    .map(|s| (s.signature(), s.index()))
                    .collect();
                info!(
                    "duplicate signatures for slot {}, sigs: {:?}",
                    bank.slot(),
                    sigs,
                );

                self.next_shred_index += 1;
                (original_last_data_shred, partition_last_data_shred)
            });

        let data_shreds = Arc::new(data_shreds);
        blockstore_sender.send((data_shreds.clone(), None))?;

        // 3) Start broadcast step
        info!(
            "{} Sending good shreds for slot {} to network",
            keypair.pubkey(),
            data_shreds.first().unwrap().slot()
        );
        assert!(data_shreds.iter().all(|shred| shred.slot() == bank.slot()));
        socket_sender.send((data_shreds, None))?;

        // Special handling of last shred to cause partition
        if let Some((original_last_data_shred, partition_last_data_shred)) = last_shreds {
            let pubkey = keypair.pubkey();
            self.original_last_data_shreds.lock().unwrap().extend(
                original_last_data_shred.iter().map(|shred| {
                    assert!(shred.verify(&pubkey));
                    shred.signature()
                }),
            );
            self.partition_last_data_shreds.lock().unwrap().extend(
                partition_last_data_shred.iter().map(|shred| {
                    info!("adding {} to partition set", shred.signature());
                    assert!(shred.verify(&pubkey));
                    shred.signature()
                }),
            );
            let original_last_data_shred = Arc::new(original_last_data_shred);
            let partition_last_data_shred = Arc::new(partition_last_data_shred);

            // Store the original shreds that this node replayed
            blockstore_sender.send((original_last_data_shred.clone(), None))?;

            assert!(original_last_data_shred
                .iter()
                .all(|shred| shred.slot() == bank.slot()));
            assert!(partition_last_data_shred
                .iter()
                .all(|shred| shred.slot() == bank.slot()));

            if let Some(duplicate_slot_sender) = &self.config.duplicate_slot_sender {
                let _ = duplicate_slot_sender.send(bank.slot());
            }
            socket_sender.send((original_last_data_shred, None))?;
            socket_sender.send((partition_last_data_shred, None))?;
        }
        Ok(())
    }

    fn transmit(
        &mut self,
        receiver: &TransmitReceiver,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
        bank_forks: &RwLock<BankForks>,
        _quic_endpoint_sender: &AsyncSender<(SocketAddr, Bytes)>,
    ) -> Result<()> {
        let (shreds, _) = receiver.recv()?;
        if shreds.is_empty() {
            return Ok(());
        }
        let slot = shreds.first().unwrap().slot();
        assert!(shreds.iter().all(|shred| shred.slot() == slot));
        let (root_bank, working_bank) = {
            let bank_forks = bank_forks.read().unwrap();
            (bank_forks.root_bank(), bank_forks.working_bank())
        };
        let self_pubkey = cluster_info.id();
        // Create cluster partition.
        let cluster_partition: HashSet<Pubkey> = {
            match &self.config.partition {
                ClusterPartition::Stake(partition_total_stake) => {
                    let mut cumulative_stake = 0;
                    let epoch = root_bank.get_leader_schedule_epoch(slot);
                    root_bank
                        .epoch_staked_nodes(epoch)
                        .unwrap()
                        .iter()
                        .filter(|(pubkey, _)| **pubkey != self_pubkey)
                        .sorted_by_key(|(pubkey, stake)| (**stake, **pubkey))
                        .take_while(|(_, stake)| {
                            cumulative_stake += *stake;
                            cumulative_stake <= *partition_total_stake
                        })
                        .map(|(pubkey, _)| *pubkey)
                        .collect()
                }
                ClusterPartition::Pubkey(pubkeys) => pubkeys.iter().cloned().collect(),
            }
        };

        // Broadcast data
        let cluster_nodes =
            self.cluster_nodes_cache
                .get(slot, &root_bank, &working_bank, cluster_info);
        let socket_addr_space = cluster_info.socket_addr_space();
        let packets: Vec<_> = shreds
            .iter()
            .filter_map(|shred| {
                let node = cluster_nodes.get_broadcast_peer(&shred.id())?;
                if !socket_addr_space.check(&node.tvu(Protocol::UDP).ok()?) {
                    return None;
                }
                if self
                    .original_last_data_shreds
                    .lock()
                    .unwrap()
                    .remove(shred.signature())
                {
                    if cluster_partition.contains(node.pubkey()) {
                        info!(
                            "Not broadcasting original shred index {}, slot {} to partition node {}",
                            shred.index(),
                            shred.slot(),
                            node.pubkey(),
                        );
                        return None;
                    }
                } else if self
                    .partition_last_data_shreds
                    .lock()
                    .unwrap()
                    .remove(shred.signature())
                {
                    // If the shred is part of the partition, broadcast it directly to the
                    // partition node. This is to account for cases when the partition stake
                    // is small such as in `test_duplicate_shreds_broadcast_leader()`, then
                    // the partition node is never selected by get_broadcast_peer()
                    return Some(
                        cluster_partition
                            .iter()
                            .filter_map(|pubkey| {
                                info!(
                                    "Broadcasting partition shred index {}, slot {} to partition node {}",
                                    shred.index(),
                                    shred.slot(),
                                    pubkey,
                                );
                                let tvu = cluster_info
                                    .lookup_contact_info(pubkey, |node| node.tvu(Protocol::UDP))?
                                    .ok()?;
                                Some((shred.payload(), tvu))
                            })
                            .collect(),
                    );
                }

                Some(vec![(shred.payload(), node.tvu(Protocol::UDP).ok()?)])
            })
            .flatten()
            .collect();

        if let Err(SendPktsError::IoError(ioerr, _)) = batch_send(sock, &packets) {
            return Err(Error::Io(ioerr));
        }
        Ok(())
    }

    fn record(&mut self, receiver: &RecordReceiver, blockstore: &Blockstore) -> Result<()> {
        let (all_shreds, _) = receiver.recv()?;
        blockstore
            .insert_shreds(all_shreds.to_vec(), None, true)
            .expect("Failed to insert shreds in blockstore");
        Ok(())
    }
}
