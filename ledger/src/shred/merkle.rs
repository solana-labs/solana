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
    solana_sdk::{
        clock::Slot,
        hash::{hashv, Hash},
        pubkey::Pubkey,
        signature::Signature,
    },
    static_assertions::const_assert_eq,
    std::{
        io::{Cursor, Seek, SeekFrom},
        iter::repeat_with,
        ops::Range,
    },
};

const_assert_eq!(SIZE_OF_MERKLE_ROOT, 20);
const SIZE_OF_MERKLE_ROOT: usize = std::mem::size_of::<MerkleRoot>();
const_assert_eq!(SIZE_OF_MERKLE_PROOF_ENTRY, 20);
const SIZE_OF_MERKLE_PROOF_ENTRY: usize = std::mem::size_of::<MerkleProofEntry>();
const_assert_eq!(ShredData::SIZE_OF_PAYLOAD, 1203);

// Defense against second preimage attack:
// https://en.wikipedia.org/wiki/Merkle_tree#Second_preimage_attack
const MERKLE_HASH_PREFIX_LEAF: &[u8] = &[0x00];
const MERKLE_HASH_PREFIX_NODE: &[u8] = &[0x01];

type MerkleRoot = MerkleProofEntry;
type MerkleProofEntry = [u8; 20];

// Layout: {common, data} headers | data buffer | merkle branch
// The slice past signature and before merkle branch is erasure coded.
// Same slice is hashed to generate merkle tree.
// The root of merkle tree is signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    merkle_branch: MerkleBranch,
    payload: Vec<u8>,
}

// Layout: {common, coding} headers | erasure coded shard | merkle branch
// The slice past signature and before merkle branch is hashed to generate
// merkle tree. The root of merkle tree is signed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShredCode {
    common_header: ShredCommonHeader,
    coding_header: CodingShredHeader,
    merkle_branch: MerkleBranch,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MerkleBranch {
    root: MerkleRoot,
    proof: Vec<MerkleProofEntry>,
}

impl ShredData {
    pub(super) fn new_from_data(
        merkle_proof_size: u8,
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
            shred_variant: ShredVariant::MerkleData(merkle_proof_size),
            slot,
            index,
            version,
            fec_set_index,
        };
        let size = (data.len() + SIZE_OF_DATA_SHRED_HEADERS) as u16;
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
        debug_assert_eq!(offset, SIZE_OF_DATA_SHRED_HEADERS);
        payload[offset..offset + data.len()].copy_from_slice(data);

        // TODO directly reference merkle from payload
        let root = MerkleRoot::default();
        let proof: Vec<_> = repeat_with(MerkleProofEntry::default)
            .take(merkle_proof_size as usize)
            .collect();
        let merkle_branch = MerkleBranch { root, proof };

        Self {
            common_header,
            data_header,
            merkle_branch,
            payload,
        }
    }

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
                    + SIZE_OF_MERKLE_ROOT
                    + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY,
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let data_buffer_size = Self::capacity(proof_size).ok()?;
        let offset = SIZE_OF_DATA_SHRED_HEADERS + data_buffer_size;
        Some(offset..offset + SIZE_OF_MERKLE_ROOT)
    }

    fn merkle_tree_node(&self) -> Result<Hash, Error> {
        let chunk = self.erasure_shard_as_slice()?;
        Ok(hashv(&[MERKLE_HASH_PREFIX_LEAF, chunk]))
    }

    fn verify_merkle_proof(&self) -> Result<bool, Error> {
        let node = self.merkle_tree_node()?;
        let index = self.erasure_shard_index()?;
        Ok(verify_merkle_proof(index, node, &self.merkle_branch))
    }
}

impl ShredCode {
    pub(super) fn new_from_parity_shard(
        merkle_proof_size: u8,
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
            shred_variant: ShredVariant::MerkleCode(merkle_proof_size),
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
            debug_assert_eq!(offset, SIZE_OF_CODING_SHRED_HEADERS);
            payload[offset..].copy_from_slice(parity_shard);
        }

        // TODO directly reference merkle from payload
        let root = MerkleRoot::default();
        let proof: Vec<_> = repeat_with(MerkleProofEntry::default)
            .take(merkle_proof_size as usize)
            .collect();
        let merkle_branch = MerkleBranch { root, proof };

        Self {
            common_header,
            coding_header,
            merkle_branch,
            payload,
        }
    }

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
                    + SIZE_OF_MERKLE_ROOT
                    + SIZE_OF_MERKLE_PROOF_ENTRY * usize::from(proof_size),
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    fn merkle_tree_node(&self) -> Result<Hash, Error> {
        let proof_size = self.proof_size()?;
        let shard_size = Self::size_of_erasure_encoded_slice(proof_size)?;
        let chunk = self
            .payload
            .get(SIZE_OF_SIGNATURE..SIZE_OF_CODING_SHRED_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))?;
        Ok(hashv(&[MERKLE_HASH_PREFIX_LEAF, chunk]))
    }

    fn verify_merkle_proof(&self) -> Result<bool, Error> {
        let node = self.merkle_tree_node()?;
        let index = self.erasure_shard_index()?;
        Ok(verify_merkle_proof(index, node, &self.merkle_branch))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let offset =
            SIZE_OF_CODING_SHRED_HEADERS + Self::size_of_erasure_encoded_slice(proof_size).ok()?;
        Some(offset..offset + SIZE_OF_MERKLE_ROOT)
    }

    pub(super) fn erasure_mismatch(&self, other: &ShredCode) -> bool {
        shred_code::erasure_mismatch(self, other)
            || self.merkle_branch.root != other.merkle_branch.root
            || self.common_header.signature != other.common_header.signature
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
        // Deserialize merkle branch.
        let root = deserialize_from_with_limit(&mut cursor)?;
        let proof = repeat_with(|| deserialize_from_with_limit(&mut cursor))
            .take(usize::from(proof_size))
            .collect::<Result<_, _>>()?;
        let merkle_branch = MerkleBranch { root, proof };
        let shred = Self {
            common_header,
            data_header,
            merkle_branch,
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
            ShredVariant::MerkleData(proof_size) => {
                if self.merkle_branch.proof.len() != usize::from(proof_size) {
                    return Err(Error::InvalidProofSize(proof_size));
                }
            }
            _ => return Err(Error::InvalidShredVariant),
        }
        shred_data::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        self.merkle_branch.root.as_ref()
    }

    fn verify(&self, pubkey: &Pubkey) -> bool {
        let ret = self.verify_merkle_proof();
        if ret.is_err() || !ret.unwrap() {
            return false;
        }
        let message = self.signed_message();
        self.common_header
            .signature
            .verify(pubkey.as_ref(), message)
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
        // Deserialize merkle branch.
        let root = deserialize_from_with_limit(&mut cursor)?;
        let proof = repeat_with(|| deserialize_from_with_limit(&mut cursor))
            .take(usize::from(proof_size))
            .collect::<Result<_, _>>()?;
        let merkle_branch = MerkleBranch { root, proof };
        // see: https://github.com/solana-labs/solana/pull/10109
        payload.truncate(Self::SIZE_OF_PAYLOAD);
        let shred = Self {
            common_header,
            coding_header,
            merkle_branch,
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
            ShredVariant::MerkleCode(proof_size) => {
                if self.merkle_branch.proof.len() != usize::from(proof_size) {
                    return Err(Error::InvalidProofSize(proof_size));
                }
            }
            _ => return Err(Error::InvalidShredVariant),
        }
        shred_code::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        self.merkle_branch.root.as_ref()
    }

    fn verify(&self, pubkey: &Pubkey) -> bool {
        let ret = self.verify_merkle_proof();
        if ret.is_err() || !ret.unwrap() {
            return false;
        }
        let message = self.signed_message();
        self.common_header
            .signature
            .verify(pubkey.as_ref(), message)
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

// Obtains parent's hash by joining two sibiling nodes in merkle tree.
fn join_nodes<S: AsRef<[u8]>, T: AsRef<[u8]>>(node: S, other: T) -> Hash {
    let node = &node.as_ref()[..SIZE_OF_MERKLE_PROOF_ENTRY];
    let other = &other.as_ref()[..SIZE_OF_MERKLE_PROOF_ENTRY];
    hashv(&[MERKLE_HASH_PREFIX_NODE, node, other])
}

fn verify_merkle_proof(index: usize, node: Hash, merkle_branch: &MerkleBranch) -> bool {
    let proof = merkle_branch.proof.iter();
    let (index, root) = proof.fold((index, node), |(index, node), other| {
        let parent = if index % 2 == 0 {
            join_nodes(node, other)
        } else {
            join_nodes(other, node)
        };
        (index >> 1, parent)
    });
    let root = &root.as_ref()[..SIZE_OF_MERKLE_ROOT];
    (index, root) == (0usize, &merkle_branch.root[..])
}

#[cfg(test)]
fn make_merkle_tree(mut nodes: Vec<Hash>) -> Vec<Hash> {
    let mut size = nodes.len();
    while size > 1 {
        let offset = nodes.len() - size;
        for index in (offset..offset + size).step_by(2) {
            let node = &nodes[index];
            let other = &nodes[(index + 1).min(offset + size - 1)];
            let parent = join_nodes(node, other);
            nodes.push(parent);
        }
        size = nodes.len() - offset - size;
    }
    nodes
}

#[cfg(test)]
fn make_merkle_branch(
    mut index: usize, // leaf index ~ shred's erasure shard index.
    mut size: usize,  // number of leaves ~ erasure batch size.
    tree: &[Hash],
) -> Option<MerkleBranch> {
    if index >= size {
        return None;
    }
    let mut offset = 0;
    let mut proof = Vec::<MerkleProofEntry>::new();
    while size > 1 {
        let node = tree.get(offset + (index ^ 1).min(size - 1))?;
        let entry = &node.as_ref()[..SIZE_OF_MERKLE_PROOF_ENTRY];
        proof.push(MerkleProofEntry::try_from(entry).unwrap());
        offset += size;
        size = (size + 1) >> 1;
        index >>= 1;
    }
    if offset + 1 != tree.len() {
        return None;
    }
    let root = &tree.last()?.as_ref()[..SIZE_OF_MERKLE_ROOT];
    let root = MerkleRoot::try_from(root).unwrap();
    Some(MerkleBranch { root, proof })
}

#[cfg(test)]
mod test {
    use {
        super::*, crate::shred::MAX_DATA_SHREDS_PER_SLOT, matches::assert_matches, rand::Rng,
        solana_sdk::hash::Hash, std::iter::repeat_with,
    };

    #[test]
    fn test_sanitize_merkle_data_shred() {
        const MERKLE_PROOF_SIZE: u8 = 6;
        const MAX_DATA_PAYLOAD: usize = ShredData::SIZE_OF_PAYLOAD
            - (SIZE_OF_DATA_SHRED_HEADERS
                + SIZE_OF_MERKLE_ROOT
                + (MERKLE_PROOF_SIZE as usize) * SIZE_OF_MERKLE_PROOF_ENTRY);

        assert_eq!(
            MAX_DATA_PAYLOAD,
            ShredData::capacity(MERKLE_PROOF_SIZE).unwrap()
        );

        let data = [0xa5u8; MAX_DATA_PAYLOAD];
        let mut shred = ShredData::new_from_data(
            MERKLE_PROOF_SIZE,
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
            assert_matches!(shred.sanitize(), Err(Error::InvalidPayloadSize(1204)));
        }
        {
            let mut shred = shred.clone();
            shred.data_header.size += 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1064,
                    payload: 1203,
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
                    payload: 1203,
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
            shred.data_header.size = shred.payload.len() as u16 + 1;
            assert_matches!(
                shred.sanitize(),
                Err(Error::InvalidDataSize {
                    size: 1204,
                    payload: 1203,
                })
            );
        }
    }

    #[test]
    fn test_sanitize_merkle_coding_shred() {
        const MERKLE_PROOF_SIZE: u8 = 6;
        let mut shred = ShredCode::new_from_parity_shard(
            MERKLE_PROOF_SIZE,
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

    // Total size of a data shred including headers and merkle branch.
    fn shred_data_size_of_payload(proof_size: u8) -> usize {
        SIZE_OF_DATA_SHRED_HEADERS
            + ShredData::capacity(proof_size).unwrap()
            + SIZE_OF_MERKLE_ROOT
            + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY
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
            - SIZE_OF_MERKLE_ROOT
            - usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY
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
        let entry = &hash.as_ref()[..SIZE_OF_MERKLE_PROOF_ENTRY];
        let entry = MerkleProofEntry::try_from(entry).unwrap();
        assert_eq!(entry, &bytes[..SIZE_OF_MERKLE_PROOF_ENTRY]);
    }

    fn run_merkle_tree_round_trip(size: usize) {
        let mut rng = rand::thread_rng();
        let nodes = repeat_with(|| rng.gen::<[u8; 32]>()).map(Hash::from);
        let nodes: Vec<_> = nodes.take(size).collect();
        let tree = make_merkle_tree(nodes.clone());
        for index in 0..size {
            let branch = make_merkle_branch(index, size, &tree).unwrap();
            let root = &tree.last().unwrap().as_ref()[..SIZE_OF_MERKLE_ROOT];
            assert_eq!(&branch.root, root);
            assert!(verify_merkle_proof(index, nodes[index], &branch));
            for i in (0..size).filter(|&i| i != index) {
                assert!(!verify_merkle_proof(i, nodes[i], &branch));
            }
        }
    }

    #[test]
    fn test_merkle_tree_round_trip() {
        for size in [1, 2, 3, 4, 5, 6, 7, 8, 9, 19, 37, 64, 79] {
            run_merkle_tree_round_trip(size);
        }
    }
}
