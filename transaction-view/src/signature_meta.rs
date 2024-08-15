use {
    crate::{
        bytes::{advance_offset_for_array, read_byte},
        result::{Result, TransactionParsingError},
    },
    solana_sdk::{packet::PACKET_DATA_SIZE, pubkey::Pubkey, signature::Signature},
};

// The packet has a maximum length of 1232 bytes.
// Each signature must be paired with a unique static pubkey, so each
// signature really requires 96 bytes. This means the maximum number of
// signatures in a **valid** transaction packet is 12.
// In our u16 encoding scheme, 12 would be encoded as a single byte.
// Rather than using the u16 decoding, we can simply read the byte and
// verify that the MSB is not set.
const MAX_SIGNATURES_PER_PACKET: u8 =
    (PACKET_DATA_SIZE / (core::mem::size_of::<Signature>() + core::mem::size_of::<Pubkey>())) as u8;

/// Meta data for accessing transaction-level signatures in a transaction view.
pub(crate) struct SignatureMeta {
    /// The number of signatures in the transaction.
    pub(crate) num_signatures: u8,
    /// Offset to the first signature in the transaction packet.
    pub(crate) offset: u16,
}

impl SignatureMeta {
    /// Get the number of signatures and the offset to the first signature in
    /// the transaction packet, starting at the given `offset`.
    #[inline(always)]
    pub(crate) fn try_new(bytes: &[u8], offset: &mut usize) -> Result<Self> {
        // Maximum number of signatures should be represented by a single byte,
        // thus the MSB should not be set.
        const _: () = assert!(MAX_SIGNATURES_PER_PACKET & 0b1000_0000 == 0);

        let num_signatures = read_byte(bytes, offset)?;
        if num_signatures == 0 || num_signatures > MAX_SIGNATURES_PER_PACKET {
            return Err(TransactionParsingError);
        }

        let signature_offset = *offset as u16;
        advance_offset_for_array::<Signature>(bytes, offset, u16::from(num_signatures))?;

        Ok(Self {
            num_signatures,
            offset: signature_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::short_vec::ShortVec};

    #[test]
    fn test_zero_signatures() {
        let bytes = bincode::serialize(&ShortVec(Vec::<Signature>::new())).unwrap();
        let mut offset = 0;
        assert!(SignatureMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_one_signature() {
        let bytes = bincode::serialize(&ShortVec(vec![Signature::default()])).unwrap();
        let mut offset = 0;
        let meta = SignatureMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_signatures, 1);
        assert_eq!(meta.offset, 1);
        assert_eq!(offset, 1 + core::mem::size_of::<Signature>());
    }

    #[test]
    fn test_max_signatures() {
        let signatures = vec![Signature::default(); usize::from(MAX_SIGNATURES_PER_PACKET)];
        let bytes = bincode::serialize(&ShortVec(signatures)).unwrap();
        let mut offset = 0;
        let meta = SignatureMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_signatures, 12);
        assert_eq!(meta.offset, 1);
        assert_eq!(offset, 1 + 12 * core::mem::size_of::<Signature>());
    }

    #[test]
    fn test_non_zero_offset() {
        let mut bytes = bincode::serialize(&ShortVec(vec![Signature::default()])).unwrap();
        bytes.insert(0, 0); // Insert a byte at the beginning of the packet.
        let mut offset = 1; // Start at the second byte.
        let meta = SignatureMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(meta.num_signatures, 1);
        assert_eq!(meta.offset, 2);
        assert_eq!(offset, 2 + core::mem::size_of::<Signature>());
    }

    #[test]
    fn test_too_many_signatures() {
        let signatures = vec![Signature::default(); usize::from(MAX_SIGNATURES_PER_PACKET) + 1];
        let bytes = bincode::serialize(&ShortVec(signatures)).unwrap();
        let mut offset = 0;
        assert!(SignatureMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_u16_max_signatures() {
        let signatures = vec![Signature::default(); u16::MAX as usize];
        let bytes = bincode::serialize(&ShortVec(signatures)).unwrap();
        let mut offset = 0;
        assert!(SignatureMeta::try_new(&bytes, &mut offset).is_err());
    }
}
