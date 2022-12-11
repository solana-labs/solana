use {
    crate::{
        cluster_info_entry_listener::ClusterInfoEntryHandler,
        crds_value::CrdsData,
        duplicate_shred::{DuplicateShred, Error},
    },
    itertools::Itertools,
    log::*,
    solana_ledger::{
        blockstore::Blockstore, blockstore_meta::DuplicateSlotProof,
        leader_schedule_cache::LeaderScheduleCache, shred::Shred,
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{collections::HashMap, sync::Arc},
};

struct ProofChunkMap {
    num_chunks: u8,
    missing_chunks: u8,
    chunks: HashMap<u8, Vec<u8>>,
}

#[derive(Eq, PartialEq, Hash, Clone)]
struct DuplicateSlotProofKey {
    from: Pubkey,
    wallclock: u64,
}

// Group received chunks by peer pubkey, when we receive an invalid proof,
// set the value to None so we don't accept future proofs with the same key.
type SlotChunkMap = HashMap<DuplicateSlotProofKey, Option<ProofChunkMap>>;

pub struct DuplicateShredHandler {
    // When a valid proof has been inserted, we change the entry for that slot to None
    // to indicate we no longer accept proofs for this slot.
    chunk_map: HashMap<Slot, Option<SlotChunkMap>>,
    // remember the last root slot handled, clear anything older than last_root.
    last_root: Slot,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
}

impl ClusterInfoEntryHandler for DuplicateShredHandler {
    // Here we are sending data one by one rather than in a batch because in the future
    // we may send different type of CrdsData to different senders.
    fn handle(&mut self, data: CrdsData) {
        if let CrdsData::DuplicateShred(_, shred_data) = data {
            if let Err(error) = self.handle_shred_data(shred_data) {
                error!("handle packet: {:?}", error)
            }
            self.cleanup_old_slots();
        }
    }
}

impl DuplicateShredHandler {
    pub fn new(
        blockstore: Arc<Blockstore>,
        leader_schedule_cache: Arc<LeaderScheduleCache>,
    ) -> Self {
        Self {
            chunk_map: HashMap::new(),
            last_root: 0,
            blockstore,
            leader_schedule_cache,
        }
    }

    fn handle_shred_data(&mut self, data: DuplicateShred) -> Result<(), Error> {
        if self.should_insert_chunk(&data) {
            match self.insert_chunk(data) {
                Err(error) => return Err(error),
                Ok(Some((slot, proof))) => {
                    self.verify_and_apply_proof(slot, proof)?;
                    // We stored the duplicate proof in this slot, no need to accept any future proof.
                    self.clear_slot(slot);
                }
                _ => (),
            }
        }
        Ok(())
    }

    fn should_insert_chunk(&self, data: &DuplicateShred) -> bool {
        let slot = data.slot;
        if slot <= self.blockstore.last_root() || self.blockstore.has_duplicate_shreds_in_slot(slot)
        {
            return false;
        }
        !matches!(self.chunk_map.get(&slot), Some(None))
    }

    fn new_proof_chunk_map(num_chunks: u8) -> ProofChunkMap {
        ProofChunkMap {
            num_chunks,
            missing_chunks: num_chunks,
            chunks: HashMap::new(),
        }
    }

    fn clear_slot(&mut self, slot: u64) {
        self.chunk_map.insert(slot, None);
    }

    fn insert_chunk(
        &mut self,
        data: DuplicateShred,
    ) -> Result<Option<(Slot, DuplicateSlotProof)>, Error> {
        if let Some(slot_chunk_map) = self
            .chunk_map
            .entry(data.slot)
            .or_insert_with(|| Some(HashMap::new()))
        {
            let proof_key = DuplicateSlotProofKey {
                from: data.from,
                wallclock: data.wallclock,
            };
            if let Some(proof_chunk_map) = slot_chunk_map
                .entry(proof_key)
                .or_insert_with(|| Some(Self::new_proof_chunk_map(data.num_chunks)))
            {
                let num_chunks = data.num_chunks;
                let chunk_index = data.chunk_index;
                if num_chunks == proof_chunk_map.num_chunks
                    && chunk_index < num_chunks
                    && !proof_chunk_map.chunks.contains_key(&chunk_index)
                {
                    proof_chunk_map.missing_chunks =
                        proof_chunk_map.missing_chunks.saturating_sub(1);
                    proof_chunk_map.chunks.insert(chunk_index, data.chunk);
                    if proof_chunk_map.missing_chunks == 0 {
                        let proof_data = (0..num_chunks)
                            .map(|k| proof_chunk_map.chunks.remove(&k).unwrap())
                            .concat();
                        let proof: DuplicateSlotProof = bincode::deserialize(&proof_data)?;
                        return Ok(Some((data.slot, proof)));
                    }
                }
            }
        }
        Ok(None)
    }

    fn verify_and_apply_proof(&self, slot: Slot, proof: DuplicateSlotProof) -> Result<(), Error> {
        if slot <= self.blockstore.last_root() || self.blockstore.has_duplicate_shreds_in_slot(slot)
        {
            return Ok(());
        }
        match self.leader_schedule_cache.slot_leader_at(slot, None) {
            Some(slot_leader) => {
                let shred1 = Shred::new_from_serialized_shred(proof.shred1.clone())?;
                let shred2 = Shred::new_from_serialized_shred(proof.shred2.clone())?;
                if shred1.slot() != slot || shred2.slot() != slot {
                    Err(Error::SlotMismatch)
                } else if shred1.index() != shred2.index() {
                    Err(Error::ShredIndexMismatch)
                } else if shred1.shred_type() != shred2.shred_type() {
                    Err(Error::ShredTypeMismatch)
                } else if shred1.payload() == shred2.payload() {
                    Err(Error::InvalidDuplicateShreds)
                } else if !shred1.verify(&slot_leader) || !shred2.verify(&slot_leader) {
                    Err(Error::InvalidSignature)
                } else {
                    self.blockstore
                        .store_duplicate_slot(slot, proof.shred1, proof.shred2)?;
                    Ok(())
                }
            }
            _ => Err(Error::UnknownSlotLeader),
        }
    }

    fn cleanup_old_slots(&mut self) {
        let new_last_root = self.blockstore.last_root();
        if self.last_root < new_last_root {
            self.chunk_map.retain(|k, _| k > &new_last_root);
            self.last_root = new_last_root
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            cluster_info::{ClusterInfo, Node},
            cluster_info_entry_listener::ClusterInfoEntryHandler,
            duplicate_shred::{from_shred, tests::new_rand_shred, DuplicateShred, Error},
        },
        rand::Rng,
        solana_ledger::shred::Shredder,
        solana_ledger::{
            genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
            get_tmp_ledger_path_auto_delete, leader_schedule_cache,
        },
        solana_runtime::{bank::Bank, bank_forks::BankForks},
        solana_sdk::signature::{Keypair, Signer},
        solana_streamer::socket::SocketAddrSpace,
        std::sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        },
    };

    #[test]
    fn test_handle_mixed_entries() {
        solana_logger::setup();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let my_keypair = Arc::new(Keypair::new());
        let my_pubkey = my_keypair.pubkey();
        let genesis_config_info = create_genesis_config_with_leader(10_000, &my_pubkey, 10_000);
        let GenesisConfigInfo { genesis_config, .. } = genesis_config_info;
        let bank_forks = BankForks::new(Bank::new_for_tests(&genesis_config));
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(
            &bank_forks.working_bank(),
        ));
        let mut duplicate_shred_handler =
            DuplicateShredHandler::new(blockstore.clone(), leader_schedule_cache.clone());
        let mut rng = rand::thread_rng();
        let (slot, parent_slot, reference_tick, version) = (1, 0, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = 353;
        let shred1 = new_rand_shred(&mut rng, next_shred_index, &shredder, &my_keypair);
        let shred2 = new_rand_shred(&mut rng, next_shred_index, &shredder, &my_keypair);
        let chunks = from_shred(
            shred1.clone(),
            my_pubkey,
            shred2.payload().clone(),
            None::<fn(Slot) -> Option<Pubkey>>,
            rng.gen(), // wallclock
            512,       // max_size
        )
        .unwrap();
        let (slot1, parent_slot1, reference_tick, version) = (2, 1, 0, 0);
        let shredder1 = Shredder::new(slot1, parent_slot1, reference_tick, version).unwrap();
        let shred3 = new_rand_shred(&mut rng, next_shred_index, &shredder1, &my_keypair);
        let shred4 = new_rand_shred(&mut rng, next_shred_index, &shredder1, &my_keypair);
        let chunks1 = from_shred(
            shred3.clone(),
            my_pubkey,
            shred4.payload().clone(),
            None::<fn(Slot) -> Option<Pubkey>>,
            rng.gen(), // wallclock
            512,       // max_size
        )
        .unwrap();
        assert!(!blockstore.has_duplicate_shreds_in_slot(slot));
        assert!(!blockstore.has_duplicate_shreds_in_slot(slot1));
        let mut index = 0;
        for (chunk1, chunk2) in chunks.zip(chunks1) {
            duplicate_shred_handler.handle(CrdsData::DuplicateShred(index, chunk1));
            duplicate_shred_handler.handle(CrdsData::DuplicateShred(index + 1, chunk2));
            index += 2
        }
        assert!(blockstore.has_duplicate_shreds_in_slot(slot));
        assert!(blockstore.has_duplicate_shreds_in_slot(slot1));
    }
}
