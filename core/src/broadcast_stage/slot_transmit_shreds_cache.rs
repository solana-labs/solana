use solana_ledger::shred::Shred;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub type TransmitShreds = (Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>);

#[derive(Default)]
pub struct SlotCachedTransmitShreds {
    pub stakes: Arc<HashMap<Pubkey, u64>>,
    pub shreds: Vec<Arc<Vec<Shred>>>,
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
        if !self.cache.contains_key(&slot) {
            let old_slot = self.insertion_order.pop_front();
            self.cache.remove(&slot).unwrap();
            self.insertion_order.push_back(slot);
            let new_slot_cache = SlotCachedTransmitShreds {
                stakes: transmit_shreds
                    .0
                    .expect("First batch of TransmitShreds for a slot must contain stakes"),
                shreds: vec![],
            };
            self.cache.insert(slot, new_slot_cache);
        }

        let slot_cache = self.cache.get_mut(&slot).unwrap();
        slot_cache.shreds.push(transmit_shreds.1);
    }

    pub fn get(&self, slot: Slot) -> Option<&SlotCachedTransmitShreds> {
        self.cache.get(&slot)
    }
}
