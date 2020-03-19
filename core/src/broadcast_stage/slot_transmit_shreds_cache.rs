use super::*;
use solana_ledger::shred::Shred;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::VecDeque;

pub type TransmitShreds = (Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>);

#[derive(Default, Debug, PartialEq)]
pub struct CachedBroadcastShreds {
    pub slot: Slot,
    pub stakes: Option<Arc<HashMap<Pubkey, u64>>>,
    pub data_shred_batches: Vec<Arc<Vec<Shred>>>,
    pub coding_shred_batches: Vec<Arc<Vec<Shred>>>,
}

impl CachedBroadcastShreds {
    pub fn contains_last_shreds(&self) -> bool {
        self.last_data_shred()
            .map(|shred| shred.last_in_slot())
            .unwrap_or(false)
            && self
                .last_coding_shred()
                .map(|shred| shred.is_last_coding_in_set())
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

    pub fn last_data_shred(&self) -> Option<&Shred> {
        self.data_shred_batches
            .last()
            .and_then(|last_shred_batch| last_shred_batch.last())
    }

    pub fn last_coding_shred(&self) -> Option<&Shred> {
        self.coding_shred_batches
            .last()
            .and_then(|last_shred_batch| last_shred_batch.last())
    }

    // Updates with new shreds from blockstore and returns a `CachedBroadcastShreds`
    // representing just the diff from the updates
    pub fn update_from_blockstore(&mut self, blockstore: &Blockstore) -> Option<Self> {
        if !self.contains_last_shreds() {
            let last_data_shred_index = self
                .last_data_shred()
                .map(|shred| shred.index() + 1)
                .unwrap_or(0);

            let last_coding_shred_index = self
                .last_coding_shred()
                .map(|shred| shred.index() + 1)
                .unwrap_or(0);

            let (new_data_shreds, new_coding_shreds) = Self::get_new_shreds(
                blockstore,
                self.slot,
                last_data_shred_index as u64,
                last_coding_shred_index as u64,
            );

            let data_shred_batches = {
                if !new_data_shreds.is_empty() {
                    self.data_shred_batches.push(new_data_shreds.clone());
                    vec![new_data_shreds]
                } else {
                    vec![]
                }
            };

            let coding_shred_batches = {
                if !new_coding_shreds.is_empty() {
                    self.coding_shred_batches.push(new_coding_shreds.clone());
                    vec![new_coding_shreds]
                } else {
                    vec![]
                }
            };

            if !data_shred_batches.is_empty() || !coding_shred_batches.is_empty() {
                Some(CachedBroadcastShreds {
                    slot: self.slot,
                    stakes: self.stakes.clone(),
                    data_shred_batches,
                    coding_shred_batches,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    fn get_new_shreds(
        blockstore: &Blockstore,
        slot: Slot,
        data_start_index: u64,
        coding_start_index: u64,
    ) -> (Arc<Vec<Shred>>, Arc<Vec<Shred>>) {
        let new_data_shreds = Arc::new(
            blockstore
                .get_data_shreds_for_slot(slot, data_start_index)
                .expect("My own shreds must be reconstructable"),
        );

        let new_coding_shreds = Arc::new(
            blockstore
                .get_coding_shreds_for_slot(slot, coding_start_index)
                .expect("My own shreds must be reconstructable"),
        );

        (new_data_shreds, new_coding_shreds)
    }
}

pub struct SlotTransmitShredsCache {
    cache: HashMap<Slot, CachedBroadcastShreds>,
    insertion_order: VecDeque<Slot>,
    capacity: usize,
}

impl SlotTransmitShredsCache {
    pub fn new(capacity: usize) -> Self {
        let capacity = std::cmp::max(capacity, 1);
        Self {
            cache: HashMap::new(),
            insertion_order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, slot: Slot, transmit_shreds: TransmitShreds) -> bool {
        if !self.cache.contains_key(&slot) {
            if !transmit_shreds.1.is_empty() && transmit_shreds.1[0].index() != 0 {
                // Shreds for a slot must come in order from broadcast.
                // If the cache doesn't contain this slot's earlier shreds, and
                // this batch does not contain the first shred for the slot,
                // this means this slot has already been purged from the cache,
                // so dump it.
                return false;
            }
            if self.insertion_order.len() == self.capacity {
                let old_slot = self.insertion_order.pop_front().unwrap();
                self.cache.remove(&old_slot).unwrap();
            }

            self.insertion_order.push_back(slot);
        }

        let slot_cache = self
            .cache
            .entry(slot)
            .or_insert_with(|| CachedBroadcastShreds {
                slot,
                stakes: transmit_shreds.0.clone(),
                data_shred_batches: vec![],
                coding_shred_batches: vec![],
            });

        // It's important that empty entries are still inserted
        // into the cache so that they can be updated later by
        // blockstore or broadcast later (used to track incomplete
        // retrasmits)
        if transmit_shreds.1.is_empty() {
            return true;
        }

        // Transmit shreds must be all of one type or another
        let should_push = Self::should_push(&slot_cache, &transmit_shreds);
        if should_push {
            if transmit_shreds.1[0].is_data() {
                assert!(transmit_shreds.1.iter().all(|s| s.is_data()));
                slot_cache.data_shred_batches.push(transmit_shreds.1);
            } else {
                assert!(transmit_shreds.1.iter().all(|s| !s.is_data()));
                slot_cache.coding_shred_batches.push(transmit_shreds.1);
            }
        }

        should_push
    }

    pub fn get(&self, slot: Slot) -> Option<&CachedBroadcastShreds> {
        self.cache.get(&slot)
    }

    pub fn get_or_insert(
        &mut self,
        bank: &Bank,
        blockstore: &Blockstore,
    ) -> &CachedBroadcastShreds {
        if self.cache.get(&bank.slot()).is_none() {
            let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
            let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
            let stakes = stakes.map(Arc::new);

            self.push(bank.slot(), (stakes, Arc::new(vec![])));
            self.cache
                .get_mut(&bank.slot())
                .expect("Just inserted this entry, must exist")
                .update_from_blockstore(blockstore);
        }

        self.cache.get(&bank.slot()).unwrap()
    }

    // Updates a cached slot with new shreds from blockstore and returns a
    // `CachedBroadcastShreds` representing just the diff from the updates
    pub fn update_cached_slot(
        &mut self,
        blockstore: &Blockstore,
        slot: Slot,
    ) -> Option<CachedBroadcastShreds> {
        self.cache
            .get_mut(&slot)
            .map(|cached_entry| cached_entry.update_from_blockstore(blockstore))
            .unwrap_or(None)
    }

    // Update with latest leader blocks. Note it should generally be safe to purge
    // old leader blocks, because the validator should only generate new leadder
    // blocks if the old blocks were confirmed to be propagated, which means the old
    // blocks will not need to be retransmitted, so they can be removed from the
    // cache
    pub fn update_retransmit_cache(
        &mut self,
        retransmit_cache_receiver: &RetransmitCacheReceiver,
        updates: &mut HashMap<Slot, Vec<TransmitShreds>>,
    ) -> Result<()> {
        let timer = Duration::from_millis(100);
        let (slot, new_transmit_shreds) = retransmit_cache_receiver.recv_timeout(timer)?;
        if self.push(slot, new_transmit_shreds.clone()) {
            updates
                .entry(slot)
                .or_insert_with(|| vec![new_transmit_shreds]);
        }

        while let Ok((slot, new_transmit_shreds)) = retransmit_cache_receiver.try_recv() {
            if self.push(slot, new_transmit_shreds.clone()) {
                updates
                    .entry(slot)
                    .or_insert_with(|| vec![])
                    .push(new_transmit_shreds.clone());
            }
        }

        Ok(())
    }

    fn should_push(
        cached_entry: &CachedBroadcastShreds,
        new_transmit_shreds: &TransmitShreds,
    ) -> bool {
        if new_transmit_shreds.1.is_empty() {
            return true;
        }
        // Check if updates should be added to the cache. Note that:
        //
        // 1) Other updates could have been read from the blockstore by
        // `broadcast_stage::retry_unfinished_retransmit_slots()`, but
        //
        // 2) Writes to blockstore are done atomically by the broadcast stage
        // insertion thread in batches of exactly the 'new_transmit_shreds`
        // given here, so it's sufficent to check if the first index in each
        //  batch of `transmit_shreds` is greater than the last index in the
        // current cache. If so, this implies we are missing the entire batch of
        // updates in `transmit_shreds`, and should send them to be
        // retransmitted.
        let (last_cached_data_index, last_cached_coding_index) = {
            (
                cached_entry.last_data_shred().map(|shred| shred.index()),
                cached_entry.last_coding_shred().map(|shred| shred.index()),
            )
        };

        let first_new_shred_index = new_transmit_shreds.1[0].index();

        (new_transmit_shreds.1[0].is_data()
            && (last_cached_data_index.is_none()
                || first_new_shred_index > last_cached_data_index.unwrap()))
            || (new_transmit_shreds.1[0].is_code()
                && (last_cached_coding_index.is_none()
                    || first_new_shred_index > last_cached_coding_index.unwrap()))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        broadcast_stage::test::make_transmit_shreds,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use itertools::Itertools;
    use solana_ledger::{
        blockstore::Blockstore,
        get_tmp_ledger_path,
        shred::{Shredder, RECOMMENDED_FEC_RATE},
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::signature::Keypair;
    use std::sync::Arc;

    fn get_data_and_coding_shreds(
        data_transmit_shreds: Vec<TransmitShreds>,
        coding_transmit_shreds: Vec<TransmitShreds>,
    ) -> (Vec<Arc<Vec<Shred>>>, Vec<Arc<Vec<Shred>>>) {
        let complete_data_shreds: Vec<_> = data_transmit_shreds
            .into_iter()
            .map(|(_, shreds)| shreds)
            .collect();
        let complete_coding_shreds: Vec<_> = coding_transmit_shreds
            .into_iter()
            .map(|(_, shreds)| shreds)
            .collect();

        (complete_data_shreds, complete_coding_shreds)
    }

    #[test]
    fn test_last_data_code_shreds() {
        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: vec![],
            coding_shred_batches: vec![],
        };

        assert!(cache_entry.last_data_shred().is_none());
        assert!(cache_entry.last_coding_shred().is_none());

        let (_, _, all_data_transmit_shreds, all_coding_transmit_shreds) =
            make_transmit_shreds(1, 10, None);
        let last_data_shred_index = all_data_transmit_shreds.len() - 1;
        let last_coding_shred_index = all_coding_transmit_shreds.len() - 1;
        let (data_shred_batches, coding_shred_batches) = get_data_and_coding_shreds(
            all_data_transmit_shreds.clone(),
            all_coding_transmit_shreds.clone(),
        );
        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches,
            coding_shred_batches,
        };

        assert_eq!(
            cache_entry.last_data_shred().unwrap().index() as usize,
            last_data_shred_index
        );
        assert_eq!(
            cache_entry.last_coding_shred().unwrap().index() as usize,
            last_coding_shred_index
        );
    }

    #[test]
    fn test_contains_last_shreds_non_empty_coding() {
        let mut shred = Shred::new_empty_data_shred();
        shred.set_last_in_slot();
        assert!(shred.is_data());
        let data_shreds = vec![shred];
        // RECOMMENDED_FEC_RATE must produce > 0 coding shreds per batch, or
        // the test for `contains_last_shreds()` will fail.
        let shredder =
            Shredder::new(1, 0, RECOMMENDED_FEC_RATE, Arc::new(Keypair::new()), 0, 0).unwrap();
        let coding_shred_batch = shredder.data_shreds_to_coding_shreds(&data_shreds);
        assert!(coding_shred_batch.len() > 0);
    }

    #[test]
    fn test_update_from_blockstore() {
        // Make the database ledger
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());

        let mut slot_cache_entry = CachedBroadcastShreds::default();

        // There are no updates
        assert!(slot_cache_entry
            .update_from_blockstore(&blockstore)
            .is_none());

        // Make some updates in blockstore
        let (all_data_shreds, all_coding_shreds, _, _) = make_transmit_shreds(0, 10, None);
        blockstore
            .insert_shreds(vec![all_data_shreds[0].clone()], None, true)
            .unwrap();
        blockstore
            .insert_shreds(vec![all_coding_shreds[0].clone()], None, true)
            .unwrap();

        // There is 1 new data, 1 new coding shred
        assert_eq!(
            slot_cache_entry
                .update_from_blockstore(&blockstore)
                .unwrap(),
            CachedBroadcastShreds {
                slot: 0,
                stakes: None,
                data_shred_batches: vec![Arc::new(vec![all_data_shreds[0].clone()])],
                coding_shred_batches: vec![Arc::new(vec![all_coding_shreds[0].clone()])]
            }
        );

        // There are no updates
        assert!(slot_cache_entry
            .update_from_blockstore(&blockstore)
            .is_none());
    }

    #[test]
    fn test_contains_last_shreds() {
        let (_, _, all_data_transmit_shreds, all_coding_transmit_shreds) =
            make_transmit_shreds(1, 10, None);
        assert!(all_data_transmit_shreds.len() > 1);
        assert!(all_coding_transmit_shreds.len() > 1);
        let (complete_data_shreds, complete_coding_shreds) =
            get_data_and_coding_shreds(all_data_transmit_shreds, all_coding_transmit_shreds);
        let partial_data_shreds = complete_data_shreds[..complete_data_shreds.len() - 1].to_vec();
        let partial_coding_shreds =
            complete_coding_shreds[..complete_coding_shreds.len() - 1].to_vec();

        let data_shred_bin = vec![complete_data_shreds, partial_data_shreds];
        let coding_shred_bin = vec![complete_coding_shreds, partial_coding_shreds];

        for (i, (data_shred_batches, coding_shred_batches)) in data_shred_bin
            .into_iter()
            .cartesian_product(coding_shred_bin.into_iter())
            .enumerate()
        {
            let cache_entry = CachedBroadcastShreds {
                slot: 1,
                stakes: None,
                data_shred_batches,
                coding_shred_batches,
            };
            if i == 0 {
                assert!(cache_entry.contains_last_shreds());
            } else {
                assert!(!cache_entry.contains_last_shreds());
            }
        }
    }

    #[test]
    fn test_to_transmit_shreds() {
        let (_, _, all_data_transmit_shreds, all_coding_transmit_shreds) =
            make_transmit_shreds(1, 10, None);
        let (complete_data_shreds, complete_coding_shreds) = get_data_and_coding_shreds(
            all_data_transmit_shreds.clone(),
            all_coding_transmit_shreds.clone(),
        );

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: vec![],
            coding_shred_batches: vec![],
        };
        assert_eq!(cache_entry.to_transmit_shreds(), vec![]);

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: complete_data_shreds.clone(),
            coding_shred_batches: vec![],
        };
        assert_eq!(cache_entry.to_transmit_shreds(), all_data_transmit_shreds);

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: vec![],
            coding_shred_batches: complete_coding_shreds.clone(),
        };
        assert_eq!(cache_entry.to_transmit_shreds(), all_coding_transmit_shreds);

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: complete_data_shreds.clone(),
            coding_shred_batches: complete_coding_shreds,
        };
        assert_eq!(
            cache_entry.to_transmit_shreds(),
            all_data_transmit_shreds
                .into_iter()
                .chain(all_coding_transmit_shreds.into_iter())
                .collect::<Vec<_>>()
        );

        let stakes = Some(Arc::new(HashMap::new()));
        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: stakes.clone(),
            data_shred_batches: vec![complete_data_shreds[0].clone()],
            coding_shred_batches: vec![],
        };

        assert_eq!(
            cache_entry.to_transmit_shreds(),
            vec![(stakes, complete_data_shreds[0].clone())]
        );
    }

    #[test]
    fn test_should_push() {
        let (_, _, all_data_transmit_shreds, all_coding_transmit_shreds) =
            make_transmit_shreds(1, 10, None);
        let (complete_data_shreds, complete_coding_shreds) =
            get_data_and_coding_shreds(all_data_transmit_shreds, all_coding_transmit_shreds);

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: vec![],
            coding_shred_batches: vec![],
        };

        // Empty cache entry should always push
        assert!(SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_data_shreds[0].clone())
        ));
        assert!(SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_coding_shreds[0].clone())
        ));

        let cache_entry = CachedBroadcastShreds {
            slot: 1,
            stakes: None,
            data_shred_batches: complete_data_shreds[0..=1].to_vec(),
            coding_shred_batches: complete_coding_shreds[0..=1].to_vec(),
        };

        // Empty TransmitShreds to push should return true
        assert!(SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, Arc::new(vec![]))
        ));

        // Greater indexes than the last shred in the last inserted batch of
        // TransmitShreds should pass, lesser or equal should fail
        assert!(!SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_data_shreds[1].clone())
        ));
        assert!(SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_data_shreds[2].clone())
        ));
        assert!(!SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_data_shreds[0].clone())
        ));
        assert!(SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_coding_shreds[2].clone())
        ));
        assert!(!SlotTransmitShredsCache::should_push(
            &cache_entry,
            &(None, complete_coding_shreds[0].clone())
        ));
    }

    #[test]
    fn test_push_get() {
        let (_, _, all_data_transmit_shreds, all_coding_transmit_shreds) =
            make_transmit_shreds(1, 10, None);

        let mut cache = SlotTransmitShredsCache::new(0);
        assert!(cache.capacity > 0);

        cache = SlotTransmitShredsCache::new(2);
        // Pushing empty should still make an entry in the cache
        assert!(cache.push(0, (None, Arc::new(vec![]))));
        cache.get(0).unwrap().to_transmit_shreds().is_empty();

        // Pushing same thing twice should fail
        assert!(cache.push(0, all_data_transmit_shreds[0].clone()));
        assert!(!cache.push(0, all_data_transmit_shreds[0].clone()));

        // Pushing new later shred should succeed
        assert!(cache.push(0, all_data_transmit_shreds[1].clone()));
        assert_eq!(
            cache.get(0).unwrap().to_transmit_shreds(),
            all_data_transmit_shreds[0..=1].to_vec()
        );

        // Trying to push a new slot starting at non-zero index should
        // fail
        assert!(!cache.push(1, all_data_transmit_shreds[2].clone()));
        assert!(!cache.push(1, all_coding_transmit_shreds[2].clone()));
        assert!(cache.get(1).is_none());

        // Pushing more than the capacity of 2 should purge earlier slots
        assert!(cache.push(1, all_data_transmit_shreds[0].clone()));
        assert!(cache.push(2, all_data_transmit_shreds[0].clone()));
        assert!(cache.get(0).is_none());
        assert_eq!(
            cache.get(1).unwrap().to_transmit_shreds(),
            vec![all_data_transmit_shreds[0].clone()]
        );
        assert_eq!(
            cache.get(2).unwrap().to_transmit_shreds(),
            vec![all_data_transmit_shreds[0].clone()]
        );
    }

    #[test]
    fn test_get_or_insert() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let bank_epoch = bank0.get_leader_schedule_epoch(bank0.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank0, bank_epoch);
        let stakes = stakes.map(Arc::new);
        let (all_data_shreds, all_coding_shreds, _, _) =
            make_transmit_shreds(0, 10, stakes.clone());

        // Make the database ledger
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let mut cache = SlotTransmitShredsCache::new(2);

        // Blockstore is empty, should insert empty entry into cache, but
        // with valid stakes
        let mut default = CachedBroadcastShreds::default();
        default.stakes = stakes.clone();
        default.slot = 0;
        assert_eq!(*cache.get_or_insert(&bank0, &blockstore), default);
        assert_eq!(
            cache.get(bank0.slot()).unwrap().to_transmit_shreds(),
            vec![]
        );

        // Insert shreds into blockstore
        blockstore
            .insert_shreds(all_data_shreds.clone(), None, true)
            .unwrap();
        blockstore
            .insert_shreds(all_coding_shreds.clone(), None, true)
            .unwrap();

        // Gettng without update still returns old cached entry
        assert_eq!(*cache.get_or_insert(&bank0, &blockstore), default);

        // Clearing cache and getting slot now should return
        // everything in blockstore
        cache = SlotTransmitShredsCache::new(2);
        let cached_entry = cache.get_or_insert(&bank0, &blockstore);
        assert_eq!(cached_entry.slot, 0);
        assert_eq!(cached_entry.stakes, stakes);
        assert_eq!(
            cached_entry.data_shred_batches,
            vec![Arc::new(all_data_shreds)]
        );
        assert_eq!(
            cached_entry.coding_shred_batches,
            vec![Arc::new(all_coding_shreds)]
        );
    }
}
