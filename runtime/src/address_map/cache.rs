use {
    crate::address_map::{AddressMapPendingChanges, ACTIVATION_WARMUP, DEACTIVATION_COOLDOWN},
    dashmap::DashMap,
    log::warn,
    solana_address_map_program::{AddressMap, SerializationError},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        message::{v0, MappedAddresses},
        pubkey::Pubkey,
    },
    std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thiserror::Error,
};

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum CacheError {
    /// Address map was not found in the cache
    #[error("Address map {0} was not found")]
    MapNotFound(Pubkey),

    /// Address map is inactive and may not be used
    #[error("Address map {0} is inactive")]
    MapInactive(Pubkey),

    /// Address map has been activated but is still warming up
    #[error("Address map {0} has been activated but is still activating until epoch {1}")]
    MapStillActivating(Pubkey, Epoch),

    /// Address map accessed with invalid entry index
    #[error("Address map {0} does not contain an entry at index {1}")]
    InvalidEntryIndex(Pubkey, u8),
}

#[derive(Debug, PartialEq, Eq)]
enum MapStatus {
    /// Address map has been activated but has not fully warmed up
    Activating(Epoch),
    /// Address map is active
    Active,
    /// Address map has deactivated but has not finished cooling down
    Deactivating,
    /// Address map is inactive
    Inactive,
}

/// Cached address map with activation information.
#[derive(Debug, PartialEq)]
struct CachedAddressMap {
    entries: Arc<Vec<Pubkey>>,
    activation_epoch: Epoch,
    deactivation_epoch: Option<Epoch>,
}

impl CachedAddressMap {
    /// Check the status of a cached address map to see if it is active for
    /// mapping transaction message addresses.
    fn status(&self, current_epoch: Epoch) -> MapStatus {
        if current_epoch < self.activation_epoch {
            return MapStatus::Inactive;
        }

        let first_active_epoch = self.activation_epoch.saturating_add(ACTIVATION_WARMUP);
        let deactivation_epoch = self.deactivation_epoch.unwrap_or(Epoch::MAX);
        let first_inactive_epoch = deactivation_epoch.saturating_add(DEACTIVATION_COOLDOWN);

        // handle special case where a map can never be active
        if first_active_epoch == first_inactive_epoch {
            return MapStatus::Inactive;
        }

        if current_epoch < first_active_epoch {
            MapStatus::Activating(first_active_epoch)
        } else if current_epoch < deactivation_epoch {
            MapStatus::Active
        } else if current_epoch < first_inactive_epoch {
            MapStatus::Deactivating
        } else {
            MapStatus::Inactive
        }
    }
}

/// Global cache which includes all active address maps for the current epoch.
#[derive(Debug, Default)]
pub struct AddressMapCache {
    address_maps: DashMap<Pubkey, CachedAddressMap>,
    epoch_schedule: EpochSchedule,
    last_rooted_bank_epoch: AtomicU64,
}

impl AddressMapCache {
    /// Create a new address map cache with an epoch schedule
    pub fn new(epoch_schedule: EpochSchedule) -> Self {
        Self {
            address_maps: DashMap::new(),
            epoch_schedule,
            last_rooted_bank_epoch: AtomicU64::new(0),
        }
    }

    /// Check if an account is owned by the address map program
    pub fn is_address_map(account: &AccountSharedData) -> bool {
        solana_address_map_program::check_id(account.owner())
    }

    /// Look up an address map in the cache and return its address entries if
    /// it's active for the current epoch.
    fn get_active_map_entries(
        &self,
        key: &Pubkey,
        current_epoch: Epoch,
    ) -> Result<Arc<Vec<Pubkey>>, CacheError> {
        if let Some(address_map) = self.address_maps.get(key) {
            match address_map.status(current_epoch) {
                MapStatus::Active | MapStatus::Deactivating => Ok(Arc::clone(&address_map.entries)),
                MapStatus::Activating(first_active_epoch) => {
                    Err(CacheError::MapStillActivating(*key, first_active_epoch))
                }
                MapStatus::Inactive => Err(CacheError::MapInactive(*key)),
            }
        } else {
            Err(CacheError::MapNotFound(*key))
        }
    }

    /// Map a message's address map indexes to full addresses if the address
    /// maps referenced in the message are valid and active for the current
    /// epoch.
    pub fn map_message_addresses(
        &self,
        message: &v0::Message,
        current_epoch: Epoch,
    ) -> Result<MappedAddresses, CacheError> {
        let mut mapped_addresses = MappedAddresses {
            writable: Vec::with_capacity(message.num_writable_map_indexes()),
            readonly: Vec::with_capacity(message.num_readonly_map_indexes()),
        };

        for (key, indexes) in message.address_map_indexes_iter() {
            let map_entries = self.get_active_map_entries(key, current_epoch)?;
            let lookup_address = |index: &u8| -> Result<&Pubkey, CacheError> {
                map_entries
                    .get(usize::from(*index))
                    .ok_or(CacheError::InvalidEntryIndex(*key, *index))
            };

            mapped_addresses.writable.extend(
                indexes
                    .writable
                    .iter()
                    .map(lookup_address)
                    .collect::<Result<Vec<_>, CacheError>>()?,
            );

            mapped_addresses.readonly.extend(
                indexes
                    .readonly
                    .iter()
                    .map(lookup_address)
                    .collect::<Result<Vec<_>, CacheError>>()?,
            );
        }

        Ok(mapped_addresses)
    }

    /// Insert newly activated map into the cache
    fn insert_activated_map(
        &self,
        key: Pubkey,
        entries: Arc<Vec<Pubkey>>,
        activation_epoch: Epoch,
    ) {
        let cached_address_map = CachedAddressMap {
            entries,
            activation_epoch,
            deactivation_epoch: None,
        };

        if self.address_maps.insert(key, cached_address_map).is_some() {
            warn!("Overwrote activated address map: {}", key);
        }
    }

    /// Update the address map cache with the pending changes from a rooted
    /// bank. Use the bank's epoch to record when address maps have started
    /// warming up or cooling down for new activations and deactivations,
    /// respectively. Also use the epoch for pruning any deactivated maps that
    /// have finished cooling down.
    pub fn update_cache(
        &self,
        rooted_bank_changes: &AddressMapPendingChanges,
        rooted_bank_epoch: Epoch,
    ) {
        rooted_bank_changes
            .activations_iter()
            .for_each(|activated_map| {
                self.insert_activated_map(
                    *activated_map.key(),
                    Arc::clone(activated_map.value()),
                    rooted_bank_epoch,
                );
            });

        rooted_bank_changes
            .deactivations_iter()
            .for_each(|deactivated_map| {
                if let Some(mut address_map) = self.address_maps.get_mut(deactivated_map.key()) {
                    address_map.value_mut().deactivation_epoch = Some(rooted_bank_epoch);
                } else {
                    warn!(
                        "Failed to find address map: {} to deactivate",
                        deactivated_map.key()
                    );
                }
            });

        // Only prune when we encounter a new epoch to avoid doing unnecessary work for each bank
        if self
            .last_rooted_bank_epoch
            .swap(rooted_bank_epoch, Ordering::Relaxed)
            < rooted_bank_epoch
        {
            self.prune_deactivated_maps(rooted_bank_epoch);
        }
    }

    /// Prune all cached address maps that have been deactivated and passed the cooldown period
    fn prune_deactivated_maps(&self, rooted_bank_epoch: Epoch) {
        self.address_maps.retain(|_address, address_map| {
            if let Some(deactivation_epoch) = address_map.deactivation_epoch {
                rooted_bank_epoch < deactivation_epoch.saturating_add(DEACTIVATION_COOLDOWN)
            } else {
                true
            }
        });
    }

    /// Called during snapshot processing to populate the cache from rooted slots
    pub fn populate_cache(
        &self,
        map_key: &Pubkey,
        map_data: &[u8],
    ) -> Result<(), SerializationError> {
        let address_map = AddressMap::deserialize(map_data)?;
        let activation_epoch = match address_map.activation_slot {
            Slot::MAX => None,
            slot => Some(self.epoch_schedule.get_epoch(slot)),
        };

        let deactivation_epoch = match address_map.deactivation_slot {
            Slot::MAX => None,
            slot => Some(self.epoch_schedule.get_epoch(slot)),
        };

        // Note that `populate_cache` could be called for slots across multiple
        // epochs without pruning deactivated address maps. Since address maps
        // may not be recreated at the same address, it's safe to assume that we
        // will not encounter two different sets of address map entries at the
        // same address.
        if let Some(activation_epoch) = activation_epoch {
            self.address_maps
                .entry(*map_key)
                .and_modify(|cached_address_map| {
                    // Entries and activation epoch cannot be modified after activation.
                    cached_address_map.deactivation_epoch = deactivation_epoch;
                })
                .or_try_insert_with(|| {
                    Ok(CachedAddressMap {
                        entries: Arc::new(address_map.deserialize_entries(map_data)?),
                        activation_epoch,
                        deactivation_epoch,
                    })
                })
                .map(|_| ())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MapStatus as Status;
    use super::*;
    use dashmap::DashSet;
    use solana_address_map_program::AddressMapState;
    use solana_sdk::message::v0::AddressMapIndexes;

    #[test]
    fn test_cached_address_map_status() {
        fn create_test_map(
            activation_epoch: Epoch,
            deactivation_epoch: Option<Epoch>,
        ) -> CachedAddressMap {
            CachedAddressMap {
                entries: Arc::default(),
                activation_epoch,
                deactivation_epoch,
            }
        }

        assert_eq!(create_test_map(1, None).status(0), Status::Inactive);
        assert_eq!(create_test_map(1, None).status(1), Status::Activating(3));
        assert_eq!(create_test_map(1, None).status(2), Status::Activating(3));
        assert_eq!(create_test_map(1, None).status(3), Status::Active);
        assert_eq!(create_test_map(1, None).status(4), Status::Active);

        assert_eq!(create_test_map(1, Some(1)).status(0), Status::Inactive);
        assert_eq!(create_test_map(1, Some(1)).status(1), Status::Inactive);
        assert_eq!(create_test_map(1, Some(1)).status(2), Status::Inactive);
        assert_eq!(create_test_map(1, Some(1)).status(3), Status::Inactive);
        assert_eq!(create_test_map(1, Some(1)).status(4), Status::Inactive);

        assert_eq!(create_test_map(1, Some(2)).status(0), Status::Inactive);
        assert_eq!(create_test_map(1, Some(2)).status(1), Status::Activating(3));
        assert_eq!(create_test_map(1, Some(2)).status(2), Status::Activating(3));
        assert_eq!(create_test_map(1, Some(2)).status(3), Status::Deactivating);
        assert_eq!(create_test_map(1, Some(2)).status(4), Status::Inactive);

        assert_eq!(create_test_map(1, Some(4)).status(0), Status::Inactive);
        assert_eq!(create_test_map(1, Some(4)).status(1), Status::Activating(3));
        assert_eq!(create_test_map(1, Some(4)).status(2), Status::Activating(3));
        assert_eq!(create_test_map(1, Some(4)).status(3), Status::Active);
        assert_eq!(create_test_map(1, Some(4)).status(4), Status::Deactivating);
        assert_eq!(create_test_map(1, Some(4)).status(5), Status::Deactivating);
        assert_eq!(create_test_map(1, Some(4)).status(6), Status::Inactive);
    }

    #[test]
    fn test_get_active_map_entries() {
        let current_epoch = 10;
        let active_map_key = Pubkey::new_unique();
        let activating_map_key = Pubkey::new_unique();
        let inactive_map_key = Pubkey::new_unique();
        let entries = Arc::new(vec![Pubkey::new_unique()]);

        let address_map_cache = {
            let cache = AddressMapCache::default();
            cache.insert_activated_map(
                active_map_key,
                entries.clone(),
                current_epoch - ACTIVATION_WARMUP,
            );
            cache.insert_activated_map(activating_map_key, entries.clone(), current_epoch);
            cache.insert_activated_map(inactive_map_key, entries.clone(), current_epoch + 1);
            cache
        };

        assert_eq!(
            address_map_cache.get_active_map_entries(&active_map_key, current_epoch),
            Ok(entries)
        );

        assert_eq!(
            address_map_cache
                .get_active_map_entries(&activating_map_key, current_epoch)
                .err(),
            Some(CacheError::MapStillActivating(
                activating_map_key,
                current_epoch + ACTIVATION_WARMUP
            ))
        );

        assert_eq!(
            address_map_cache
                .get_active_map_entries(&inactive_map_key, current_epoch)
                .err(),
            Some(CacheError::MapInactive(inactive_map_key))
        );

        let unknown_map_key = Pubkey::new_unique();
        assert_eq!(
            address_map_cache
                .get_active_map_entries(&unknown_map_key, current_epoch)
                .err(),
            Some(CacheError::MapNotFound(unknown_map_key))
        );
    }

    #[test]
    fn test_map_message_addresses() {
        let address_map1 = (
            Pubkey::new_unique(),
            vec![Pubkey::new_unique(), Pubkey::new_unique()],
        );
        let address_map2 = (
            Pubkey::new_unique(),
            vec![Pubkey::new_unique(), Pubkey::new_unique()],
        );

        let activation_epoch = 0;
        let current_epoch = activation_epoch + ACTIVATION_WARMUP;
        let address_map_cache = {
            let cache = AddressMapCache::default();
            for (map_key, map_entries) in &[&address_map1, &address_map2] {
                cache.insert_activated_map(
                    *map_key,
                    Arc::new(map_entries.clone()),
                    activation_epoch,
                );
            }
            cache
        };

        // map valid message #1
        assert_eq!(
            address_map_cache.map_message_addresses(
                &v0::Message {
                    account_keys: vec![address_map1.0, address_map2.0],
                    address_map_indexes: vec![
                        AddressMapIndexes {
                            writable: vec![0, 1],
                            readonly: vec![],
                        },
                        AddressMapIndexes {
                            writable: vec![],
                            readonly: vec![0, 1],
                        },
                    ],
                    ..v0::Message::default()
                },
                current_epoch
            ),
            Ok(MappedAddresses {
                writable: vec![address_map1.1[0], address_map1.1[1]],
                readonly: vec![address_map2.1[0], address_map2.1[1]],
            })
        );

        // map valid message #2
        assert_eq!(
            address_map_cache.map_message_addresses(
                &v0::Message {
                    account_keys: vec![Pubkey::new_unique(), address_map1.0, address_map2.0,],
                    address_map_indexes: vec![
                        AddressMapIndexes {
                            writable: vec![0],
                            readonly: vec![1],
                        },
                        AddressMapIndexes {
                            writable: vec![0],
                            readonly: vec![1],
                        },
                    ],
                    ..v0::Message::default()
                },
                current_epoch
            ),
            Ok(MappedAddresses {
                writable: vec![address_map1.1[0], address_map2.1[0],],
                readonly: vec![address_map1.1[1], address_map2.1[1],],
            })
        );

        // Try to use invalid address map index
        assert_eq!(
            address_map_cache
                .map_message_addresses(
                    &v0::Message {
                        account_keys: vec![address_map1.0, address_map2.0,],
                        address_map_indexes: vec![
                            AddressMapIndexes {
                                writable: vec![0, 1, 2],
                                readonly: vec![],
                            },
                            AddressMapIndexes {
                                writable: vec![0],
                                readonly: vec![1],
                            },
                        ],
                        ..v0::Message::default()
                    },
                    current_epoch
                )
                .err(),
            Some(CacheError::InvalidEntryIndex(address_map1.0, 2)),
        );

        // Try to map unknown address map
        let unknown_map_key = Pubkey::new_unique();
        assert_eq!(
            address_map_cache
                .map_message_addresses(
                    &v0::Message {
                        account_keys: vec![address_map1.0, unknown_map_key],
                        address_map_indexes: vec![
                            AddressMapIndexes {
                                writable: vec![0],
                                readonly: vec![1],
                            },
                            AddressMapIndexes {
                                writable: vec![0],
                                readonly: vec![1],
                            },
                        ],
                        ..v0::Message::default()
                    },
                    current_epoch
                )
                .err(),
            Some(CacheError::MapNotFound(unknown_map_key)),
        );
    }

    #[test]
    fn test_update_cache() {
        let mut rooted_bank_epoch = 0;
        let deactivated_map_key = Pubkey::new_unique();
        let cache = {
            let cache = AddressMapCache::default();
            cache.address_maps.insert(
                deactivated_map_key,
                CachedAddressMap {
                    entries: Arc::default(),
                    activation_epoch: rooted_bank_epoch,
                    deactivation_epoch: Some(rooted_bank_epoch),
                },
            );
            cache
        };

        assert_eq!(cache.address_maps.len(), 1);
        assert_eq!(
            cache.last_rooted_bank_epoch.load(Ordering::Relaxed),
            rooted_bank_epoch
        );

        let address_map1 = (
            Pubkey::new_unique(),
            Arc::new(vec![Pubkey::new_unique(), Pubkey::new_unique()]),
        );
        let address_map2 = (
            Pubkey::new_unique(),
            Arc::new(vec![Pubkey::new_unique(), Pubkey::new_unique()]),
        );

        let activations = {
            let dashmap = DashMap::new();
            dashmap.insert(address_map1.0, Arc::clone(&address_map1.1));
            dashmap.insert(address_map2.0, Arc::clone(&address_map2.1));
            dashmap
        };

        let deactivations = {
            let dashset = DashSet::new();
            dashset.insert(address_map1.0);
            dashset
        };

        // This update call should prune the initial deactivated address map
        rooted_bank_epoch += DEACTIVATION_COOLDOWN;
        cache.update_cache(
            &AddressMapPendingChanges::new_for_tests(activations, deactivations),
            rooted_bank_epoch,
        );

        assert_eq!(
            cache.last_rooted_bank_epoch.load(Ordering::Relaxed),
            rooted_bank_epoch
        );
        assert_eq!(cache.address_maps.len(), 2);

        // This update call should prune the address map that was deactivated from pending changes
        rooted_bank_epoch += DEACTIVATION_COOLDOWN;
        cache.update_cache(
            &AddressMapPendingChanges::new_for_tests(DashMap::default(), DashSet::default()),
            rooted_bank_epoch,
        );

        assert_eq!(
            cache.last_rooted_bank_epoch.load(Ordering::Relaxed),
            rooted_bank_epoch
        );
        assert_eq!(cache.address_maps.len(), 1);
    }

    #[test]
    fn test_populate_cache_invalid_map() {
        let cache = AddressMapCache::default();
        let map_key = Pubkey::new_unique();
        let map_data = vec![];
        assert_eq!(
            cache.populate_cache(&map_key, &map_data).err(),
            Some(SerializationError::BincodeError)
        );
        assert!(!cache.address_maps.contains_key(&map_key));
    }

    #[test]
    fn test_populate_cache_uninitialized_map() {
        let cache = AddressMapCache::default();
        let map_key = Pubkey::new_unique();
        let uninitialized_data = bincode::serialize(&AddressMapState::Uninitialized).unwrap();
        assert_eq!(
            cache.populate_cache(&map_key, &uninitialized_data).err(),
            Some(SerializationError::Uninitialized)
        );
        assert!(!cache.address_maps.contains_key(&map_key));
    }

    #[test]
    fn test_populate_cache_invalid_entries_map() {
        let cache = AddressMapCache::default();
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: Some(Pubkey::new_unique()),
            activation_slot: 0,
            deactivation_slot: 0,
            num_entries: 1,
        }
        .serialize_with_entries(&[]);
        assert_eq!(
            cache.populate_cache(&map_key, &map_data).err(),
            Some(SerializationError::InvalidNumEntries(1))
        );
        assert!(!cache.address_maps.contains_key(&map_key));
    }

    #[test]
    fn test_populate_cache_no_entries_map() {
        let cache = AddressMapCache::default();
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: Some(Pubkey::new_unique()),
            activation_slot: 0,
            deactivation_slot: 0,
            num_entries: 0,
        }
        .serialize_with_entries(&[]);
        assert!(cache.populate_cache(&map_key, &map_data).is_ok());
        assert!(cache.address_maps.contains_key(&map_key));
    }

    #[test]
    fn test_populate_cache() {
        let cache = AddressMapCache::default();
        let activation_epoch = 1;
        let entries = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let mut map = AddressMap {
            authority: Some(Pubkey::new_unique()),
            activation_slot: cache
                .epoch_schedule
                .get_first_slot_in_epoch(activation_epoch),
            deactivation_slot: Slot::MAX,
            num_entries: 2,
        };
        let map_key = Pubkey::new_unique();
        let map_data = map.serialize_with_entries(&entries);
        assert!(cache.populate_cache(&map_key, &map_data).is_ok());
        assert_eq!(
            cache.address_maps.get(&map_key).unwrap().value(),
            &CachedAddressMap {
                entries: Arc::new(entries.clone()),
                activation_epoch,
                deactivation_epoch: None,
            }
        );

        let deactivation_epoch = 2;
        map.deactivation_slot = cache
            .epoch_schedule
            .get_first_slot_in_epoch(deactivation_epoch);
        let updated_map_data = map.serialize_with_entries(&entries);
        assert!(cache.populate_cache(&map_key, &updated_map_data).is_ok());
        assert_eq!(
            cache.address_maps.get(&map_key).unwrap().value(),
            &CachedAddressMap {
                entries: Arc::new(entries),
                activation_epoch,
                deactivation_epoch: Some(deactivation_epoch),
            }
        );
    }
}
