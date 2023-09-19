use {
    serde::{Deserialize, Serialize},
    solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample},
    solana_program::{
        address_lookup_table::error::AddressLookupError,
        clock::Slot,
        instruction::InstructionError,
        pubkey::Pubkey,
        slot_hashes::{SlotHashes, MAX_ENTRIES},
    },
    std::borrow::Cow,
};

/// The maximum number of addresses that a lookup table can hold
pub const LOOKUP_TABLE_MAX_ADDRESSES: usize = 256;

/// The serialized size of lookup table metadata
pub const LOOKUP_TABLE_META_SIZE: usize = 56;

/// Activation status of a lookup table
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum LookupTableStatus {
    Activated,
    Deactivating { remaining_blocks: usize },
    Deactivated,
}

/// Address lookup table metadata
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, AbiExample)]
pub struct LookupTableMeta {
    /// Lookup tables cannot be closed until the deactivation slot is
    /// no longer "recent" (not accessible in the `SlotHashes` sysvar).
    pub deactivation_slot: Slot,
    /// The slot that the table was last extended. Address tables may
    /// only be used to lookup addresses that were extended before
    /// the current bank's slot.
    pub last_extended_slot: Slot,
    /// The start index where the table was last extended from during
    /// the `last_extended_slot`.
    pub last_extended_slot_start_index: u8,
    /// Authority address which must sign for each modification.
    pub authority: Option<Pubkey>,
    // Padding to keep addresses 8-byte aligned
    pub _padding: u16,
    // Raw list of addresses follows this serialized structure in
    // the account's data, starting from `LOOKUP_TABLE_META_SIZE`.
}

impl Default for LookupTableMeta {
    fn default() -> Self {
        Self {
            deactivation_slot: Slot::MAX,
            last_extended_slot: 0,
            last_extended_slot_start_index: 0,
            authority: None,
            _padding: 0,
        }
    }
}

impl LookupTableMeta {
    pub fn new(authority: Pubkey) -> Self {
        LookupTableMeta {
            authority: Some(authority),
            ..LookupTableMeta::default()
        }
    }

    /// Returns whether the table is considered active for address lookups
    pub fn is_active(&self, current_slot: Slot, slot_hashes: &SlotHashes) -> bool {
        match self.status(current_slot, slot_hashes) {
            LookupTableStatus::Activated => true,
            LookupTableStatus::Deactivating { .. } => true,
            LookupTableStatus::Deactivated => false,
        }
    }

    /// Return the current status of the lookup table
    pub fn status(&self, current_slot: Slot, slot_hashes: &SlotHashes) -> LookupTableStatus {
        if self.deactivation_slot == Slot::MAX {
            LookupTableStatus::Activated
        } else if self.deactivation_slot == current_slot {
            LookupTableStatus::Deactivating {
                remaining_blocks: MAX_ENTRIES.saturating_add(1),
            }
        } else if let Some(slot_hash_position) = slot_hashes.position(&self.deactivation_slot) {
            // Deactivation requires a cool-down period to give in-flight transactions
            // enough time to land and to remove indeterminism caused by transactions loading
            // addresses in the same slot when a table is closed. The cool-down period is
            // equivalent to the amount of time it takes for a slot to be removed from the
            // slot hash list.
            //
            // By using the slot hash to enforce the cool-down, there is a side effect
            // of not allowing lookup tables to be recreated at the same derived address
            // because tables must be created at an address derived from a recent slot.
            LookupTableStatus::Deactivating {
                remaining_blocks: MAX_ENTRIES.saturating_sub(slot_hash_position),
            }
        } else {
            LookupTableStatus::Deactivated
        }
    }
}

/// Program account states
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, AbiExample, AbiEnumVisitor)]
#[allow(clippy::large_enum_variant)]
pub enum ProgramState {
    /// Account is not initialized.
    Uninitialized,
    /// Initialized `LookupTable` account.
    LookupTable(LookupTableMeta),
}

#[derive(Debug, PartialEq, Eq, Clone, AbiExample)]
pub struct AddressLookupTable<'a> {
    pub meta: LookupTableMeta,
    pub addresses: Cow<'a, [Pubkey]>,
}

impl<'a> AddressLookupTable<'a> {
    /// Serialize an address table's updated meta data and zero
    /// any leftover bytes.
    pub fn overwrite_meta_data(
        data: &mut [u8],
        lookup_table_meta: LookupTableMeta,
    ) -> Result<(), InstructionError> {
        let meta_data = data
            .get_mut(0..LOOKUP_TABLE_META_SIZE)
            .ok_or(InstructionError::InvalidAccountData)?;
        meta_data.fill(0);
        bincode::serialize_into(meta_data, &ProgramState::LookupTable(lookup_table_meta))
            .map_err(|_| InstructionError::GenericError)?;
        Ok(())
    }

    /// Get the length of addresses that are active for lookups
    pub fn get_active_addresses_len(
        &self,
        current_slot: Slot,
        slot_hashes: &SlotHashes,
    ) -> Result<usize, AddressLookupError> {
        if !self.meta.is_active(current_slot, slot_hashes) {
            // Once a lookup table is no longer active, it can be closed
            // at any point, so returning a specific error for deactivated
            // lookup tables could result in a race condition.
            return Err(AddressLookupError::LookupTableAccountNotFound);
        }

        // If the address table was extended in the same slot in which it is used
        // to lookup addresses for another transaction, the recently extended
        // addresses are not considered active and won't be accessible.
        let active_addresses_len = if current_slot > self.meta.last_extended_slot {
            self.addresses.len()
        } else {
            self.meta.last_extended_slot_start_index as usize
        };

        Ok(active_addresses_len)
    }

    /// Lookup addresses for provided table indexes. Since lookups are performed on
    /// tables which are not read-locked, this implementation needs to be careful
    /// about resolving addresses consistently.
    pub fn lookup(
        &self,
        current_slot: Slot,
        indexes: &[u8],
        slot_hashes: &SlotHashes,
    ) -> Result<Vec<Pubkey>, AddressLookupError> {
        let active_addresses_len = self.get_active_addresses_len(current_slot, slot_hashes)?;
        let active_addresses = &self.addresses[0..active_addresses_len];
        indexes
            .iter()
            .map(|idx| active_addresses.get(*idx as usize).cloned())
            .collect::<Option<_>>()
            .ok_or(AddressLookupError::InvalidLookupIndex)
    }

    /// Serialize an address table including its addresses
    pub fn serialize_for_tests(self) -> Result<Vec<u8>, InstructionError> {
        let mut data = vec![0; LOOKUP_TABLE_META_SIZE];
        Self::overwrite_meta_data(&mut data, self.meta)?;
        self.addresses.iter().for_each(|address| {
            data.extend_from_slice(address.as_ref());
        });
        Ok(data)
    }

    /// Efficiently deserialize an address table without allocating
    /// for stored addresses.
    pub fn deserialize(data: &'a [u8]) -> Result<AddressLookupTable<'a>, InstructionError> {
        let program_state: ProgramState =
            bincode::deserialize(data).map_err(|_| InstructionError::InvalidAccountData)?;

        let meta = match program_state {
            ProgramState::LookupTable(meta) => Ok(meta),
            ProgramState::Uninitialized => Err(InstructionError::UninitializedAccount),
        }?;

        let raw_addresses_data = data.get(LOOKUP_TABLE_META_SIZE..).ok_or({
            // Should be impossible because table accounts must
            // always be LOOKUP_TABLE_META_SIZE in length
            InstructionError::InvalidAccountData
        })?;
        let addresses: &[Pubkey] = bytemuck::try_cast_slice(raw_addresses_data).map_err(|_| {
            // Should be impossible because raw address data
            // should be aligned and sized in multiples of 32 bytes
            InstructionError::InvalidAccountData
        })?;

        Ok(Self {
            meta,
            addresses: Cow::Borrowed(addresses),
        })
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::hash::Hash};

    impl AddressLookupTable<'_> {
        fn new_for_tests(meta: LookupTableMeta, num_addresses: usize) -> Self {
            let mut addresses = Vec::with_capacity(num_addresses);
            addresses.resize_with(num_addresses, Pubkey::new_unique);
            AddressLookupTable {
                meta,
                addresses: Cow::Owned(addresses),
            }
        }
    }

    impl LookupTableMeta {
        fn new_for_tests() -> Self {
            Self {
                authority: Some(Pubkey::new_unique()),
                ..LookupTableMeta::default()
            }
        }
    }

    #[test]
    fn test_lookup_table_meta_size() {
        let lookup_table = ProgramState::LookupTable(LookupTableMeta::new_for_tests());
        let meta_size = bincode::serialized_size(&lookup_table).unwrap();
        assert!(meta_size as usize <= LOOKUP_TABLE_META_SIZE);
        assert_eq!(meta_size as usize, 56);

        let lookup_table = ProgramState::LookupTable(LookupTableMeta::default());
        let meta_size = bincode::serialized_size(&lookup_table).unwrap();
        assert!(meta_size as usize <= LOOKUP_TABLE_META_SIZE);
        assert_eq!(meta_size as usize, 24);
    }

    #[test]
    fn test_lookup_table_meta_status() {
        let mut slot_hashes = SlotHashes::default();
        for slot in 1..=MAX_ENTRIES as Slot {
            slot_hashes.add(slot, Hash::new_unique());
        }

        let most_recent_slot = slot_hashes.first().unwrap().0;
        let least_recent_slot = slot_hashes.last().unwrap().0;
        assert!(least_recent_slot < most_recent_slot);

        // 10 was chosen because the current slot isn't necessarily the next
        // slot after the most recent block
        let current_slot = most_recent_slot + 10;

        let active_table = LookupTableMeta {
            deactivation_slot: Slot::MAX,
            ..LookupTableMeta::default()
        };

        let just_started_deactivating_table = LookupTableMeta {
            deactivation_slot: current_slot,
            ..LookupTableMeta::default()
        };

        let recently_started_deactivating_table = LookupTableMeta {
            deactivation_slot: most_recent_slot,
            ..LookupTableMeta::default()
        };

        let almost_deactivated_table = LookupTableMeta {
            deactivation_slot: least_recent_slot,
            ..LookupTableMeta::default()
        };

        let deactivated_table = LookupTableMeta {
            deactivation_slot: least_recent_slot - 1,
            ..LookupTableMeta::default()
        };

        assert_eq!(
            active_table.status(current_slot, &slot_hashes),
            LookupTableStatus::Activated
        );
        assert_eq!(
            just_started_deactivating_table.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivating {
                remaining_blocks: MAX_ENTRIES.saturating_add(1),
            }
        );
        assert_eq!(
            recently_started_deactivating_table.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivating {
                remaining_blocks: MAX_ENTRIES,
            }
        );
        assert_eq!(
            almost_deactivated_table.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivating {
                remaining_blocks: 1,
            }
        );
        assert_eq!(
            deactivated_table.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivated
        );
    }

    #[test]
    fn test_overwrite_meta_data() {
        let meta = LookupTableMeta::new_for_tests();
        let empty_table = ProgramState::LookupTable(meta.clone());
        let mut serialized_table_1 = bincode::serialize(&empty_table).unwrap();
        serialized_table_1.resize(LOOKUP_TABLE_META_SIZE, 0);

        let address_table = AddressLookupTable::new_for_tests(meta, 0);
        let mut serialized_table_2 = vec![0; LOOKUP_TABLE_META_SIZE];
        AddressLookupTable::overwrite_meta_data(&mut serialized_table_2, address_table.meta)
            .unwrap();

        assert_eq!(serialized_table_1, serialized_table_2);
    }

    #[test]
    fn test_deserialize() {
        assert_eq!(
            AddressLookupTable::deserialize(&[]).err(),
            Some(InstructionError::InvalidAccountData),
        );

        assert_eq!(
            AddressLookupTable::deserialize(&[0u8; LOOKUP_TABLE_META_SIZE]).err(),
            Some(InstructionError::UninitializedAccount),
        );

        fn test_case(num_addresses: usize) {
            let lookup_table_meta = LookupTableMeta::new_for_tests();
            let address_table = AddressLookupTable::new_for_tests(lookup_table_meta, num_addresses);
            let address_table_data =
                AddressLookupTable::serialize_for_tests(address_table.clone()).unwrap();
            assert_eq!(
                AddressLookupTable::deserialize(&address_table_data).unwrap(),
                address_table,
            );
        }

        for case in [0, 1, 10, 255, 256] {
            test_case(case);
        }
    }

    #[test]
    fn test_lookup_from_empty_table() {
        let lookup_table = AddressLookupTable {
            meta: LookupTableMeta::default(),
            addresses: Cow::Owned(vec![]),
        };

        assert_eq!(
            lookup_table.lookup(0, &[], &SlotHashes::default()),
            Ok(vec![])
        );
        assert_eq!(
            lookup_table.lookup(0, &[0], &SlotHashes::default()),
            Err(AddressLookupError::InvalidLookupIndex)
        );
    }

    #[test]
    fn test_lookup_from_deactivating_table() {
        let current_slot = 1;
        let slot_hashes = SlotHashes::default();
        let addresses = vec![Pubkey::new_unique()];
        let lookup_table = AddressLookupTable {
            meta: LookupTableMeta {
                deactivation_slot: current_slot,
                last_extended_slot: current_slot - 1,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses.clone()),
        };

        assert_eq!(
            lookup_table.meta.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivating {
                remaining_blocks: MAX_ENTRIES + 1
            }
        );

        assert_eq!(
            lookup_table.lookup(current_slot, &[0], &slot_hashes),
            Ok(vec![addresses[0]]),
        );
    }

    #[test]
    fn test_lookup_from_deactivated_table() {
        let current_slot = 1;
        let slot_hashes = SlotHashes::default();
        let lookup_table = AddressLookupTable {
            meta: LookupTableMeta {
                deactivation_slot: current_slot - 1,
                last_extended_slot: current_slot - 1,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(vec![]),
        };

        assert_eq!(
            lookup_table.meta.status(current_slot, &slot_hashes),
            LookupTableStatus::Deactivated
        );
        assert_eq!(
            lookup_table.lookup(current_slot, &[0], &slot_hashes),
            Err(AddressLookupError::LookupTableAccountNotFound)
        );
    }

    #[test]
    fn test_lookup_from_table_extended_in_current_slot() {
        let current_slot = 0;
        let addresses: Vec<_> = (0..2).map(|_| Pubkey::new_unique()).collect();
        let lookup_table = AddressLookupTable {
            meta: LookupTableMeta {
                last_extended_slot: current_slot,
                last_extended_slot_start_index: 1,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses.clone()),
        };

        assert_eq!(
            lookup_table.lookup(current_slot, &[0], &SlotHashes::default()),
            Ok(vec![addresses[0]])
        );
        assert_eq!(
            lookup_table.lookup(current_slot, &[1], &SlotHashes::default()),
            Err(AddressLookupError::InvalidLookupIndex),
        );
    }

    #[test]
    fn test_lookup_from_table_extended_in_previous_slot() {
        let current_slot = 1;
        let addresses: Vec<_> = (0..10).map(|_| Pubkey::new_unique()).collect();
        let lookup_table = AddressLookupTable {
            meta: LookupTableMeta {
                last_extended_slot: current_slot - 1,
                last_extended_slot_start_index: 1,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses.clone()),
        };

        assert_eq!(
            lookup_table.lookup(current_slot, &[0, 3, 1, 5], &SlotHashes::default()),
            Ok(vec![addresses[0], addresses[3], addresses[1], addresses[5]])
        );
        assert_eq!(
            lookup_table.lookup(current_slot, &[10], &SlotHashes::default()),
            Err(AddressLookupError::InvalidLookupIndex),
        );
    }
}
