use {
    crate::{
        duplicate_shred::{self, DuplicateShred, Error},
        duplicate_shred_listener::DuplicateShredHandlerTrait,
    },
    crossbeam_channel::Sender,
    log::error,
    solana_ledger::{blockstore::Blockstore, leader_schedule_cache::LeaderScheduleCache},
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{
        clock::{Epoch, Slot},
        pubkey::Pubkey,
    },
    std::{
        cmp::Reverse,
        collections::HashMap,
        sync::{Arc, RwLock},
    },
};

// Normally num_chunks is 3, because there are two shreds (each is one packet)
// and meta data. So we discard anything larger than 3 chunks.
const MAX_NUM_CHUNKS: usize = 3;
// Limit number of entries per node.
const MAX_NUM_ENTRIES_PER_PUBKEY: usize = 128;
const BUFFER_CAPACITY: usize = 512 * MAX_NUM_ENTRIES_PER_PUBKEY;

type BufferEntry = [Option<DuplicateShred>; MAX_NUM_CHUNKS];

pub struct DuplicateShredHandler {
    // Because we use UDP for packet transfer, we can normally only send ~1500 bytes
    // in each packet. We send both shreds and meta data in duplicate shred proof, and
    // each shred is normally 1 packet(1500 bytes), so the whole proof is larger than
    // 1 packet and it needs to be cut down as chunks for transfer. So we need to piece
    // together the chunks into the original proof before anything useful is done.
    buffer: HashMap<(Slot, Pubkey), BufferEntry>,
    // Slots for which a duplicate proof is already ingested.
    consumed: HashMap<Slot, bool>,
    // Cache last root to reduce read lock.
    last_root: Slot,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    bank_forks: Arc<RwLock<BankForks>>,
    // Cache information from root bank so we could function correctly without reading roots.
    cached_on_epoch: Epoch,
    cached_staked_nodes: Arc<HashMap<Pubkey, u64>>,
    cached_slots_in_epoch: u64,
    // Used to notify duplicate consensus state machine
    duplicate_slots_sender: Sender<Slot>,
    shred_version: u16,
}

impl DuplicateShredHandlerTrait for DuplicateShredHandler {
    // Here we are sending data one by one rather than in a batch because in the future
    // we may send different type of CrdsData to different senders.
    fn handle(&mut self, shred_data: DuplicateShred) {
        self.cache_root_info();
        self.maybe_prune_buffer();
        let slot = shred_data.slot;
        let pubkey = shred_data.from;
        if let Err(error) = self.handle_shred_data(shred_data) {
            if error.is_non_critical() {
                info!("Received invalid duplicate shred proof from {pubkey} for slot {slot}: {error:?}");
            } else {
                error!("Unable to process duplicate shred proof from {pubkey} for slot {slot}: {error:?}");
            }
        }
    }
}

impl DuplicateShredHandler {
    pub fn new(
        blockstore: Arc<Blockstore>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
        bank_forks: Arc<RwLock<BankForks>>,
        duplicate_slots_sender: Sender<Slot>,
        shred_version: u16,
    ) -> Self {
        Self {
            buffer: HashMap::<(Slot, Pubkey), BufferEntry>::default(),
            consumed: HashMap::<Slot, bool>::default(),
            last_root: 0,
            cached_on_epoch: 0,
            cached_staked_nodes: Arc::new(HashMap::new()),
            cached_slots_in_epoch: 0,
            blockstore,
            leader_schedule_cache,
            bank_forks,
            duplicate_slots_sender,
            shred_version,
        }
    }

    fn cache_root_info(&mut self) {
        let last_root = self.blockstore.max_root();
        if last_root == self.last_root && !self.cached_staked_nodes.is_empty() {
            return;
        }
        self.last_root = last_root;
        if let Ok(bank_fork) = self.bank_forks.try_read() {
            let root_bank = bank_fork.root_bank();
            let epoch_info = root_bank.get_epoch_info();
            if self.cached_staked_nodes.is_empty() || self.cached_on_epoch < epoch_info.epoch {
                self.cached_on_epoch = epoch_info.epoch;
                if let Some(cached_staked_nodes) = root_bank.epoch_staked_nodes(epoch_info.epoch) {
                    self.cached_staked_nodes = cached_staked_nodes;
                }
                self.cached_slots_in_epoch = epoch_info.slots_in_epoch;
            }
        }
    }

    fn handle_shred_data(&mut self, chunk: DuplicateShred) -> Result<(), Error> {
        if !self.should_consume_slot(chunk.slot) {
            return Ok(());
        }
        let slot = chunk.slot;
        let num_chunks = chunk.num_chunks();
        let chunk_index = chunk.chunk_index();
        if usize::from(num_chunks) > MAX_NUM_CHUNKS || chunk_index >= num_chunks {
            return Err(Error::InvalidChunkIndex {
                chunk_index,
                num_chunks,
            });
        }
        let entry = self.buffer.entry((chunk.slot, chunk.from)).or_default();
        *entry
            .get_mut(usize::from(chunk_index))
            .ok_or(Error::InvalidChunkIndex {
                chunk_index,
                num_chunks,
            })? = Some(chunk);
        // If all chunks are already received, reconstruct and store
        // the duplicate slot proof in blockstore
        if entry.iter().flatten().count() == usize::from(num_chunks) {
            let chunks = std::mem::take(entry).into_iter().flatten();
            let pubkey = self
                .leader_schedule_cache
                .slot_leader_at(slot, /*bank:*/ None)
                .ok_or(Error::UnknownSlotLeader(slot))?;
            let (shred1, shred2) =
                duplicate_shred::into_shreds(&pubkey, chunks, self.shred_version)?;
            if !self.blockstore.has_duplicate_shreds_in_slot(slot) {
                self.blockstore.store_duplicate_slot(
                    slot,
                    shred1.into_payload(),
                    shred2.into_payload(),
                )?;
                // Notify duplicate consensus state machine
                self.duplicate_slots_sender
                    .send(slot)
                    .map_err(|_| Error::DuplicateSlotSenderFailure)?;
            }
            self.consumed.insert(slot, true);
        }
        Ok(())
    }

    fn should_consume_slot(&mut self, slot: Slot) -> bool {
        slot > self.last_root
            && slot < self.last_root.saturating_add(self.cached_slots_in_epoch)
            && should_consume_slot(slot, &self.blockstore, &mut self.consumed)
    }

    fn maybe_prune_buffer(&mut self) {
        // The buffer is allowed to grow to twice the intended capacity, at
        // which point the extraneous entries are removed in linear time,
        // resulting an amortized O(1) performance.
        if self.buffer.len() < BUFFER_CAPACITY.saturating_mul(2) {
            return;
        }
        self.consumed.retain(|&slot, _| slot > self.last_root);
        // Filter out obsolete slots and limit number of entries per pubkey.
        {
            let mut counts = HashMap::<Pubkey, usize>::new();
            self.buffer.retain(|(slot, pubkey), _| {
                *slot > self.last_root
                    && should_consume_slot(*slot, &self.blockstore, &mut self.consumed)
                    && {
                        let count = counts.entry(*pubkey).or_default();
                        *count = count.saturating_add(1);
                        *count <= MAX_NUM_ENTRIES_PER_PUBKEY
                    }
            });
        }
        if self.buffer.len() < BUFFER_CAPACITY {
            return;
        }
        // Lookup stake for each entry.
        let mut buffer: Vec<_> = self
            .buffer
            .drain()
            .map(|entry @ ((_, pubkey), _)| {
                let stake = self
                    .cached_staked_nodes
                    .get(&pubkey)
                    .copied()
                    .unwrap_or_default();
                (stake, entry)
            })
            .collect();
        // Drop entries with lowest stake and rebuffer remaining ones.
        buffer.select_nth_unstable_by_key(BUFFER_CAPACITY, |&(stake, _)| Reverse(stake));
        self.buffer.extend(
            buffer
                .into_iter()
                .take(BUFFER_CAPACITY)
                .map(|(_, entry)| entry),
        );
    }
}

// Returns false if a duplicate proof is already ingested for the slot,
// and updates local `consumed` cache with blockstore.
fn should_consume_slot(
    slot: Slot,
    blockstore: &Blockstore,
    consumed: &mut HashMap<Slot, bool>,
) -> bool {
    !*consumed
        .entry(slot)
        .or_insert_with(|| blockstore.has_duplicate_shreds_in_slot(slot))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            cluster_info::DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
            duplicate_shred::{from_shred, tests::new_rand_shred},
        },
        crossbeam_channel::unbounded,
        itertools::Itertools,
        solana_ledger::{
            genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
            get_tmp_ledger_path_auto_delete,
            shred::Shredder,
        },
        solana_runtime::{accounts_background_service::AbsRequestSender, bank::Bank},
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
    };

    fn create_duplicate_proof(
        keypair: Arc<Keypair>,
        sender_pubkey: Option<Pubkey>,
        slot: u64,
        expected_error: Option<Error>,
        chunk_size: usize,
        shred_version: u16,
    ) -> Result<impl Iterator<Item = DuplicateShred>, Error> {
        let my_keypair = match expected_error {
            Some(Error::InvalidSignature) => Arc::new(Keypair::new()),
            _ => keypair,
        };
        let mut rng = rand::thread_rng();
        let shredder = Shredder::new(slot, slot - 1, 0, shred_version).unwrap();
        let next_shred_index = 353;
        let shred1 = new_rand_shred(&mut rng, next_shred_index, &shredder, &my_keypair);
        let shredder1 = Shredder::new(slot + 1, slot, 0, shred_version).unwrap();
        let shred2 = match expected_error {
            Some(Error::SlotMismatch) => {
                new_rand_shred(&mut rng, next_shred_index, &shredder1, &my_keypair)
            }
            Some(Error::InvalidDuplicateShreds) => shred1.clone(),
            _ => new_rand_shred(&mut rng, next_shred_index, &shredder, &my_keypair),
        };
        let sender = match sender_pubkey {
            Some(pubkey) => pubkey,
            None => my_keypair.pubkey(),
        };
        let chunks = from_shred(
            shred1,
            sender,
            shred2.payload().clone(),
            None::<fn(Slot) -> Option<Pubkey>>,
            timestamp(), // wallclock
            chunk_size,  // max_size
            shred_version,
        )?;
        Ok(chunks)
    }

    #[test]
    fn test_handle_mixed_entries() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let my_keypair = Arc::new(Keypair::new());
        let my_pubkey = my_keypair.pubkey();
        let shred_version = 0;
        let genesis_config_info = create_genesis_config_with_leader(10_000, &my_pubkey, 10_000);
        let GenesisConfigInfo { genesis_config, .. } = genesis_config_info;
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks_arc = BankForks::new_rw_arc(bank);
        {
            let mut bank_forks = bank_forks_arc.write().unwrap();
            let bank0 = bank_forks.get(0).unwrap();
            bank_forks.insert(Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 9));
            bank_forks
                .set_root(9, &AbsRequestSender::default(), None)
                .unwrap();
        }
        blockstore.set_roots([0, 9].iter()).unwrap();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(
            &bank_forks_arc.read().unwrap().working_bank(),
        ));
        let (sender, receiver) = unbounded();
        let start_slot: Slot = 10;

        let mut duplicate_shred_handler = DuplicateShredHandler::new(
            blockstore.clone(),
            leader_schedule_cache,
            bank_forks_arc,
            sender,
            shred_version,
        );
        let chunks = create_duplicate_proof(
            my_keypair.clone(),
            None,
            start_slot,
            None,
            DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
            shred_version,
        )
        .unwrap();
        let chunks1 = create_duplicate_proof(
            my_keypair.clone(),
            None,
            start_slot + 1,
            None,
            DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
            shred_version,
        )
        .unwrap();
        assert!(!blockstore.has_duplicate_shreds_in_slot(start_slot));
        assert!(!blockstore.has_duplicate_shreds_in_slot(start_slot + 1));
        // Test that two proofs are mixed together, but we can store the proofs fine.
        for (chunk1, chunk2) in chunks.zip(chunks1) {
            duplicate_shred_handler.handle(chunk1);
            duplicate_shred_handler.handle(chunk2);
        }
        assert!(blockstore.has_duplicate_shreds_in_slot(start_slot));
        assert!(blockstore.has_duplicate_shreds_in_slot(start_slot + 1));
        assert_eq!(
            receiver.try_iter().collect_vec(),
            vec![start_slot, start_slot + 1]
        );

        // Test all kinds of bad proofs.
        for error in [
            Error::InvalidSignature,
            Error::SlotMismatch,
            Error::InvalidDuplicateShreds,
        ] {
            match create_duplicate_proof(
                my_keypair.clone(),
                None,
                start_slot + 2,
                Some(error),
                DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
                shred_version,
            ) {
                Err(_) => (),
                Ok(chunks) => {
                    for chunk in chunks {
                        duplicate_shred_handler.handle(chunk);
                    }
                    assert!(!blockstore.has_duplicate_shreds_in_slot(start_slot + 2));
                    assert!(receiver.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_reject_abuses() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let my_keypair = Arc::new(Keypair::new());
        let my_pubkey = my_keypair.pubkey();
        let shred_version = 0;
        let genesis_config_info = create_genesis_config_with_leader(10_000, &my_pubkey, 10_000);
        let GenesisConfigInfo { genesis_config, .. } = genesis_config_info;
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks_arc = BankForks::new_rw_arc(bank);
        {
            let mut bank_forks = bank_forks_arc.write().unwrap();
            let bank0 = bank_forks.get(0).unwrap();
            bank_forks.insert(Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 9));
            bank_forks
                .set_root(9, &AbsRequestSender::default(), None)
                .unwrap();
        }
        blockstore.set_roots([0, 9].iter()).unwrap();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(
            &bank_forks_arc.read().unwrap().working_bank(),
        ));
        let (sender, receiver) = unbounded();
        let mut duplicate_shred_handler = DuplicateShredHandler::new(
            blockstore.clone(),
            leader_schedule_cache,
            bank_forks_arc,
            sender,
            shred_version,
        );
        let start_slot: Slot = 10;

        // This proof will not be accepted because num_chunks is too large.
        let chunks = create_duplicate_proof(
            my_keypair.clone(),
            None,
            start_slot,
            None,
            DUPLICATE_SHRED_MAX_PAYLOAD_SIZE / 2,
            shred_version,
        )
        .unwrap();
        for chunk in chunks {
            duplicate_shred_handler.handle(chunk);
        }
        assert!(!blockstore.has_duplicate_shreds_in_slot(start_slot));
        assert!(receiver.is_empty());

        // This proof will be rejected because the slot is too far away in the future.
        let future_slot =
            blockstore.max_root() + duplicate_shred_handler.cached_slots_in_epoch + start_slot;
        let chunks = create_duplicate_proof(
            my_keypair.clone(),
            None,
            future_slot,
            None,
            DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
            shred_version,
        )
        .unwrap();
        for chunk in chunks {
            duplicate_shred_handler.handle(chunk);
        }
        assert!(!blockstore.has_duplicate_shreds_in_slot(future_slot));
        assert!(receiver.is_empty());

        // Send in two proofs, the first proof showing up will be accepted, the following
        // proofs will be discarded.
        let mut chunks = create_duplicate_proof(
            my_keypair,
            None,
            start_slot,
            None,
            DUPLICATE_SHRED_MAX_PAYLOAD_SIZE,
            shred_version,
        )
        .unwrap();
        // handle chunk 0 of the first proof.
        duplicate_shred_handler.handle(chunks.next().unwrap());
        assert!(!blockstore.has_duplicate_shreds_in_slot(start_slot));
        assert!(receiver.is_empty());
        // Now send in the rest of the first proof, it will succeed.
        for chunk in chunks {
            duplicate_shred_handler.handle(chunk);
        }
        assert!(blockstore.has_duplicate_shreds_in_slot(start_slot));
        assert_eq!(receiver.try_iter().collect_vec(), vec![start_slot]);
    }
}
