use solana_sdk::{clock::Slot, signature::Signature, timing::timestamp};

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

pub const SLOT_CACHE_LIFE_MS: u64 = 10_000;

#[derive(Default)]
struct ReceivedSignatureCache {
    purged_slots: BTreeSet<Slot>,
    last_purged: u64,
    signature_cache: HashSet<Arc<Signature>>,
    active_slots: BTreeSet<(u64, Slot)>,
    slot_to_signatures: HashMap<Slot, Vec<Arc<Signature>>>,
}

impl ReceivedSignatureCache {
    pub fn insert(&mut self, slot: Slot, signatures: Vec<Signature>) {
        self.insert_internal(slot, signatures, None)
    }

    pub fn contains(&mut self, signature: &Signature) -> bool {
        self.signature_cache.contains(signature)
    }

    pub fn set_root(&mut self, root: Slot) {
        // Purge everything < root from self.purged_slots
        let mut to_keep = self.purged_slots.split_off(&root);
        std::mem::swap(&mut to_keep, &mut self.purged_slots);
    }

    fn insert_internal(&mut self, slot: Slot, signatures: Vec<Signature>, now: Option<u64>) {
        if self.purged_slots.contains(&slot) {
            return;
        }

        if !self.slot_to_signatures.contains_key(&slot) {
            self.active_slots
                .insert((now.unwrap_or_else(timestamp) + SLOT_CACHE_LIFE_MS, slot));
        }

        for signature in signatures {
            let signature = if let Some(signature) = self.signature_cache.get(&signature) {
                signature.clone()
            } else {
                let signature = Arc::new(signature);
                self.signature_cache.insert(signature.clone());
                signature
            };

            self.slot_to_signatures
                .entry(slot)
                .or_default()
                .push(signature);
        }

        // Check if we need to purge any slots from the cache
        if now.unwrap_or_else(timestamp) - self.last_purged > SLOT_CACHE_LIFE_MS {
            let mut to_purge = self
                .active_slots
                .split_off(&(self.last_purged + SLOT_CACHE_LIFE_MS + 1, 0));
            std::mem::swap(&mut to_purge, &mut self.active_slots);

            for (_, purge_slot) in to_purge {
                self.purge_slot(purge_slot);
            }
            self.last_purged = now.unwrap_or_else(timestamp);
        }
    }

    fn purge_slot(&mut self, purge_slot: Slot) {
        self.purged_slots.insert(purge_slot);
        let slot_signatures = self
            .slot_to_signatures
            .remove(&purge_slot)
            .expect("Slot to purge must exist in `slot_to_signatures`");
        assert!(!slot_signatures.is_empty());

        for signature in slot_signatures {
            // If this is the last reference to this signature, remove it from the cache
            if Arc::strong_count(&signature) == 2 {
                self.signature_cache.remove(&signature);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_received_signature_cache_insert() {
        let mut cache = ReceivedSignatureCache::default();
        cache.insert(0, vec![Signature::new(&[0u8; 64])]);
        assert!(cache.contains(&Signature::new(&[0u8; 64])));
        cache.insert(1, vec![Signature::new(&[1u8; 64])]);
        assert!(cache.contains(&Signature::new(&[0u8; 64])));

        assert!(!cache.contains(&Signature::new(&[2u8; 64])));
    }

    #[test]
    fn test_received_signature_cache_purge() {
        let mut cache = ReceivedSignatureCache::default();

        // Insert slot 0 signatures
        let slot0_insert_time = 0;
        let slot0_purge_time = slot0_insert_time + SLOT_CACHE_LIFE_MS + 1;
        cache.insert_internal(0, vec![Signature::new(&[0u8; 64])], Some(slot0_insert_time));
        cache.insert_internal(0, vec![Signature::new(&[1u8; 64])], Some(slot0_insert_time));

        // Insert slot 1 signatures. Make slot 1 have a later timestamp than slot 0
        let slot1_insert_time = 1;
        let slot1_purge_time = slot1_insert_time + SLOT_CACHE_LIFE_MS + 1;
        cache.insert_internal(1, vec![Signature::new(&[1u8; 64])], Some(slot1_insert_time));
        assert!(slot1_insert_time < slot0_purge_time);

        // Insert with last possible timestamp before purging slot 0,
        // nothing should be purged
        cache.insert_internal(
            1,
            vec![Signature::new(&[2u8; 64])],
            Some(slot0_purge_time - 1),
        );
        for i in 0..=2 {
            assert!(cache.contains(&Signature::new(&[i; 64])));
        }

        // Insert with first expire timestamp for purge, signature 0 should be purged,
        // signature 1 should not be purged because slot 1 still references it
        cache.insert_internal(1, vec![Signature::new(&[3u8; 64])], Some(slot0_purge_time));
        assert!(!cache.contains(&Signature::new(&[0; 64])));
        for i in 1..=3 {
            assert!(cache.contains(&Signature::new(&[i; 64])));
        }

        // Trigger another purge, this time of slot 1
        let purge_time = slot0_purge_time + SLOT_CACHE_LIFE_MS + 1;
        assert!(purge_time > slot1_purge_time);
        cache.insert_internal(1, vec![Signature::new(&[4u8; 64])], Some(purge_time));
        assert!(cache.signature_cache.is_empty());
        assert!(cache.active_slots.is_empty());
        assert!(cache.slot_to_signatures.is_empty());
        assert_eq!(cache.purged_slots, vec![0, 1].into_iter().collect());
        for i in 0..=4 {
            assert!(!cache.contains(&Signature::new(&[i; 64])));
        }

        // Trying to insert a signature for a purged slot should fail
        cache.insert_internal(1, vec![Signature::new(&[5u8; 64])], Some(purge_time + 1));
        assert!(!cache.contains(&Signature::new(&[5u8; 64])));

        // Setting a root should clear the purged slots < root
        assert_eq!(cache.purged_slots, vec![0, 1].into_iter().collect());
        cache.set_root(1);
        assert_eq!(cache.purged_slots, vec![1].into_iter().collect());
        cache.set_root(2);
        assert!(cache.purged_slots.is_empty());

        // Now inserting for slot should work again
        cache.insert_internal(1, vec![Signature::new(&[5u8; 64])], Some(purge_time + 1));
        assert!(cache.contains(&Signature::new(&[5u8; 64])));
    }
}
