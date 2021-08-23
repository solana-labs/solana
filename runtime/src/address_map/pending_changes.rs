use {
    dashmap::{
        mapref::multiple::RefMulti as MapEntry, setref::multiple::RefMulti as SetEntry, DashMap,
        DashSet,
    },
    solana_address_map_program::{AddressMap, SerializationError},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::sync::Arc,
};

/// Address map activations and deactivations that occurred in a single slot and
/// will be applied to the address map cache when the bank has been rooted.
#[derive(Debug)]
pub struct AddressMapPendingChanges {
    slot: Slot,
    activations: DashMap<Pubkey, Arc<Vec<Pubkey>>>,
    deactivations: DashSet<Pubkey>,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for AddressMapPendingChanges {
    fn example() -> Self {
        AddressMapPendingChanges::new(0)
    }
}

impl AddressMapPendingChanges {
    /// Create new pending changes collection for a slot
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            activations: DashMap::default(),
            deactivations: DashSet::default(),
        }
    }

    #[cfg(test)]
    pub fn new_for_tests(
        activations: DashMap<Pubkey, Arc<Vec<Pubkey>>>,
        deactivations: DashSet<Pubkey>,
    ) -> Self {
        Self {
            slot: 0,
            activations,
            deactivations,
        }
    }

    /// Iterator over all address maps activated in a single bank
    pub fn activations_iter(&self) -> impl Iterator<Item = MapEntry<Pubkey, Arc<Vec<Pubkey>>>> {
        self.activations.iter()
    }

    /// Iterator over all address maps deactivated in a single bank
    pub fn deactivations_iter(&self) -> impl Iterator<Item = SetEntry<Pubkey>> {
        self.deactivations.iter()
    }

    /// Record pending change for address map account if it was newly activated or deactivated
    pub fn record(&self, key: &Pubkey, map_data: &[u8]) -> Result<(), SerializationError> {
        let address_map = AddressMap::deserialize(map_data)?;
        if self.slot == address_map.activation_slot {
            self.activations
                .entry(*key)
                .or_try_insert_with(|| -> Result<_, SerializationError> {
                    Ok(Arc::new(address_map.deserialize_entries(map_data)?))
                })
                .map(|_| ())?;
        }

        // If an address map is deactivated in the same slot that it is
        // activated, it will not be usable for mapping transaction
        // addresses if the warmup time is less than or equal to the
        // cooldown time. However, the cooldown must still be observed
        // before the account can be closed.
        if self.slot == address_map.deactivation_slot {
            self.deactivations.insert(*key);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_uninitialized_map() {
        let changes = AddressMapPendingChanges::new(0);
        let map_key = Pubkey::new_unique();
        assert_eq!(
            changes.record(&map_key, &[]).err(),
            Some(SerializationError::BincodeError)
        );
        assert!(changes.activations.get(&map_key).is_none());
        assert!(changes.deactivations.get(&map_key).is_none());
    }

    #[test]
    fn test_record_invalid_entries_map() {
        let slot = 1;
        let changes = AddressMapPendingChanges::new(slot);
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: None,
            activation_slot: 1,
            deactivation_slot: Slot::MAX,
            num_entries: 1,
        }
        .serialize_with_entries(&[]);
        assert_eq!(
            changes.record(&map_key, &map_data).err(),
            Some(SerializationError::InvalidNumEntries(1))
        );
        assert!(changes.activations.get(&map_key).is_none());
        assert!(changes.deactivations.get(&map_key).is_none());
    }

    #[test]
    fn test_record_with_instant_deactivation() {
        let slot = 1;
        let changes = AddressMapPendingChanges::new(slot);
        let entries = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: None,
            activation_slot: slot,
            deactivation_slot: slot,
            num_entries: 2,
        }
        .serialize_with_entries(&entries);
        assert!(changes.record(&map_key, &map_data).is_ok());
        assert_eq!(
            changes.activations.get(&map_key).unwrap().value(),
            &Arc::new(entries),
        );
        assert!(changes.deactivations.get(&map_key).is_some());
    }

    #[test]
    fn test_record_activation() {
        let slot = 1;
        let changes = AddressMapPendingChanges::new(slot);
        let entries = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: None,
            activation_slot: 1,
            deactivation_slot: Slot::MAX,
            num_entries: 2,
        }
        .serialize_with_entries(&entries);
        assert!(changes.record(&map_key, &map_data).is_ok());
        assert_eq!(
            changes.activations.get(&map_key).unwrap().value().as_ref(),
            &entries,
        );
        assert!(changes.deactivations.get(&map_key).is_none());
    }

    #[test]
    fn test_record_deactivation() {
        let slot = 1;
        let changes = AddressMapPendingChanges::new(slot);
        let entries = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let map_key = Pubkey::new_unique();
        let map_data = AddressMap {
            authority: None,
            activation_slot: slot - 1,
            deactivation_slot: slot,
            num_entries: 2,
        }
        .serialize_with_entries(&entries);
        assert!(changes.record(&map_key, &map_data).is_ok());
        assert!(changes.activations.get(&map_key).is_none());
        assert!(changes.deactivations.get(&map_key).is_some());
    }
}
