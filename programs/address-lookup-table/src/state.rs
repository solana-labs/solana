use {
    serde::{Deserialize, Serialize},
    solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample},
    solana_sdk::{clock::Slot, instruction::InstructionError, pubkey::Pubkey},
    std::borrow::Cow,
};

/// The maximum number of addresses that a lookup table can hold
pub const LOOKUP_TABLE_MAX_ADDRESSES: usize = 256;

/// The serialized size of lookup table metadata
pub const LOOKUP_TABLE_META_SIZE: usize = 56;

/// Program account states
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, AbiExample, AbiEnumVisitor)]
#[allow(clippy::large_enum_variant)]
pub enum ProgramState {
    /// Account is not initialized.
    Uninitialized,
    /// Initialized `LookupTable` account.
    LookupTable(LookupTableMeta),
}

/// Address lookup table metadata
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, AbiExample)]
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
}

#[derive(Debug, PartialEq, Clone, AbiExample)]
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

    /// Serialize an address table including its addresses
    pub fn serialize_for_tests(self, data: &mut Vec<u8>) -> Result<(), InstructionError> {
        data.resize(LOOKUP_TABLE_META_SIZE, 0);
        Self::overwrite_meta_data(data, self.meta)?;
        self.addresses.iter().for_each(|address| {
            data.extend_from_slice(address.as_ref());
        });
        Ok(())
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
    use super::*;

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
    fn test_overwrite_meta_data() {
        let meta = LookupTableMeta::new_for_tests();
        let empty_table = ProgramState::LookupTable(meta.clone());
        let mut serialized_table_1 = bincode::serialize(&empty_table).unwrap();
        serialized_table_1.resize(LOOKUP_TABLE_META_SIZE, 0);

        let address_table = AddressLookupTable::new_for_tests(meta, 0);
        let mut serialized_table_2 = Vec::new();
        serialized_table_2.resize(LOOKUP_TABLE_META_SIZE, 0);
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
            let mut address_table_data = Vec::new();
            AddressLookupTable::serialize_for_tests(address_table.clone(), &mut address_table_data)
                .unwrap();
            assert_eq!(
                AddressLookupTable::deserialize(&address_table_data).unwrap(),
                address_table,
            );
        }

        for case in [0, 1, 10, 255, 256] {
            test_case(case);
        }
    }
}
