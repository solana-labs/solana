use super::*;
use solana_ledger::shred::Shred;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::VecDeque;

pub type TransmitShreds = (Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>);

#[derive(Default)]
pub struct SlotCachedTransmitShreds {
    pub stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    pub data_shred_batches: Vec<Arc<Vec<Shred>>>,
    pub coding_shred_batches: Vec<Arc<Vec<Shred>>>,
}

impl SlotCachedTransmitShreds {
    pub fn contains_last_shreds(&self) -> bool {
        self.data_shred_batches
            .last()
            .and_then(|last_shred_batch| {
                last_shred_batch
                    .last()
                    .and_then(|shred| Some(shred.last_in_slot()))
            })
            .unwrap_or(false)
            && self
                .coding_shred_batches
                .last()
                .and_then(|last_shred_batch| {
                    last_shred_batch
                        .last()
                        .and_then(|shred| Some(shred.is_last_coding_in_set()))
                })
                .unwrap_or(false)
    }

    pub fn to_transmit_shreds(&self) -> Vec<TransmitShreds> {
        self.data_shred_batches
            .iter()
            .map(|data_shred_batch| (self.stakes.clone(), data_shred_batch.clone()))
            .chain(
                self.coding_shred_batches
                    .iter()
                    .map(|code_shred_batch| (self.stakes.clone(), code_shred_batch.clone())),
            )
            .collect()
    }
}

pub struct SlotTransmitShredsCache {
    cache: HashMap<Slot, SlotCachedTransmitShreds>,
    insertion_order: VecDeque<Slot>,
}

impl SlotTransmitShredsCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::new(),
            insertion_order: VecDeque::with_capacity(capacity),
        }
    }

    pub fn remove_slot(&mut self, slot: Slot) -> Option<SlotCachedTransmitShreds> {
        self.insertion_order.retain(|x| *x != slot);
        self.cache.remove(&slot)
    }

    pub fn contains_slot(&self, slot: Slot) -> bool {
        self.cache.contains_key(&slot)
    }

    pub fn push(&mut self, slot: Slot, transmit_shreds: TransmitShreds) {
        if transmit_shreds.1.is_empty() {
            return;
        }
        if !self.cache.contains_key(&slot) {
            if transmit_shreds.1[0].index() != 0 {
                // Shreds for a slot must come in order from broadcast.
                // If it's not the first shred for the slot, and the cache
                // doesn't contain this slot's earlier shreds, this means this
                // slot has already been purged from the cache, so dump it.
                return;
            }
            if self.insertion_order.len() == self.insertion_order.capacity() {
                let old_slot = self.insertion_order.pop_front().unwrap();
                self.cache.remove(&old_slot).unwrap();
            }
            self.insertion_order.push_back(slot);
            let new_slot_cache = SlotCachedTransmitShreds {
                stakes: transmit_shreds.0,
                data_shred_batches: vec![],
                coding_shred_batches: vec![],
            };
            self.cache.insert(slot, new_slot_cache);
        }

        let slot_cache = self.cache.get_mut(&slot).unwrap();

        // Transmit shreds must be all of one type or another
        if transmit_shreds.1[0].is_data() {
            assert!(transmit_shreds.1.iter().all(|s| s.is_data()));
            slot_cache.data_shred_batches.push(transmit_shreds.1);
        } else {
            assert!(transmit_shreds.1.iter().all(|s| !s.is_data()));
            slot_cache.coding_shred_batches.push(transmit_shreds.1);
        }
    }

    pub fn get(&mut self, slot: Slot) -> Option<&SlotCachedTransmitShreds> {
        self.cache.get(&slot)
    }

    pub fn get_or_update(
        &mut self,
        bank: &Bank,
        blockstore: &Blockstore,
    ) -> &SlotCachedTransmitShreds {
        if self.cache.get(&bank.slot()).is_none() {
            let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
            let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
            let stakes = stakes.map(Arc::new);

            let (data_shreds, coding_shreds) =
                self.get_new_shreds_since(blockstore, bank.slot(), 0, 0);
            self.push(bank.slot(), (stakes.clone(), data_shreds));
            self.push(bank.slot(), (stakes.clone(), coding_shreds));
            self.cache
                .get(&bank.slot())
                .expect("Just inserted this entry, must exist")
        } else {
            self.cache.get(&bank.slot()).unwrap()
        }
    }

    // Gets any missing shreds for entries in the cache
    pub fn update_cache_from_blockstore(
        &mut self,
        blockstore: &Blockstore,
    ) -> Vec<(Slot, SlotCachedTransmitShreds)> {
        let updates: Vec<_> = self
            .cache
            .iter()
            .filter_map(|(slot, cached_shreds)| {
                if !cached_shreds.contains_last_shreds() {
                    let last_data_shred_index = cached_shreds
                        .data_shred_batches
                        .last()
                        .and_then(|last_shred_batch| {
                            last_shred_batch
                                .last()
                                .and_then(|shred| Some(shred.index()))
                        })
                        .expect("Cache entry cannot be empty (guaranteed by push())");

                    let last_coding_shred_index = cached_shreds
                        .coding_shred_batches
                        .last()
                        .and_then(|last_shred_batch| {
                            last_shred_batch
                                .last()
                                .and_then(|shred| Some(shred.index()))
                        })
                        .expect("Cache entry cannot be empty (guaranteed by push())");

                    let (new_data_shreds, new_coding_shreds) =
                        self.get_new_shreds_since(blockstore, *slot, 0, 0);

                    Some((
                        *slot,
                        cached_shreds.stakes.clone(),
                        new_data_shreds,
                        new_coding_shreds,
                    ))
                } else {
                    None
                }
            })
            .collect();

        updates
            .into_iter()
            .map(|(slot, stakes, new_data_shreds, new_coding_shreds)| {
                let data_transmit_shreds = (stakes.clone(), new_data_shreds.clone());
                let coding_transmit_shreds = (stakes.clone(), new_coding_shreds.clone());

                // Add the data shreds to the cache
                self.push(slot, data_transmit_shreds);

                // Add the coding shreds to the cache
                self.push(slot, coding_transmit_shreds);

                (
                    slot,
                    SlotCachedTransmitShreds {
                        stakes,
                        data_shred_batches: vec![new_data_shreds],
                        coding_shred_batches: vec![new_coding_shreds],
                    },
                )
            })
            .collect()
    }

    fn get_new_shreds_since(
        &self,
        blockstore: &Blockstore,
        slot: Slot,
        data_start_index: u64,
        coding_start_index: u64,
    ) -> (Arc<Vec<Shred>>, Arc<Vec<Shred>>) {
        let new_data_shreds = Arc::new(
            blockstore
                .get_data_shreds_since(slot, 0)
                .expect("My own shreds must be reconstructable"),
        );

        let new_coding_shreds = Arc::new(
            blockstore
                .get_coding_shreds_since(slot, 0)
                .expect("My own shreds must be reconstructable"),
        );

        (new_data_shreds, new_coding_shreds)
    }

    // Update with latest leader blocks. Note it should generally be safe to purge
    // old leader blocks, because the validator should only generate new leadder
    // blocks if the old blocks were confirmed to be propagated, which means the old
    // blocks will not need to be retransmitted, so they can be removed from the
    // cache
    pub fn update_retransmit_cache(
        &mut self,
        retransmit_cache_receiver: &RetransmitCacheReceiver,
        updated_slots: &mut HashSet<Slot>,
    ) -> Result<()> {
        let timer = Duration::from_millis(100);
        let (slot, transmit_shreds) = retransmit_cache_receiver.recv_timeout(timer)?;
        updated_slots.insert(slot);
        // Update the cache with shreds from latest leader slot
        self.push(slot, transmit_shreds);
        while let Ok((slot, transmit_shreds)) = retransmit_cache_receiver.try_recv() {
            updated_slots.insert(slot);
            self.push(slot, transmit_shreds);
        }

        Ok(())
    }
}
