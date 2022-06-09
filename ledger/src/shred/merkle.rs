use {
    crate::shred::{
        common::impl_shred_common,
        shred_code, shred_data,
        traits::{Shred, ShredCode as ShredCodeTrait, ShredData as ShredDataTrait},
        CodingShredHeader, DataShredHeader, Error, ShredCommonHeader, ShredFlags, ShredVariant,
        SIZE_OF_CODING_SHRED_HEADERS, SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADERS,
        SIZE_OF_SIGNATURE,
    },
    solana_perf::{
        packet::deserialize_from_with_limit,
        turbine_merkle::{
            TurbineMerkleHash, TurbineMerkleProof, TURBINE_MERKLE_HASH_BYTES,
            TURBINE_MERKLE_ROOT_BYTES,
        },
    },
    solana_sdk::{clock::Slot, signature::Signature},
    static_assertions::const_assert_eq,
    std::{
        io::{Cursor, Seek, SeekFrom},
        ops::Range,
    },
};

const_assert_eq!(ShredData::SIZE_OF_PAYLOAD, 1203);

// Layout: {common, data} headers | data buffer | merkle branch
// The slice past signature and before merkle branch is erasure coded.
// Same slice is hashed to generate merkle tree.
// The root of merkle tree is signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    payload: Vec<u8>,
}

// Layout: {common, coding} headers | erasure coded shard | merkle branch
// The slice past signature and before merkle branch is hashed to generate
// merkle tree. The root of merkle tree is signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredCode {
    common_header: ShredCommonHeader,
    coding_header: CodingShredHeader,
    payload: Vec<u8>,
}

impl ShredData {
    // proof_size is the number of proof entries in the merkle tree branch.
    fn proof_size(&self) -> Result<u8, Error> {
        match self.common_header.shred_variant {
            ShredVariant::MerkleData(proof_size) => Ok(proof_size),
            _ => Err(Error::InvalidShredVariant),
        }
    }

    // Maximum size of ledger data that can be embedded in a data-shred.
    // Also equal to:
    //   ShredCode::size_of_erasure_encoded_slice(proof_size).unwrap()
    //       - SIZE_OF_DATA_SHRED_HEADERS
    //       + SIZE_OF_SIGNATURE
    pub(super) fn capacity(proof_size: u8) -> Result<usize, Error> {
        Self::SIZE_OF_PAYLOAD
            .checked_sub(
                SIZE_OF_DATA_SHRED_HEADERS
                    + TURBINE_MERKLE_ROOT_BYTES
                    + usize::from(proof_size) * TURBINE_MERKLE_HASH_BYTES,
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let data_buffer_size = Self::capacity(proof_size).ok()?;
        let offset = SIZE_OF_DATA_SHRED_HEADERS + data_buffer_size;
        Some(offset..offset + TURBINE_MERKLE_ROOT_BYTES)
    }

    fn merkle_root_slice(&self) -> Result<&[u8], Error> {
        let proof_size = self.proof_size()?;
        let capacity = Self::capacity(proof_size)?;
        let root_offset = SIZE_OF_DATA_SHRED_HEADERS + capacity;
        self.payload
            .get(root_offset..root_offset + TURBINE_MERKLE_ROOT_BYTES)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn merkle_proof_slice(&self) -> Result<&[u8], Error> {
        let proof_size = self.proof_size()?;
        let proof_byte_count = (proof_size as usize) * TURBINE_MERKLE_HASH_BYTES;
        let capacity = Self::capacity(proof_size)?;
        let proof_offset = SIZE_OF_DATA_SHRED_HEADERS + capacity + TURBINE_MERKLE_ROOT_BYTES;
        self.payload
            .get(proof_offset..proof_offset + proof_byte_count)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn merkle_leaf_hash(&self) -> Result<TurbineMerkleHash, Error> {
        let chunk = self.erasure_shard_as_slice()?;
        Ok(TurbineMerkleHash::hash_leaf(&[chunk]))
    }

    fn verify_merkle_proof(&self) -> Result<bool, Error> {
        let root_hash = TurbineMerkleHash::from(self.merkle_root_slice()?);
        Ok(TurbineMerkleProof::verify_buf(
            &root_hash,
            self.merkle_proof_slice()?,
            &self.merkle_leaf_hash()?,
            self.erasure_shard_index()?,
        ))
    }
}

impl ShredCode {
    // proof_size is the number of proof entries in the merkle tree branch.
    fn proof_size(&self) -> Result<u8, Error> {
        match self.common_header.shred_variant {
            ShredVariant::MerkleCode(proof_size) => Ok(proof_size),
            _ => Err(Error::InvalidShredVariant),
        }
    }

    // Size of the chunk of payload which will be erasure coded.
    fn size_of_erasure_encoded_slice(proof_size: u8) -> Result<usize, Error> {
        // Merkle branch is generated and signed after coding shreds are
        // generated. Coding shred headers cannot be erasure coded either.
        Self::SIZE_OF_PAYLOAD
            .checked_sub(
                SIZE_OF_CODING_SHRED_HEADERS
                    + TURBINE_MERKLE_ROOT_BYTES
                    + usize::from(proof_size) * TURBINE_MERKLE_HASH_BYTES,
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    fn merkle_root_slice(&self) -> Result<&[u8], Error> {
        let proof_size = self.proof_size()?;
        let capacity = Self::size_of_erasure_encoded_slice(proof_size)?;
        let root_offset = SIZE_OF_CODING_SHRED_HEADERS + capacity;
        self.payload
            .get(root_offset..root_offset + TURBINE_MERKLE_ROOT_BYTES)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn merkle_proof_slice(&self) -> Result<&[u8], Error> {
        let proof_size = self.proof_size()?;
        let proof_byte_count = (proof_size as usize) * TURBINE_MERKLE_HASH_BYTES;
        let capacity = Self::size_of_erasure_encoded_slice(proof_size)?;
        let proof_offset = SIZE_OF_CODING_SHRED_HEADERS + capacity + TURBINE_MERKLE_ROOT_BYTES;
        self.payload
            .get(proof_offset..proof_offset + proof_byte_count)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn merkle_leaf_hash(&self) -> Result<TurbineMerkleHash, Error> {
        let proof_size = self.proof_size()?;
        let shard_size = Self::size_of_erasure_encoded_slice(proof_size)?;
        let chunk = self
            .payload
            .get(SIZE_OF_SIGNATURE..SIZE_OF_CODING_SHRED_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))?;
        Ok(TurbineMerkleHash::hash_leaf(&[chunk]))
    }

    fn verify_merkle_proof(&self) -> Result<bool, Error> {
        let root_hash = TurbineMerkleHash::from(self.merkle_root_slice()?);
        Ok(TurbineMerkleProof::verify_buf(
            &root_hash,
            self.merkle_proof_slice()?,
            &self.merkle_leaf_hash()?,
            self.erasure_shard_index()?,
        ))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let offset =
            SIZE_OF_CODING_SHRED_HEADERS + Self::size_of_erasure_encoded_slice(proof_size).ok()?;
        Some(offset..offset + TURBINE_MERKLE_ROOT_BYTES)
    }

    pub(super) fn erasure_mismatch(&self, other: &ShredCode) -> bool {
        if shred_code::erasure_mismatch(self, other)
            || self.common_header.signature != other.common_header.signature
        {
            return true;
        }
        let root_slice = match self.merkle_root_slice() {
            Ok(slice) => slice,
            _ => return true,
        };
        let other_root_slice = match other.merkle_root_slice() {
            Ok(slice) => slice,
            _ => return true,
        };
        root_slice != other_root_slice
    }
}

impl Shred for ShredData {
    impl_shred_common!();

    // Also equal to:
    // SIZE_OF_DATA_SHRED_HEADERS
    //       + ShredData::capacity(proof_size).unwrap()
    //       + SIZE_OF_MERKLE_ROOT
    //       + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY
    const SIZE_OF_PAYLOAD: usize =
        ShredCode::SIZE_OF_PAYLOAD - SIZE_OF_CODING_SHRED_HEADERS + SIZE_OF_SIGNATURE;

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        if payload.len() < Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(payload.len()));
        }
        payload.truncate(Self::SIZE_OF_PAYLOAD);
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        let proof_size = match common_header.shred_variant {
            ShredVariant::MerkleData(proof_size) => proof_size,
            _ => return Err(Error::InvalidShredVariant),
        };
        let data_header = deserialize_from_with_limit(&mut cursor)?;
        // Skip data buffer.
        let data_buffer_size = Self::capacity(proof_size)?;
        let data_buffer_size = i64::try_from(data_buffer_size).unwrap();
        cursor.seek(SeekFrom::Current(data_buffer_size))?;

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
        let proof_size = self.proof_size()?;
        let data_buffer_size = Self::capacity(proof_size)?;
        let mut shard = self.payload;
        shard.truncate(SIZE_OF_DATA_SHRED_HEADERS + data_buffer_size);
        shard.drain(0..SIZE_OF_SIGNATURE);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let proof_size = self.proof_size()?;
        let data_buffer_size = Self::capacity(proof_size)?;
        self.payload
            .get(SIZE_OF_SIGNATURE..SIZE_OF_DATA_SHRED_HEADERS + data_buffer_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn sanitize(&self) -> Result<(), Error> {
        match self.common_header.shred_variant {
            ShredVariant::MerkleData(_) => (),
            _ => return Err(Error::InvalidShredVariant),
        }
        if !self.verify_merkle_proof()? {
            return Err(Error::InvalidMerkleProof);
        }
        shred_data::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        let s = self.merkle_root_slice();
        debug_assert!(s.is_ok());
        s.unwrap_or(&[])
    }
}

impl Shred for ShredCode {
    impl_shred_common!();
    const SIZE_OF_PAYLOAD: usize = shred_code::ShredCode::SIZE_OF_PAYLOAD;

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        let proof_size = match common_header.shred_variant {
            ShredVariant::MerkleCode(proof_size) => proof_size,
            _ => return Err(Error::InvalidShredVariant),
        };
        let coding_header = deserialize_from_with_limit(&mut cursor)?;
        // Skip erasure code shard.
        let shard_size = Self::size_of_erasure_encoded_slice(proof_size)?;
        let shard_size = i64::try_from(shard_size).unwrap();
        cursor.seek(SeekFrom::Current(shard_size))?;

        // see: https://github.com/solana-labs/solana/pull/10109
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
        let proof_size = self.proof_size()?;
        let shard_size = Self::size_of_erasure_encoded_slice(proof_size)?;
        let mut shard = self.payload;
        shard.drain(..SIZE_OF_CODING_SHRED_HEADERS);
        shard.truncate(shard_size);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let proof_size = self.proof_size()?;
        let shard_size = Self::size_of_erasure_encoded_slice(proof_size)?;
        self.payload
            .get(SIZE_OF_CODING_SHRED_HEADERS..SIZE_OF_CODING_SHRED_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn sanitize(&self) -> Result<(), Error> {
        match self.common_header.shred_variant {
            ShredVariant::MerkleCode(_) => (),
            _ => return Err(Error::InvalidShredVariant),
        }
        if !self.verify_merkle_proof()? {
            return Err(Error::InvalidMerkleProof);
        }
        shred_code::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        let s = self.merkle_root_slice();
        debug_assert!(s.is_ok());
        s.unwrap_or(&[])
    }
}

impl ShredDataTrait for ShredData {
    #[inline]
    fn data_header(&self) -> &DataShredHeader {
        &self.data_header
    }

    fn data(&self) -> Result<&[u8], Error> {
        let proof_size = self.proof_size()?;
        let data_buffer_size = Self::capacity(proof_size)?;
        let size = usize::from(self.data_header.size);
        if size > self.payload.len()
            || size < SIZE_OF_DATA_SHRED_HEADERS
            || size > SIZE_OF_DATA_SHRED_HEADERS + data_buffer_size
        {
            return Err(Error::InvalidDataSize {
                size: self.data_header.size,
                payload: self.payload.len(),
            });
        }
        Ok(&self.payload[SIZE_OF_DATA_SHRED_HEADERS..size])
    }

    // Only for tests.
    fn set_last_in_slot(&mut self) {
        self.data_header.flags |= ShredFlags::LAST_SHRED_IN_SLOT;
        let buffer = &mut self.payload[SIZE_OF_COMMON_SHRED_HEADER..];
        bincode::serialize_into(buffer, &self.data_header).unwrap();
    }
}

impl ShredCodeTrait for ShredCode {
    #[inline]
    fn coding_header(&self) -> &CodingShredHeader {
        &self.coding_header
    }
}

#[cfg(test)]
mod test {
    use {
        super::*, rand::Rng, solana_perf::turbine_merkle::TurbineMerkleTree,
        solana_sdk::hash::Hash, std::iter::repeat_with,
    };

    // Total size of a data shred including headers and merkle branch.
    fn shred_data_size_of_payload(proof_size: u8) -> usize {
        SIZE_OF_DATA_SHRED_HEADERS
            + ShredData::capacity(proof_size).unwrap()
            + TURBINE_MERKLE_ROOT_BYTES
            + usize::from(proof_size) * TURBINE_MERKLE_HASH_BYTES
    }

    // Merkle branch is generated and signed after coding shreds are generated.
    // All payload excluding merkle branch and the signature are erasure coded.
    // Therefore the data capacity is equal to erasure encoded shard size minus
    // size of erasure encoded header.
    fn shred_data_capacity(proof_size: u8) -> usize {
        const SIZE_OF_ERASURE_ENCODED_HEADER: usize =
            SIZE_OF_DATA_SHRED_HEADERS - SIZE_OF_SIGNATURE;
        ShredCode::size_of_erasure_encoded_slice(proof_size).unwrap()
            - SIZE_OF_ERASURE_ENCODED_HEADER
    }

    fn shred_data_size_of_erasure_encoded_slice(proof_size: u8) -> usize {
        ShredData::SIZE_OF_PAYLOAD
            - SIZE_OF_SIGNATURE
            - TURBINE_MERKLE_ROOT_BYTES
            - usize::from(proof_size) * TURBINE_MERKLE_HASH_BYTES
    }

    #[test]
    fn test_shred_data_size_of_payload() {
        for proof_size in 0..0x15 {
            assert_eq!(
                ShredData::SIZE_OF_PAYLOAD,
                shred_data_size_of_payload(proof_size)
            );
        }
    }

    #[test]
    fn test_shred_data_capacity() {
        for proof_size in 0..0x15 {
            assert_eq!(
                ShredData::capacity(proof_size).unwrap(),
                shred_data_capacity(proof_size)
            );
        }
    }

    #[test]
    fn test_size_of_erasure_encoded_slice() {
        for proof_size in 0..0x15 {
            assert_eq!(
                ShredCode::size_of_erasure_encoded_slice(proof_size).unwrap(),
                shred_data_size_of_erasure_encoded_slice(proof_size),
            );
        }
    }

    #[test]
    fn test_merkle_proof_entry_from_hash() {
        let mut rng = rand::thread_rng();
        let bytes: [u8; 32] = rng.gen();
        let hash = Hash::from(bytes);
        let entry = &hash.as_ref()[..TURBINE_MERKLE_HASH_BYTES];
        let entry = TurbineMerkleHash::from(entry);
        assert_eq!(entry.as_ref(), &bytes[..TURBINE_MERKLE_HASH_BYTES]);
    }

    fn run_merkle_tree_round_trip(size: usize) {
        let mut rng = rand::thread_rng();
        let nodes = repeat_with(|| rng.gen::<[u8; 20]>()).map(TurbineMerkleHash::from);
        let nodes: Vec<_> = nodes.take(size).collect();

        let tree = TurbineMerkleTree::new_from_leaves(&nodes);
        let root = tree.root();

        for index in 0..size {
            let proof = tree.prove(index);
            assert!(proof.verify(&root, &nodes[index], index));
            for i in (0..size).filter(|&i| i != index) {
                assert!(!proof.verify(&root, &nodes[i], i));
            }
        }
    }

    #[test]
    fn test_merkle_tree_round_trip() {
        for size in [1, 2, 3, 4, 5, 6, 7, 8, 9, 19, 37, 64, 79, 96] {
            run_merkle_tree_round_trip(size);
        }
    }

    fn make_test_shred(variant: ShredVariant, capacity: usize) -> ShredData {
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: variant,
            slot: 123,
            index: 0,
            version: 0,
            fec_set_index: 0,
        };
        let data_header = DataShredHeader {
            parent_offset: 0,
            flags: ShredFlags::default(),
            size: u16::try_from(SIZE_OF_DATA_SHRED_HEADERS + capacity).unwrap(),
        };
        let mut payload = vec![0u8; ShredData::SIZE_OF_PAYLOAD];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = u8::try_from(i % 256).unwrap();
        }
        ShredData {
            common_header,
            data_header,
            payload,
        }
    }

    #[test]
    fn test_merkle_shred_root_slice() {
        let proof_len = 5u8;
        let shred = make_test_shred(ShredVariant::MerkleData(proof_len), 500);
        let root_slice = shred.merkle_root_slice().unwrap();
        assert_eq!(root_slice.len(), TURBINE_MERKLE_ROOT_BYTES);
        let capacity = ShredData::capacity(proof_len).unwrap();
        let expected_offset = SIZE_OF_DATA_SHRED_HEADERS + capacity;
        assert_eq!(root_slice[0], u8::try_from(expected_offset % 256).unwrap());
    }

    #[test]
    fn test_merkle_shred_proof_slice() {
        for proof_len in 0..=7 {
            let shred = make_test_shred(
                ShredVariant::MerkleData(u8::try_from(proof_len).unwrap()),
                500,
            );
            let proof_slice = shred.merkle_proof_slice().unwrap();
            assert_eq!(proof_slice.len(), TURBINE_MERKLE_HASH_BYTES * proof_len);
            let capacity = ShredData::capacity(u8::try_from(proof_len).unwrap()).unwrap();
            let expected_offset = SIZE_OF_DATA_SHRED_HEADERS + TURBINE_MERKLE_ROOT_BYTES + capacity;
            if proof_slice.is_empty() {
                assert_eq!(proof_len, 0);
            } else {
                assert_eq!(proof_slice[0], u8::try_from(expected_offset % 256).unwrap());
            }
        }
    }
}
