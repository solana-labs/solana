#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

use {
    serde::{Deserialize, Serialize},
    solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample},
    solana_sdk::{
        clock::Slot,
        declare_id,
        pubkey::{Pubkey, PUBKEY_BYTES},
    },
    thiserror::Error,
};

declare_id!("AddressMap111111111111111111111111111111111");

/// The byte offset where entry encoding begins
pub const ADDRESS_MAP_ENTRIES_START: usize = 54;

/// The maximum number of entries that can be stored in a single address map
/// account.
pub const MAX_ADDRESS_MAP_ENTRIES: u8 = u8::MAX;

/// Address map program account states
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, AbiExample, AbiEnumVisitor)]
#[allow(clippy::large_enum_variant)]
pub enum AddressMapState {
    /// Account is not initialized.
    Uninitialized,
    /// Initialized `AddressMap` account.
    Initialized(AddressMap),
}

/// Data structure of address map
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, AbiExample)]
pub struct AddressMap {
    /// Authority address which must sign for each modification.
    pub authority: Option<Pubkey>,
    /// Activation slot. Address map entries may not be modified once activated.
    pub activation_slot: Slot,
    /// Deactivation slot. Address map accounts cannot be closed unless they have been deactivated.
    pub deactivation_slot: Slot,
    /// Number of stored address entries. Limited by `MAX_ADDRESS_MAP_ENTRIES`.
    pub num_entries: u8,
    // Raw list of `num_entries` addresses follows this serialized structure in
    // the account's data, starting from `ADDRESS_MAP_ENTRIES_START`.
}

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum AddressMapError {
    /// Address map data must have room for exactly `num_entries` addresses and
    /// cannot have trailing bytes.
    #[error("Entries data size is invalid")]
    InvalidEntriesDataSize,
}

impl AddressMap {
    /// Attempt to deserialize the address entries stored in an address map
    /// account. Address map data must have room for exactly `num_entries`
    /// addresses.
    pub fn try_deserialize_entries(
        &self,
        serialized_map: &[u8],
    ) -> Result<Vec<Pubkey>, AddressMapError> {
        let entries_size = PUBKEY_BYTES.saturating_mul(self.num_entries as usize);
        if serialized_map.len() == ADDRESS_MAP_ENTRIES_START.saturating_add(entries_size) {
            Ok(serialized_map[ADDRESS_MAP_ENTRIES_START..]
                .chunks(32)
                .map(|entry_bytes| Pubkey::new(entry_bytes))
                .collect())
        } else {
            Err(AddressMapError::InvalidEntriesDataSize)
        }
    }

    /// Attempt to serialize the address map along with its entries. The length
    /// of `entries` must equal `num_entries` addresses.
    pub fn try_serialize_with_entries(
        &self,
        entries: &[Pubkey],
    ) -> Result<Vec<u8>, AddressMapError> {
        if usize::from(self.num_entries) != entries.len() {
            return Err(AddressMapError::InvalidEntriesDataSize);
        }

        Ok(self.serialize_with_entries_unchecked(entries))
    }

    /// Serialize the address map along with its entries. Used for tests only.
    #[doc(hidden)]
    pub fn serialize_with_entries_unchecked(&self, entries: &[Pubkey]) -> Vec<u8> {
        let mut data = bincode::serialize(&(1u32, &self)).unwrap();
        data.resize(ADDRESS_MAP_ENTRIES_START, 0);
        for entry in entries {
            data.extend_from_slice(entry.as_ref());
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_address_map(num_entries: u8) -> AddressMap {
        AddressMap {
            authority: Some(Pubkey::new_unique()),
            activation_slot: Slot::MAX,
            deactivation_slot: Slot::MAX,
            num_entries,
        }
    }

    #[test]
    fn test_map_entries_start() {
        let address_map = AddressMapState::Initialized(create_test_address_map(0));
        let map_meta_size = bincode::serialized_size(&address_map).unwrap();

        assert_eq!(map_meta_size as usize, ADDRESS_MAP_ENTRIES_START);
    }

    #[test]
    fn test_try_deserialize_entries() {
        for num_entries in [0, 1, 10, MAX_ADDRESS_MAP_ENTRIES] {
            let address_map = create_test_address_map(num_entries);
            let entries = {
                let mut vec = Vec::with_capacity(usize::from(num_entries));
                vec.resize_with(usize::from(num_entries), Pubkey::new_unique);
                vec
            };

            let serialized_map = address_map.try_serialize_with_entries(&entries).unwrap();
            assert_eq!(
                address_map.try_deserialize_entries(&serialized_map),
                Ok(entries),
            );
        }
    }

    #[test]
    fn test_try_deserialize_entries_trailing_byte() {
        let address_map = create_test_address_map(0);
        let mut serialized_map =
            bincode::serialize(&AddressMapState::Initialized(address_map.clone())).unwrap();
        serialized_map.resize(serialized_map.len() + 1, 0u8);

        assert_eq!(
            address_map.try_deserialize_entries(&serialized_map).err(),
            Some(AddressMapError::InvalidEntriesDataSize),
        );
    }

    #[test]
    fn test_try_deserialize_entries_too_small() {
        let address_map = create_test_address_map(1);
        let serialized_map =
            bincode::serialize(&AddressMapState::Initialized(address_map.clone())).unwrap();

        assert_eq!(
            address_map.try_deserialize_entries(&serialized_map).err(),
            Some(AddressMapError::InvalidEntriesDataSize),
        );
    }

    #[test]
    fn test_try_serialize_with_entries_failure() {
        let address_map = create_test_address_map(0);
        assert!(address_map
            .try_serialize_with_entries(&[Pubkey::new_unique()])
            .is_err());
    }

    #[test]
    fn test_try_serialize_with_entries() {
        let address_map = create_test_address_map(1);
        let entries = vec![Pubkey::new_unique()];
        let serialized_map = address_map.try_serialize_with_entries(&entries).unwrap();

        assert_eq!(
            address_map.try_deserialize_entries(&serialized_map),
            Ok(entries)
        );
        assert_eq!(
            bincode::deserialize::<AddressMapState>(&serialized_map).ok(),
            Some(AddressMapState::Initialized(address_map))
        );
    }
}
