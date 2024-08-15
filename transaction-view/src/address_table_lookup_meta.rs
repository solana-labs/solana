use {
    crate::{
        bytes::{
            advance_offset_for_array, advance_offset_for_type, check_remaining,
            optimized_read_compressed_u16, read_byte,
        },
        result::Result,
    },
    solana_sdk::{hash::Hash, packet::PACKET_DATA_SIZE, pubkey::Pubkey, signature::Signature},
};

// Each ATL has at least a Pubkey, one byte for the number of write indexes,
// and one byte for the number of read indexes. Additionally, for validity
// the ATL must have at least one write or read index giving a minimum size
// of 35 bytes.
const MIN_SIZED_ATL: usize = {
    core::mem::size_of::<Pubkey>() // account key
            + 1 // writable indexes length
            + 1 // readonly indexes length
            + 1 // single account (either write or read)
};

// A valid packet with ATLs has:
// 1. At least 1 signature
// 2. 1 message prefix byte
// 3. 3 bytes for the message header
// 4. 1 static account key
// 5. 1 recent blockhash
// 6. 1 byte for the number of instructions (0)
// 7. 1 byte for the number of ATLS
const MIN_SIZED_PACKET_WITH_ATLS: usize = {
    1 // signatures count
    + core::mem::size_of::<Signature>() // signature
    + 1 // message prefix
    + 3 // message header
    + 1 // static account keys count
    + core::mem::size_of::<Pubkey>() // static account key
    + core::mem::size_of::<Hash>() // recent blockhash
    + 1 // number of instructions
    + 1 // number of ATLS
};

/// The maximum number of ATLS that can fit in a valid packet.
const MAX_ATLS_PER_PACKET: usize = (PACKET_DATA_SIZE - MIN_SIZED_PACKET_WITH_ATLS) / MIN_SIZED_ATL;

/// Contains metadata about the address table lookups in a transaction packet.
pub struct AddressTableLookupMeta {
    /// The number of address table lookups in the transaction.
    pub(crate) num_address_table_lookup: u8,
    /// The offset to the first address table lookup in the transaction.
    pub(crate) offset: u16,
}

impl AddressTableLookupMeta {
    /// Get the number of address table lookups (ATL) and offset to the first.
    /// The offset will be updated to point to the first byte after the last
    /// ATL.
    /// This function will parse each ATL to ensure the data is well-formed,
    /// but will not cache data related to these ATLs.
    #[inline(always)]
    pub fn try_new(bytes: &[u8], offset: &mut usize) -> Result<Self> {
        // Maximum number of ATLs should be represented by a single byte,
        // thus the MSB should not be set.
        const _: () = assert!(MAX_ATLS_PER_PACKET & 0b1000_0000 == 0);
        let num_address_table_lookups = read_byte(bytes, offset)?;

        // Check that the remaining bytes are enough to hold the ATLs.
        check_remaining(
            bytes,
            *offset,
            MIN_SIZED_ATL.wrapping_mul(usize::from(num_address_table_lookups)),
        )?;

        // We know the offset does not exceed packet length, and our packet
        // length is less than u16::MAX, so we can safely cast to u16.
        let address_table_lookups_offset = *offset as u16;

        // The ATLs do not have a fixed size. So we must iterate over
        // each ATL to find the total size of the ATLs in the packet,
        // and check for any malformed ATLs or buffer overflows.
        for _index in 0..num_address_table_lookups {
            // Each ATL has 3 pieces:
            // 1. Address (Pubkey)
            // 2. write indexes ([u8])
            // 3. read indexes ([u8])

            // Advance offset for address of the lookup table.
            advance_offset_for_type::<Pubkey>(bytes, offset)?;

            // Read the number of write indexes, and then update the offset.
            let num_write_accounts = optimized_read_compressed_u16(bytes, offset)?;
            advance_offset_for_array::<u8>(bytes, offset, num_write_accounts)?;

            // Read the number of read indexes, and then update the offset.
            let num_read_accounts = optimized_read_compressed_u16(bytes, offset)?;
            advance_offset_for_array::<u8>(bytes, offset, num_read_accounts)?
        }

        Ok(Self {
            num_address_table_lookup: num_address_table_lookups,
            offset: address_table_lookups_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{message::v0::MessageAddressTableLookup, short_vec::ShortVec},
    };

    #[test]
    fn test_zero_atls() {
        let bytes = bincode::serialize(&ShortVec::<MessageAddressTableLookup>(vec![])).unwrap();
        let mut offset = 0;
        let meta = AddressTableLookupMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_address_table_lookup, 0);
        assert_eq!(meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_length_too_high() {
        let mut bytes = bincode::serialize(&ShortVec::<MessageAddressTableLookup>(vec![])).unwrap();
        let mut offset = 0;
        // modify the number of atls to be too high
        bytes[0] = 5;
        assert!(AddressTableLookupMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_single_atl() {
        let bytes = bincode::serialize(&ShortVec::<MessageAddressTableLookup>(vec![
            MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![4, 5, 6],
            },
        ]))
        .unwrap();
        let mut offset = 0;
        let meta = AddressTableLookupMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_address_table_lookup, 1);
        assert_eq!(meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_multiple_atls() {
        let bytes = bincode::serialize(&ShortVec::<MessageAddressTableLookup>(vec![
            MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![4, 5, 6],
            },
            MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![4, 5, 6],
            },
        ]))
        .unwrap();
        let mut offset = 0;
        let meta = AddressTableLookupMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_address_table_lookup, 2);
        assert_eq!(meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_invalid_writable_indexes_vec() {
        let mut bytes = bincode::serialize(&ShortVec(vec![MessageAddressTableLookup {
            account_key: Pubkey::new_unique(),
            writable_indexes: vec![1, 2, 3],
            readonly_indexes: vec![4, 5, 6],
        }]))
        .unwrap();

        // modify the number of accounts to be too high
        bytes[33] = 127;

        let mut offset = 0;
        assert!(AddressTableLookupMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_invalid_readonly_indexes_vec() {
        let mut bytes = bincode::serialize(&ShortVec(vec![MessageAddressTableLookup {
            account_key: Pubkey::new_unique(),
            writable_indexes: vec![1, 2, 3],
            readonly_indexes: vec![4, 5, 6],
        }]))
        .unwrap();

        // modify the number of accounts to be too high
        bytes[37] = 127;

        let mut offset = 0;
        assert!(AddressTableLookupMeta::try_new(&bytes, &mut offset).is_err());
    }
}
