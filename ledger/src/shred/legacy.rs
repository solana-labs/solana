use {
    crate::shred::{
        traits::{Shred, ShredCode as _, ShredData as _},
        CodingShredHeader, DataShredHeader, Error, ShredCommonHeader, ShredFlags, ShredVariant,
        MAX_DATA_SHREDS_PER_FEC_BLOCK, MAX_DATA_SHREDS_PER_SLOT, SHRED_DATA_OFFSET,
        SHRED_PAYLOAD_SIZE, SIZE_OF_CODING_SHRED_HEADERS, SIZE_OF_COMMON_SHRED_HEADER,
        SIZE_OF_DATA_SHRED_HEADER, SIZE_OF_DATA_SHRED_PAYLOAD, SIZE_OF_SIGNATURE,
    },
    solana_perf::packet::deserialize_from_with_limit,
    solana_sdk::{clock::Slot, signature::Signature},
    static_assertions::const_assert_eq,
    std::{io::Cursor, ops::RangeInclusive},
};

// DataShredHeader.size is sum of common-shred-header, data-shred-header and
// data.len(). Broadcast stage may create zero length data shreds when the
// previous slot was interrupted:
// https://github.com/solana-labs/solana/blob/2d4defa47/core/src/broadcast_stage/standard_broadcast_run.rs#L79
const DATA_SHRED_SIZE_RANGE: RangeInclusive<usize> =
    SHRED_DATA_OFFSET..=SHRED_DATA_OFFSET + SIZE_OF_DATA_SHRED_PAYLOAD;
// SIZE_OF_CODING_SHRED_HEADERS bytes at the end of data shreds
// is never used and is not part of erasure coding.
const_assert_eq!(ENCODED_PAYLOAD_SIZE, 1139);
const ENCODED_PAYLOAD_SIZE: usize = SHRED_PAYLOAD_SIZE - SIZE_OF_CODING_SHRED_HEADERS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShredCode {
    common_header: ShredCommonHeader,
    coding_header: CodingShredHeader,
    payload: Vec<u8>,
}

macro_rules! impl_shred_common {
    () => {
        #[inline]
        fn common_header(&self) -> &ShredCommonHeader {
            &self.common_header
        }

        #[inline]
        fn payload(&self) -> &Vec<u8> {
            &self.payload
        }

        fn into_payload(self) -> Vec<u8> {
            self.payload
        }

        fn set_signature(&mut self, signature: Signature) {
            bincode::serialize_into(&mut self.payload[..], &signature).unwrap();
            self.common_header.signature = signature;
        }

        // Only for tests.
        fn set_index(&mut self, index: u32) {
            self.common_header.index = index;
            bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
        }

        // Only for tests.
        fn set_slot(&mut self, slot: Slot) {
            self.common_header.slot = slot;
            bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
        }
    };
}

impl Shred for ShredData {
    impl_shred_common!();

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        if common_header.shred_variant != ShredVariant::LegacyData {
            return Err(Error::InvalidShredVariant);
        }
        let data_header = deserialize_from_with_limit(&mut cursor)?;
        // see: https://github.com/solana-labs/solana/pull/16602
        payload.resize(SHRED_PAYLOAD_SIZE, 0u8);
        let shred = Self {
            common_header,
            data_header,
            payload,
        };
        shred.sanitize().map(|_| shred)
    }

    fn erasure_shard_index(&self) -> Option<usize> {
        let fec_set_index = self.common_header.fec_set_index;
        let index = self.common_header.index.checked_sub(fec_set_index)?;
        usize::try_from(index).ok()
    }

    fn erasure_shard(self) -> Result<Vec<u8>, Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let mut shard = self.payload;
        shard.resize(ENCODED_PAYLOAD_SIZE, 0u8);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        Ok(&self.payload[..ENCODED_PAYLOAD_SIZE])
    }

    fn resize_stored_shred(mut shred: Vec<u8>) -> Result<Vec<u8>, Error> {
        // TODO: assert that this is the right type!
        if !(SHRED_DATA_OFFSET..SHRED_PAYLOAD_SIZE).contains(&shred.len()) {
            return Err(Error::InvalidPayloadSize(shred.len()));
        }
        shred.resize(SHRED_PAYLOAD_SIZE, 0u8);
        Ok(shred)
    }

    fn sanitize(&self) -> Result<(), Error> {
        if self.payload().len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        if self.erasure_shard_index().is_none() {
            let headers = Box::new((self.common_header, self.data_header));
            return Err(Error::InvalidErasureShardIndex(headers));
        }
        if self.common_header.index as usize >= MAX_DATA_SHREDS_PER_SLOT {
            return Err(Error::InvalidDataShredIndex(self.common_header.index));
        }
        let _parent = self.parent()?;
        let size = usize::from(self.data_header.size);
        if size > self.payload.len() || !DATA_SHRED_SIZE_RANGE.contains(&size) {
            return Err(Error::InvalidDataSize {
                size: self.data_header.size,
                payload: self.payload.len(),
            });
        }
        let flags = self.data_header.flags;
        if flags.intersects(ShredFlags::LAST_SHRED_IN_SLOT)
            && !flags.contains(ShredFlags::DATA_COMPLETE_SHRED)
        {
            return Err(Error::InvalidShredFlags(self.data_header.flags.bits()));
        }
        Ok(())
    }

    fn signed_payload(&self) -> &[u8] {
        debug_assert_eq!(self.payload.len(), SHRED_PAYLOAD_SIZE);
        &self.payload[SIZE_OF_SIGNATURE..]
    }
}

impl Shred for ShredCode {
    impl_shred_common!();

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        if common_header.shred_variant != ShredVariant::LegacyCode {
            return Err(Error::InvalidShredVariant);
        }
        let coding_header = deserialize_from_with_limit(&mut cursor)?;
        // see: https://github.com/solana-labs/solana/pull/10109
        payload.truncate(SHRED_PAYLOAD_SIZE);
        let shred = Self {
            common_header,
            coding_header,
            payload,
        };
        shred.sanitize().map(|_| shred)
    }

    fn erasure_shard_index(&self) -> Option<usize> {
        // Assert that the last shred index in the erasure set does not
        // overshoot u32.
        self.common_header.fec_set_index.checked_add(u32::from(
            self.coding_header.num_data_shreds.checked_sub(1)?,
        ))?;
        self.first_coding_index()?.checked_add(u32::from(
            self.coding_header.num_coding_shreds.checked_sub(1)?,
        ))?;
        let num_data_shreds = usize::from(self.coding_header.num_data_shreds);
        let num_coding_shreds = usize::from(self.coding_header.num_coding_shreds);
        let position = usize::from(self.coding_header.position);
        let fec_set_size = num_data_shreds.checked_add(num_coding_shreds)?;
        let index = position.checked_add(num_data_shreds)?;
        (index < fec_set_size).then(|| index)
    }

    fn erasure_shard(self) -> Result<Vec<u8>, Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let mut shard = self.payload;
        // SIZE_OF_CODING_SHRED_HEADERS bytes at the beginning of the
        // coding shreds contains the header and is not part of erasure
        // coding.
        shard.drain(..SIZE_OF_CODING_SHRED_HEADERS);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        Ok(&self.payload[SIZE_OF_CODING_SHRED_HEADERS..])
    }

    fn resize_stored_shred(shred: Vec<u8>) -> Result<Vec<u8>, Error> {
        if shred.len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(shred.len()));
        }
        Ok(shred)
    }

    fn sanitize(&self) -> Result<(), Error> {
        if self.payload().len() != SHRED_PAYLOAD_SIZE {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        if self.erasure_shard_index().is_none() {
            let headers = Box::new((self.common_header, self.coding_header));
            return Err(Error::InvalidErasureShardIndex(headers));
        }
        let num_coding_shreds = u32::from(self.coding_header.num_coding_shreds);
        if num_coding_shreds > 8 * MAX_DATA_SHREDS_PER_FEC_BLOCK {
            return Err(Error::InvalidNumCodingShreds(
                self.coding_header.num_coding_shreds,
            ));
        }
        Ok(())
    }

    fn signed_payload(&self) -> &[u8] {
        debug_assert_eq!(self.payload.len(), SHRED_PAYLOAD_SIZE);
        &self.payload[SIZE_OF_SIGNATURE..]
    }
}

impl super::traits::ShredData for ShredData {
    #[inline]
    fn data_header(&self) -> &DataShredHeader {
        &self.data_header
    }

    fn data(&self) -> Result<&[u8], Error> {
        let size = usize::from(self.data_header.size);
        if size > self.payload.len() || !DATA_SHRED_SIZE_RANGE.contains(&size) {
            return Err(Error::InvalidDataSize {
                size: self.data_header.size,
                payload: self.payload.len(),
            });
        }
        Ok(&self.payload[SHRED_DATA_OFFSET..size])
    }

    fn bytes_to_store(&self) -> &[u8] {
        // Payload will be padded out to SHRED_PAYLOAD_SIZE.
        // But only need to store the bytes within data_header.size.
        &self.payload[..self.data_header.size as usize]
    }

    // Only for tests.
    fn set_last_in_slot(&mut self) {
        self.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT;
        let buffer = &mut self.payload[SIZE_OF_COMMON_SHRED_HEADER..];
        bincode::serialize_into(buffer, &self.data_header).unwrap();
    }
}

impl super::traits::ShredCode for ShredCode {
    #[inline]
    fn coding_header(&self) -> &CodingShredHeader {
        &self.coding_header
    }
}

impl ShredData {
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
        let mut payload = vec![0; SHRED_PAYLOAD_SIZE];
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: ShredVariant::LegacyData,
            slot,
            index,
            version,
            fec_set_index,
        };
        let size = (data.len() + SIZE_OF_DATA_SHRED_HEADER + SIZE_OF_COMMON_SHRED_HEADER) as u16;
        let flags = flags
            | unsafe {
                ShredFlags::from_bits_unchecked(
                    ShredFlags::SHRED_TICK_REFERENCE_MASK
                        .bits()
                        .min(reference_tick),
                )
            };
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
        debug_assert_eq!(offset, SHRED_DATA_OFFSET);
        payload[offset..offset + data.len()].copy_from_slice(data);
        Self {
            common_header,
            data_header,
            payload,
        }
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
        let mut payload = vec![0; SHRED_PAYLOAD_SIZE];
        let mut cursor = Cursor::new(&mut payload[..]);
        bincode::serialize_into(&mut cursor, &common_header).unwrap();
        bincode::serialize_into(&mut cursor, &coding_header).unwrap();
        // Tests may have an empty parity_shard.
        if !parity_shard.is_empty() {
            let offset = cursor.position() as usize;
            debug_assert_eq!(offset, SIZE_OF_CODING_SHRED_HEADERS);
            payload[offset..].copy_from_slice(parity_shard);
        }
        Self {
            common_header,
            coding_header,
            payload,
        }
    }

    // Returns true if the erasure coding of the two shreds mismatch.
    pub(super) fn erasure_mismatch(&self, other: &ShredCode) -> bool {
        let CodingShredHeader {
            num_data_shreds,
            num_coding_shreds,
            position: _,
        } = self.coding_header;
        num_coding_shreds != other.coding_header.num_coding_shreds
            || num_data_shreds != other.coding_header.num_data_shreds
            || self.first_coding_index() != other.first_coding_index()
    }
}

#[cfg(test)]
mod test {
    use {super::*, matches::assert_matches};

    #[test]
    fn test_sanitize_data_shred() {
        let data = [0xa5u8; SIZE_OF_DATA_SHRED_PAYLOAD];
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
            assert_matches!(shred.sanitize(), Err(Error::InvalidDataShredIndex(32768)));
        }
        {
            let mut shred = shred.clone();
            shred.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT;
            assert_matches!(shred.sanitize(), Ok(()));
            shred.data_header.flags &= !ShredFlags::DATA_COMPLETE_SHRED;
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
            shred.set_index(index as u32);
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
            shred.common_header.fec_set_index = std::u32::MAX - 1;
            shred.coding_header.num_data_shreds = 2;
            shred.coding_header.num_coding_shreds = 4;
            shred.coding_header.position = 1;
            shred.common_header.index = std::u32::MAX - 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );

            shred.coding_header.num_coding_shreds = 2000;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidErasureShardIndex { .. })
            );

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
