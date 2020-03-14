use super::*;
use solana_ledger::shred::Shred;
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::VecDeque;

pub type TransmitShreds = (Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>);

#[derive(Default)]
pub struct SlotCachedTransmitShreds {
    pub stakes: Arc<HashMap<Pubkey, u64>>,
    pub data_shreds: Vec<Arc<Vec<Shred>>>,
    pub coding_shreds: Vec<Arc<Vec<Shred>>>,
}

impl SlotCachedTransmitShreds {
    pub fn contains_all_shreds(&self) -> bool {
        self.data_shreds
            .last()
            .and_then(|last_shred_vec| {
                last_shred_vec
                    .last()
                    .and_then(|shred| Some(shred.last_in_slot()))
            })
            .unwrap_or(false)
            && self
                .coding_shreds
                .last()
                .and_then(|last_shred_vec| {
                    last_shred_vec
                        .last()
                        .and_then(|shred| Some(shred.is_last_coding_in_set()))
                })
                .unwrap_or(false)
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

    pub fn push(&mut self, slot: Slot, transmit_shreds: TransmitShreds) {
        if transmit_shreds.1.is_empty() {
            return;
        }
        if !self.cache.contains_key(&slot) {
            if transmit_shreds.1[0].index() != 0 {
                // Shreds for a slot must come in order from broadcast.
                // If it's not the first shred for the slot, and the cache
                // doesn't contain this slot's earleir shreds, this means this
                // slot has already been purged from the cache, so dump it.
                return;
            }
            let old_slot = self.insertion_order.pop_front();
            self.cache.remove(&slot).unwrap();
            self.insertion_order.push_back(slot);
            let new_slot_cache = SlotCachedTransmitShreds {
                stakes: transmit_shreds
                    .0
                    .expect("TransmitShreds for a slot must contain stakes"),
                data_shreds: vec![],
                coding_shreds: vec![],
            };
            self.cache.insert(slot, new_slot_cache);
        }

        let slot_cache = self.cache.get_mut(&slot).unwrap();

        // Transmit shreds must be all of one type or another
        if transmit_shreds.1[0].is_data() {
            assert!(transmit_shreds.1.iter().all(|s| s.is_data()));
            slot_cache.data_shreds.push(transmit_shreds.1);
        } else {
            assert!(transmit_shreds.1.iter().all(|s| !s.is_data()));
            slot_cache.coding_shreds.push(transmit_shreds.1);
        }
    }

    pub fn get(&mut self, bank: &Bank, blockstore: &Blockstore) -> &SlotCachedTransmitShreds {
        if self.cache.get(&bank.slot()).is_none() {
            let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
            let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
            let stakes = stakes.map(Arc::new);
            let mut current_index = 0;
            let mut data_shreds = vec![];
            loop {
                if let Some(data) = blockstore
                    .get_data_shred(bank.slot(), current_index)
                    .expect("blockstore couldn't fetch data")
                {
                    let data_shred = Shred::new_from_serialized_shred(data)
                        .expect("My own shreds must be reconstructable");
                    let is_last = data_shred.last_in_slot();
                    data_shreds.push(data_shred);
                    if is_last {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Add the data shreds to the cache
            self.push(bank.slot(), (stakes.clone(), Arc::new(data_shreds)));

            current_index = 0;
            let mut coding_shreds = vec![];
            loop {
                if let Some(code) = blockstore
                    .get_coding_shred(bank.slot(), current_index)
                    .expect("blockstore couldn't fetch coding")
                {
                    let code_shred = Shred::new_from_serialized_shred(code)
                        .expect("My own shreds must be reconstructable");
                    coding_shreds.push(code_shred);
                } else {
                    break;
                }
            }

            // Add the coding shreds to the cache
            self.push(bank.slot(), (stakes.clone(), Arc::new(coding_shreds)));
            self.cache
                .get(&bank.slot())
                .expect("Just inserted this entry, must exist")
        } else {
            self.cache.get(&bank.slot()).unwrap()
        }
    }
}
