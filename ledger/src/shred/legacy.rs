use {
    crate::shred::{
        common::impl_shred_common,
        shred_code, shred_data,
        traits::{Shred, ShredCode as ShredCodeTrait, ShredData as ShredDataTrait},
        CodingShredHeader, DataShredHeader, Error, ShredCommonHeader, ShredFlags, ShredVariant,
        SIZE_OF_CODING_SHRED_HEADERS, SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADERS,
        SIZE_OF_SIGNATURE,
    },
    solana_perf::packet::deserialize_from_with_limit,
    solana_sdk::{clock::Slot, signature::Signature},
    static_assertions::const_assert_eq,
    std::{io::Cursor, ops::Range},
};

// All payload including any zero paddings are signed.
// Code and data shreds have the same payload size.
pub(super) const SIGNED_MESSAGE_OFFSETS: Range<usize> =
    SIZE_OF_SIGNATURE..ShredData::SIZE_OF_PAYLOAD;
const_assert_eq!(ShredData::SIZE_OF_PAYLOAD, ShredCode::SIZE_OF_PAYLOAD);
const_assert_eq!(ShredData::SIZE_OF_PAYLOAD, 1228);
const_assert_eq!(ShredData::CAPACITY, 1051);

// ShredCode::SIZE_OF_HEADERS bytes at the end of data shreds
// is never used and is not part of erasure coding.
const_assert_eq!(SIZE_OF_ERASURE_ENCODED_SLICE, 1139);
pub(super) const SIZE_OF_ERASURE_ENCODED_SLICE: usize =
    ShredCode::SIZE_OF_PAYLOAD - ShredCode::SIZE_OF_HEADERS;

// Layout: {common, data} headers | data | zero padding
// Everything up to ShredCode::SIZE_OF_HEADERS bytes at the end (which is part
// of zero padding) is erasure coded.
// All payload past signature, including the entirety of zero paddings, is
// signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    payload: Vec<u8>,
}

// Layout: {common, coding} headers | erasure coded shard
// All payload past signature is singed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredCode {
    common_header: ShredCommonHeader,
    coding_header: CodingShredHeader,
    payload: Vec<u8>,
}

impl<'a> Shred<'a> for ShredData {
    type SignedData = &'a [u8];

    impl_shred_common!();
    // Legacy data shreds are always zero padded and
    // the same size as coding shreds.
    const SIZE_OF_PAYLOAD: usize = shred_code::ShredCode::SIZE_OF_PAYLOAD;
    const SIZE_OF_HEADERS: usize = SIZE_OF_DATA_SHRED_HEADERS;

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        if common_header.shred_variant != ShredVariant::LegacyData {
            return Err(Error::InvalidShredVariant);
        }
        let data_header = deserialize_from_with_limit(&mut cursor)?;
        // Shreds stored to blockstore may have trailing zeros trimmed.
        // Repair packets have nonce at the end of packet payload; see:
        // https://github.com/solana-labs/solana/pull/10109
        // https://github.com/solana-labs/solana/pull/16602
        if payload.len() < Self::SIZE_OF_HEADERS {
            return Err(Error::InvalidPayloadSize(payload.len()));
        }
        payload.resize(Self::SIZE_OF_PAYLOAD, 0u8);
        let shred = Self {
            common_header,
            data_header,
            payload,
        };
        shred.sanitize().map(|_| shred)
    }

    fn erasure_shard_index(&self) -> Result<usize, Error> {
        shred_data::erasure_shard_index(self).ok_or_else(|| {
            let headers = Box::new((self.common_header, self.data_header));
            Error::InvalidErasureShardIndex(headers)
        })
    }

    fn erasure_shard(self) -> Result<Vec<u8>, Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let mut shard = self.payload;
        shard.truncate(SIZE_OF_ERASURE_ENCODED_SLICE);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        Ok(&self.payload[..SIZE_OF_ERASURE_ENCODED_SLICE])
    }

    fn sanitize(&self) -> Result<(), Error> {
        match self.common_header.shred_variant {
            ShredVariant::LegacyData => (),
            _ => return Err(Error::InvalidShredVariant),
        }
        shred_data::sanitize(self)
    }

    fn signed_data(&'a self) -> Result<Self::SignedData, Error> {
        debug_assert_eq!(self.payload.len(), Self::SIZE_OF_PAYLOAD);
        Ok(&self.payload[SIZE_OF_SIGNATURE..])
    }
}

impl<'a> Shred<'a> for ShredCode {
    type SignedData = &'a [u8];

    impl_shred_common!();
    const SIZE_OF_PAYLOAD: usize = shred_code::ShredCode::SIZE_OF_PAYLOAD;
    const SIZE_OF_HEADERS: usize = SIZE_OF_CODING_SHRED_HEADERS;

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        if common_header.shred_variant != ShredVariant::LegacyCode {
            return Err(Error::InvalidShredVariant);
        }
        let coding_header = deserialize_from_with_limit(&mut cursor)?;
        // Repair packets have nonce at the end of packet payload:
        // https://github.com/solana-labs/solana/pull/10109
        payload.truncate(Self::SIZE_OF_PAYLOAD);
        let shred = Self {
            common_header,
            coding_header,
            payload,
        };
        shred.sanitize().map(|_| shred)
    }

    fn erasure_shard_index(&self) -> Result<usize, Error> {
        shred_code::erasure_shard_index(self).ok_or_else(|| {
            let headers = Box::new((self.common_header, self.coding_header));
            Error::InvalidErasureShardIndex(headers)
        })
    }

    fn erasure_shard(self) -> Result<Vec<u8>, Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let mut shard = self.payload;
        // ShredCode::SIZE_OF_HEADERS bytes at the beginning of the coding
        // shreds contains the header and is not part of erasure coding.
        shard.drain(..Self::SIZE_OF_HEADERS);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        Ok(&self.payload[Self::SIZE_OF_HEADERS..])
    }

    fn sanitize(&self) -> Result<(), Error> {
        match self.common_header.shred_variant {
            ShredVariant::LegacyCode => (),
            _ => return Err(Error::InvalidShredVariant),
        }
        shred_code::sanitize(self)
    }

    fn signed_data(&'a self) -> Result<Self::SignedData, Error> {
        debug_assert_eq!(self.payload.len(), Self::SIZE_OF_PAYLOAD);
        Ok(&self.payload[SIZE_OF_SIGNATURE..])
    }
}

impl ShredDataTrait for ShredData {
    #[inline]
    fn data_header(&self) -> &DataShredHeader {
        &self.data_header
    }

    fn data(&self) -> Result<&[u8], Error> {
        let size = usize::from(self.data_header.size);
        #[allow(clippy::manual_range_contains)]
        if size > self.payload.len()
            || size < Self::SIZE_OF_HEADERS
            || size > Self::SIZE_OF_HEADERS + Self::CAPACITY
        {
            return Err(Error::InvalidDataSize {
                size: self.data_header.size,
                payload: self.payload.len(),
            });
        }
        Ok(&self.payload[Self::SIZE_OF_HEADERS..size])
    }
}

impl ShredCodeTrait for ShredCode {
    #[inline]
    fn coding_header(&self) -> &CodingShredHeader {
        &self.coding_header
    }
}

impl ShredData {
    // Maximum size of ledger data that can be embedded in a data-shred.
    pub(super) const CAPACITY: usize =
        Self::SIZE_OF_PAYLOAD - Self::SIZE_OF_HEADERS - ShredCode::SIZE_OF_HEADERS;

    pub(super) fn new_from_data(
        slot: Slot,
        index: u32,
        parent_offset: u16,
        data: &[u8],
        flags: ShredFlags,
        reference_tick: u8,
        version: u16,
        fec_set_index: u32,
    ) -> Self {
        let mut payload = vec![0; Self::SIZE_OF_PAYLOAD];
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: ShredVariant::LegacyData,
            slot,
            index,
            version,
            fec_set_index,
        };
        let size = (data.len() + Self::SIZE_OF_HEADERS) as u16;
        let flags = flags
            | ShredFlags::from_bits_retain(
                ShredFlags::SHRED_TICK_REFERENCE_MASK
                    .bits()
                    .min(reference_tick),
            );
        let data_header = DataShredHeader {
            parent_offset,
            flags,
            size,
        };
        let mut cursor = Cursor::new(&mut payload[..]);
        bincode::serialize_into(&mut cursor, &common_header).unwrap();
        bincode::serialize_into(&mut cursor, &data_header).unwrap();
        // TODO: Need to check if data is too large!
        let offset = cursor.position() as usize;
        debug_assert_eq!(offset, Self::SIZE_OF_HEADERS);
        payload[offset..offset + data.len()].copy_from_slice(data);
        Self {
            common_header,
            data_header,
            payload,
        }
    }

    pub(super) fn bytes_to_store(&self) -> &[u8] {
        // Payload will be padded out to Self::SIZE_OF_PAYLOAD.
        // But only need to store the bytes within data_header.size.
        &self.payload[..self.data_header.size as usize]
    }

    pub(super) fn resize_stored_shred(mut shred: Vec<u8>) -> Result<Vec<u8>, Error> {
        // Old shreds might have been extra zero padded.
        if !(Self::SIZE_OF_HEADERS..=Self::SIZE_OF_PAYLOAD).contains(&shred.len()) {
            return Err(Error::InvalidPayloadSize(shred.len()));
        }
        shred.resize(Self::SIZE_OF_PAYLOAD, 0u8);
        Ok(shred)
    }

    // Only for tests.
    pub(crate) fn set_last_in_slot(&mut self) {
        self.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT;
        let buffer = &mut self.payload[SIZE_OF_COMMON_SHRED_HEADER..];
        bincode::serialize_into(buffer, &self.data_header).unwrap();
    }
}

impl ShredCode {
    pub(super) fn new_from_parity_shard(
        slot: Slot,
        index: u32,
        parity_shard: &[u8],
        fec_set_index: u32,
        num_data_shreds: u16,
        num_coding_shreds: u16,
        position: u16,
        version: u16,
    ) -> Self {
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: ShredVariant::LegacyCode,
            index,
            slot,
            version,
            fec_set_index,
        };
        let coding_header = CodingShredHeader {
            num_data_shreds,
            num_coding_shreds,
            position,
        };
        let mut payload = vec![0; Self::SIZE_OF_PAYLOAD];
        let mut cursor = Cursor::new(&mut payload[..]);
        bincode::serialize_into(&mut cursor, &common_header).unwrap();
        bincode::serialize_into(&mut cursor, &coding_header).unwrap();
        // Tests may have an empty parity_shard.
        if !parity_shard.is_empty() {
            let offset = cursor.position() as usize;
            debug_assert_eq!(offset, Self::SIZE_OF_HEADERS);
            payload[offset..].copy_from_slice(parity_shard);
        }
        Self {
            common_header,
            coding_header,
            payload,
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::shred::{shred_code::MAX_CODE_SHREDS_PER_SLOT, ShredType, MAX_DATA_SHREDS_PER_SLOT},
        assert_matches::assert_matches,
    };

    #[test]
    fn test_sanitize_data_shred() {
        let data = [0xa5u8; ShredData::CAPACITY];
        let mut shred = ShredData::new_from_data(
            420, // slot
            19,  // index
            5,   // parent_offset
            &data,
            ShredFlags::DATA_COMPLETE_SHRED,
            3,  // reference_tick
            1,  // version
            16, // fec_set_index
        );
        assert_matches!(shred.sanitize(), Ok(()));
        // Corrupt shred by making it too large
        {
            let mut shred = shred.clone();
            shred.payload.push(10u8);
            assert_matches!(shred.sanitize(), Err(Error::InvalidPayloadSize(1229)));
        }
        {
            let mut shred = shred.clone();
            shred.data_header.size += 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1140,
                    payload: 1228,
                })
            );
        }
        {
            let mut shred = shred.clone();
            shred.data_header.size = 0;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 0,
                    payload: 1228,
                })
            );
        }
        {
            let mut shred = shred.clone();
            shred.common_header.index = MAX_DATA_SHREDS_PER_SLOT as u32;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidShredIndex(ShredType::Data, 32768))
            );
        }
        {
            let mut shred = shred.clone();
            shred.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT;
            assert_matches!(shred.sanitize(), Ok(()));
            shred.data_header.flags &=
                ShredFlags::from_bits_retain(!ShredFlags::DATA_COMPLETE_SHRED.bits());
            assert_matches!(shred.sanitize(), Err(Error::InvalidShredFlags(131u8)));
            shred.data_header.flags |= ShredFlags::SHRED_TICK_REFERENCE_MASK;
            assert_matches!(shred.sanitize(), Err(Error::InvalidShredFlags(191u8)));
        }
        {
            shred.data_header.size = shred.payload().len() as u16 + 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1229,
                    payload: 1228,
                })
            );
        }
    }

    #[test]
    fn test_sanitize_coding_shred() {
        let mut shred = ShredCode::new_from_parity_shard(
            1,   // slot
            12,  // index
            &[], // parity_shard
            11,  // fec_set_index
            11,  // num_data_shreds
            11,  // num_coding_shreds
            8,   // position
            0,   // version
        );
        assert_matches!(shred.sanitize(), Ok(()));
        // index < position is invalid.
        {
            let mut shred = shred.clone();
            let index = shred.common_header.index - shred.common_header.fec_set_index - 1;
            shred.set_index(index);
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        {
            let mut shred = shred.clone();
            shred.coding_header.num_coding_shreds = 0;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        {
            let mut shred = shred.clone();
            shred.common_header.index = MAX_CODE_SHREDS_PER_SLOT as u32;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidShredIndex(ShredType::Code, 32_768))
            );
        }
        // pos >= num_coding is invalid.
        {
            let mut shred = shred.clone();
            let num_coding_shreds = shred.common_header.index - shred.common_header.fec_set_index;
            shred.coding_header.num_coding_shreds = num_coding_shreds as u16;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );
        }
        // set_index with num_coding that would imply the last
        // shred has index > u32::MAX should fail.
        {
            let mut shred = shred.clone();
            shred.common_header.fec_set_index = MAX_DATA_SHREDS_PER_SLOT as u32 - 2;
            shred.coding_header.num_data_shreds = 3;
            shred.coding_header.num_coding_shreds = 4;
            shred.coding_header.position = 1;
            shred.common_header.index = MAX_DATA_SHREDS_PER_SLOT as u32 - 2;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );

            shred.coding_header.num_data_shreds = 2;
            shred.coding_header.num_coding_shreds = 2000;
            assert_matches!(shred.sanitize(), Err(Error::InvalidNumCodingShreds(2000)));

            // Decreasing the number of num_coding_shreds will put it within
            // the allowed limit.
            shred.coding_header.num_coding_shreds = 2;
            assert_matches!(shred.sanitize(), Ok(()));
        }
        {
            shred.coding_header.num_coding_shreds = u16::MAX;
            assert_matches!(shred.sanitize(), Err(Error::InvalidNumCodingShreds(65535)));
        }
    }
}
