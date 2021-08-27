use {
    serde::{Deserialize, Serialize},
    solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample},
    solana_sdk::{
        clock::Slot,
        pubkey::{Pubkey, PUBKEY_BYTES},
    },
    thiserror::Error,
};

/// The byte offset where entry encoding begins
pub const ADDRESS_MAP_ENTRIES_START: usize = 54;

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
    /// Activation slot. Address map entries may not be modified once activated.
    pub activation_slot: Slot,
    /// Deactivation slot. Address map accounts cannot be closed unless they have been deactivated.
    pub deactivation_slot: Slot,
    /// Number of stored address entries. Limited by `MAX_ADDRESS_MAP_ENTRIES`.
    pub num_entries: u8,
    /// Authority address which must sign for each modification.
    pub authority: Option<Pubkey>,
    // Raw list of `num_entries` addresses follows this serialized structure in
    // the account's data, starting from `ADDRESS_MAP_ENTRIES_START`.
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum SerializationError {
    /// Bincode serialization failed
    #[error("Bincode serialization error")]
    BincodeError,

    /// Address map is uninitialized
    #[error("Address map must be initialized")]
    Uninitialized,

    /// Address map data must have room for `num_entries` addresses.
    #[error("Address map data too small for {0} address entries")]
    InvalidNumEntries(u8),
}

impl From<bincode::Error> for SerializationError {
    fn from(_: bincode::Error) -> Self {
        Self::BincodeError
    }
}

impl AddressMap {
    /// Attempt to deserialize an initialized address map account.
    pub fn deserialize(serialized_map: &[u8]) -> Result<AddressMap, SerializationError> {
        let state = bincode::deserialize::<AddressMapState>(serialized_map)?;
        match state {
            AddressMapState::Initialized(address_map) => Ok(address_map),
            AddressMapState::Uninitialized => Err(SerializationError::Uninitialized),
        }
    }

    /// Attempt to deserialize the address entries stored in an address map
    /// account. Address map data must have room for `num_entries` addresses.
    pub fn deserialize_entries(
        &self,
        serialized_map: &[u8],
    ) -> Result<Vec<Pubkey>, SerializationError> {
        let entries_size = PUBKEY_BYTES.saturating_mul(usize::from(self.num_entries));
        let entries_end = ADDRESS_MAP_ENTRIES_START.saturating_add(entries_size);
        if serialized_map.len() >= entries_end {
            Ok(serialized_map[ADDRESS_MAP_ENTRIES_START..entries_end]
                .chunks(PUBKEY_BYTES)
                .map(|entry_bytes| Pubkey::new(entry_bytes))
                .collect())
        } else {
            Err(SerializationError::InvalidNumEntries(self.num_entries))
        }
    }

    /// Serialize the address map along with its entries.
    pub fn serialize_with_entries(&self, entries: &[Pubkey]) -> Vec<u8> {
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
    fn test_deserialize() {
        assert_eq!(
            AddressMap::deserialize(&[]).err(),
            Some(SerializationError::BincodeError),
        );

        assert_eq!(
            AddressMap::deserialize(&[0u8; ADDRESS_MAP_ENTRIES_START]).err(),
            Some(SerializationError::Uninitialized),
        );

        let address_map = create_test_address_map(1);
        let address_map_data = address_map.serialize_with_entries(&[]);
        assert_eq!(AddressMap::deserialize(&address_map_data), Ok(address_map),);
    }

    #[test]
    fn test_deserialize_entries() {
        for num_entries in [0, 1, 10, u8::MAX] {
            let address_map = create_test_address_map(num_entries);
            let entries = {
                let mut vec = Vec::with_capacity(usize::from(num_entries));
                vec.resize_with(usize::from(num_entries), Pubkey::new_unique);
                vec
            };

            let serialized_map = address_map.serialize_with_entries(&entries);
            assert_eq!(
                address_map.deserialize_entries(&serialized_map),
                Ok(entries),
            );
        }
    }

    #[test]
    fn test_deserialize_entries_trailing_bytes() {
        let address_map = create_test_address_map(0);
        let mut serialized_map = address_map.serialize_with_entries(&[]);
        serialized_map.resize(serialized_map.len() + 1, 0u8);

        assert!(address_map.deserialize_entries(&serialized_map).is_ok());
    }

    #[test]
    fn test_deserialize_entries_too_small() {
        let address_map = create_test_address_map(1);
        let serialized_map = address_map.serialize_with_entries(&[]);

        assert_eq!(
            address_map.deserialize_entries(&serialized_map).err(),
            Some(SerializationError::InvalidNumEntries(1)),
        );
    }

    #[test]
    fn test_serialize_with_entries() {
        let address_map = create_test_address_map(1);
        let entries = vec![Pubkey::new_unique()];
        let serialized_map = address_map.serialize_with_entries(&entries);

        assert_eq!(
            address_map.deserialize_entries(&serialized_map),
            Ok(entries)
        );
        assert_eq!(
            bincode::deserialize::<AddressMapState>(&serialized_map).ok(),
            Some(AddressMapState::Initialized(address_map))
        );
    }
}
