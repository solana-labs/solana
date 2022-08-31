#[cfg(test)]
use {crate::shred::ShredType, solana_sdk::pubkey::Pubkey};
use {
    crate::{
        shred::{
            common::impl_shred_common,
            dispatch, shred_code, shred_data,
            traits::{
                Shred as ShredTrait, ShredCode as ShredCodeTrait, ShredData as ShredDataTrait,
            },
            CodingShredHeader, DataShredHeader, Error, ShredCommonHeader, ShredFlags, ShredVariant,
            SIZE_OF_CODING_SHRED_HEADERS, SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADERS,
            SIZE_OF_SIGNATURE,
        },
        shredder::ReedSolomon,
    },
    reed_solomon_erasure::Error::{InvalidIndex, TooFewParityShards, TooFewShards},
    solana_perf::packet::deserialize_from_with_limit,
    solana_sdk::{
        clock::Slot,
        hash::{hashv, Hash},
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
pub(super) enum Shred {
    ShredCode(ShredCode),
    ShredData(ShredData),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MerkleBranch {
    root: MerkleRoot,
    proof: Vec<MerkleProofEntry>,
}

impl Shred {
    dispatch!(fn common_header(&self) -> &ShredCommonHeader);
    dispatch!(fn erasure_shard_as_slice(&self) -> Result<&[u8], Error>);
    dispatch!(fn erasure_shard_index(&self) -> Result<usize, Error>);
    dispatch!(fn merkle_tree_node(&self) -> Result<Hash, Error>);
    dispatch!(fn sanitize(&self) -> Result<(), Error>);
    dispatch!(fn set_merkle_branch(&mut self, merkle_branch: MerkleBranch) -> Result<(), Error>);

    fn merkle_root(&self) -> &MerkleRoot {
        match self {
            Self::ShredCode(shred) => &shred.merkle_branch.root,
            Self::ShredData(shred) => &shred.merkle_branch.root,
        }
    }
}

#[cfg(test)]
impl Shred {
    dispatch!(fn set_signature(&mut self, signature: Signature));
    dispatch!(fn signed_message(&self) -> &[u8]);

    fn index(&self) -> u32 {
        self.common_header().index
    }

    fn shred_type(&self) -> ShredType {
        ShredType::from(self.common_header().shred_variant)
    }

    fn signature(&self) -> Signature {
        self.common_header().signature
    }

    #[must_use]
    fn verify(&self, pubkey: &Pubkey) -> bool {
        let message = self.signed_message();
        self.signature().verify(pubkey.as_ref(), message)
    }
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
    //   ShredCode::capacity(proof_size).unwrap()
    //       - ShredData::SIZE_OF_HEADERS
    //       + SIZE_OF_SIGNATURE
    pub(super) fn capacity(proof_size: u8) -> Result<usize, Error> {
        Self::SIZE_OF_PAYLOAD
            .checked_sub(
                Self::SIZE_OF_HEADERS
                    + SIZE_OF_MERKLE_ROOT
                    + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY,
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let data_buffer_size = Self::capacity(proof_size).ok()?;
        let offset = Self::SIZE_OF_HEADERS + data_buffer_size;
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

    fn from_recovered_shard(signature: &Signature, mut shard: Vec<u8>) -> Result<Self, Error> {
        let shard_size = shard.len();
        if shard_size + SIZE_OF_SIGNATURE > Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidShardSize(shard_size));
        }
        shard.resize(Self::SIZE_OF_PAYLOAD, 0u8);
        shard.copy_within(0..shard_size, SIZE_OF_SIGNATURE);
        shard[0..SIZE_OF_SIGNATURE].copy_from_slice(signature.as_ref());
        // Deserialize headers.
        let mut cursor = Cursor::new(&shard[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        let proof_size = match common_header.shred_variant {
            ShredVariant::MerkleData(proof_size) => proof_size,
            _ => return Err(Error::InvalidShredVariant),
        };
        if ShredCode::capacity(proof_size)? != shard_size {
            return Err(Error::InvalidShardSize(shard_size));
        }
        let data_header = deserialize_from_with_limit(&mut cursor)?;
        Ok(Self {
            common_header,
            data_header,
            merkle_branch: MerkleBranch::new_zeroed(proof_size),
            payload: shard,
        })
    }

    fn set_merkle_branch(&mut self, merkle_branch: MerkleBranch) -> Result<(), Error> {
        let proof_size = self.proof_size()?;
        if merkle_branch.proof.len() != usize::from(proof_size) {
            return Err(Error::InvalidMerkleProof);
        }
        let offset = Self::SIZE_OF_HEADERS + Self::capacity(proof_size)?;
        let mut cursor = Cursor::new(
            self.payload
                .get_mut(offset..)
                .ok_or(Error::InvalidProofSize(proof_size))?,
        );
        bincode::serialize_into(&mut cursor, &merkle_branch.root)?;
        for entry in &merkle_branch.proof {
            bincode::serialize_into(&mut cursor, entry)?;
        }
        self.merkle_branch = merkle_branch;
        Ok(())
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

    // Size of buffer embedding erasure codes.
    fn capacity(proof_size: u8) -> Result<usize, Error> {
        // Merkle branch is generated and signed after coding shreds are
        // generated. Coding shred headers cannot be erasure coded either.
        Self::SIZE_OF_PAYLOAD
            .checked_sub(
                Self::SIZE_OF_HEADERS
                    + SIZE_OF_MERKLE_ROOT
                    + SIZE_OF_MERKLE_PROOF_ENTRY * usize::from(proof_size),
            )
            .ok_or(Error::InvalidProofSize(proof_size))
    }

    fn merkle_tree_node(&self) -> Result<Hash, Error> {
        let proof_size = self.proof_size()?;
        let shard_size = Self::capacity(proof_size)?;
        let chunk = self
            .payload
            .get(SIZE_OF_SIGNATURE..Self::SIZE_OF_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))?;
        Ok(hashv(&[MERKLE_HASH_PREFIX_LEAF, chunk]))
    }

    fn verify_merkle_proof(&self) -> Result<bool, Error> {
        let node = self.merkle_tree_node()?;
        let index = self.erasure_shard_index()?;
        Ok(verify_merkle_proof(index, node, &self.merkle_branch))
    }

    pub(super) fn get_signed_message_range(proof_size: u8) -> Option<Range<usize>> {
        let offset = Self::SIZE_OF_HEADERS + Self::capacity(proof_size).ok()?;
        Some(offset..offset + SIZE_OF_MERKLE_ROOT)
    }

    pub(super) fn erasure_mismatch(&self, other: &ShredCode) -> bool {
        shred_code::erasure_mismatch(self, other)
            || self.merkle_branch.root != other.merkle_branch.root
            || self.common_header.signature != other.common_header.signature
    }

    fn from_recovered_shard(
        common_header: ShredCommonHeader,
        coding_header: CodingShredHeader,
        mut shard: Vec<u8>,
    ) -> Result<Self, Error> {
        let proof_size = match common_header.shred_variant {
            ShredVariant::MerkleCode(proof_size) => proof_size,
            _ => return Err(Error::InvalidShredVariant),
        };
        let shard_size = shard.len();
        if Self::capacity(proof_size)? != shard_size {
            return Err(Error::InvalidShardSize(shard_size));
        }
        if shard_size + Self::SIZE_OF_HEADERS > Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidShardSize(shard_size));
        }
        shard.resize(Self::SIZE_OF_PAYLOAD, 0u8);
        shard.copy_within(0..shard_size, Self::SIZE_OF_HEADERS);
        let mut cursor = Cursor::new(&mut shard[..]);
        bincode::serialize_into(&mut cursor, &common_header)?;
        bincode::serialize_into(&mut cursor, &coding_header)?;
        Ok(Self {
            common_header,
            coding_header,
            merkle_branch: MerkleBranch::new_zeroed(proof_size),
            payload: shard,
        })
    }

    fn set_merkle_branch(&mut self, merkle_branch: MerkleBranch) -> Result<(), Error> {
        let proof_size = self.proof_size()?;
        if merkle_branch.proof.len() != usize::from(proof_size) {
            return Err(Error::InvalidMerkleProof);
        }
        let offset = Self::SIZE_OF_HEADERS + Self::capacity(proof_size)?;
        let mut cursor = Cursor::new(
            self.payload
                .get_mut(offset..)
                .ok_or(Error::InvalidProofSize(proof_size))?,
        );
        bincode::serialize_into(&mut cursor, &merkle_branch.root)?;
        for entry in &merkle_branch.proof {
            bincode::serialize_into(&mut cursor, entry)?;
        }
        self.merkle_branch = merkle_branch;
        Ok(())
    }
}

impl MerkleBranch {
    fn new_zeroed(proof_size: u8) -> Self {
        Self {
            root: MerkleRoot::default(),
            proof: vec![MerkleProofEntry::default(); usize::from(proof_size)],
        }
    }
}

impl ShredTrait for ShredData {
    impl_shred_common!();

    // Also equal to:
    // ShredData::SIZE_OF_HEADERS
    //       + ShredData::capacity(proof_size).unwrap()
    //       + SIZE_OF_MERKLE_ROOT
    //       + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY
    const SIZE_OF_PAYLOAD: usize =
        ShredCode::SIZE_OF_PAYLOAD - ShredCode::SIZE_OF_HEADERS + SIZE_OF_SIGNATURE;
    const SIZE_OF_HEADERS: usize = SIZE_OF_DATA_SHRED_HEADERS;

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
        shard.truncate(Self::SIZE_OF_HEADERS + data_buffer_size);
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
            .get(SIZE_OF_SIGNATURE..Self::SIZE_OF_HEADERS + data_buffer_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn erasure_shard_as_slice_mut(&mut self) -> Result<&mut [u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let proof_size = self.proof_size()?;
        let data_buffer_size = Self::capacity(proof_size)?;
        let payload_len = self.payload.len();
        self.payload
            .get_mut(SIZE_OF_SIGNATURE..Self::SIZE_OF_HEADERS + data_buffer_size)
            .ok_or(Error::InvalidPayloadSize(payload_len))
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
        if !self.verify_merkle_proof()? {
            return Err(Error::InvalidMerkleProof);
        }
        shred_data::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        self.merkle_branch.root.as_ref()
    }
}

impl ShredTrait for ShredCode {
    impl_shred_common!();
    const SIZE_OF_PAYLOAD: usize = shred_code::ShredCode::SIZE_OF_PAYLOAD;
    const SIZE_OF_HEADERS: usize = SIZE_OF_CODING_SHRED_HEADERS;

    fn from_payload(mut payload: Vec<u8>) -> Result<Self, Error> {
        let mut cursor = Cursor::new(&payload[..]);
        let common_header: ShredCommonHeader = deserialize_from_with_limit(&mut cursor)?;
        let proof_size = match common_header.shred_variant {
            ShredVariant::MerkleCode(proof_size) => proof_size,
            _ => return Err(Error::InvalidShredVariant),
        };
        let coding_header = deserialize_from_with_limit(&mut cursor)?;
        // Skip erasure code shard.
        let shard_size = Self::capacity(proof_size)?;
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
        let shard_size = Self::capacity(proof_size)?;
        let mut shard = self.payload;
        shard.drain(..Self::SIZE_OF_HEADERS);
        shard.truncate(shard_size);
        Ok(shard)
    }

    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let proof_size = self.proof_size()?;
        let shard_size = Self::capacity(proof_size)?;
        self.payload
            .get(Self::SIZE_OF_HEADERS..Self::SIZE_OF_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(self.payload.len()))
    }

    fn erasure_shard_as_slice_mut(&mut self) -> Result<&mut [u8], Error> {
        if self.payload.len() != Self::SIZE_OF_PAYLOAD {
            return Err(Error::InvalidPayloadSize(self.payload.len()));
        }
        let proof_size = self.proof_size()?;
        let shard_size = Self::capacity(proof_size)?;
        let payload_len = self.payload.len();
        self.payload
            .get_mut(Self::SIZE_OF_HEADERS..Self::SIZE_OF_HEADERS + shard_size)
            .ok_or(Error::InvalidPayloadSize(payload_len))
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
        if !self.verify_merkle_proof()? {
            return Err(Error::InvalidMerkleProof);
        }
        shred_code::sanitize(self)
    }

    fn signed_message(&self) -> &[u8] {
        self.merkle_branch.root.as_ref()
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
            || size < Self::SIZE_OF_HEADERS
            || size > Self::SIZE_OF_HEADERS + data_buffer_size
        {
            return Err(Error::InvalidDataSize {
                size: self.data_header.size,
                payload: self.payload.len(),
            });
        }
        Ok(&self.payload[Self::SIZE_OF_HEADERS..size])
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

pub(super) fn recover(mut shreds: Vec<Shred>) -> Result<Vec<Shred>, Error> {
    // Grab {common, coding} headers from first coding shred.
    let headers = shreds.iter().find_map(|shred| {
        let shred = match shred {
            Shred::ShredCode(shred) => shred,
            Shred::ShredData(_) => return None,
        };
        let position = u32::from(shred.coding_header.position);
        let common_header = ShredCommonHeader {
            index: shred.common_header.index.checked_sub(position)?,
            ..shred.common_header
        };
        let coding_header = CodingShredHeader {
            position: 0u16,
            ..shred.coding_header
        };
        Some((common_header, coding_header))
    });
    let (common_header, coding_header) = headers.ok_or(TooFewParityShards)?;
    debug_assert!(matches!(
        common_header.shred_variant,
        ShredVariant::MerkleCode(_)
    ));
    let proof_size = match common_header.shred_variant {
        ShredVariant::MerkleCode(proof_size) => proof_size,
        ShredVariant::MerkleData(_) | ShredVariant::LegacyCode | ShredVariant::LegacyData => {
            return Err(Error::InvalidShredVariant);
        }
    };
    // Verify that shreds belong to the same erasure batch
    // and have consistent headers.
    debug_assert!(shreds.iter().all(|shred| {
        let ShredCommonHeader {
            signature,
            shred_variant,
            slot,
            index: _,
            version,
            fec_set_index,
        } = shred.common_header();
        signature == &common_header.signature
            && slot == &common_header.slot
            && version == &common_header.version
            && fec_set_index == &common_header.fec_set_index
            && match shred {
                Shred::ShredData(_) => shred_variant == &ShredVariant::MerkleData(proof_size),
                Shred::ShredCode(shred) => {
                    let CodingShredHeader {
                        num_data_shreds,
                        num_coding_shreds,
                        position: _,
                    } = shred.coding_header;
                    shred_variant == &ShredVariant::MerkleCode(proof_size)
                        && num_data_shreds == coding_header.num_data_shreds
                        && num_coding_shreds == coding_header.num_coding_shreds
                }
            }
    }));
    let num_data_shreds = usize::from(coding_header.num_data_shreds);
    let num_coding_shreds = usize::from(coding_header.num_coding_shreds);
    let num_shards = num_data_shreds + num_coding_shreds;
    // Obtain erasure encoded shards from shreds.
    let shreds = {
        let mut batch = vec![None; num_shards];
        while let Some(shred) = shreds.pop() {
            let index = match shred.erasure_shard_index() {
                Ok(index) if index < batch.len() => index,
                _ => return Err(Error::from(InvalidIndex)),
            };
            batch[index] = Some(shred);
        }
        batch
    };
    let mut shards: Vec<Option<Vec<u8>>> = shreds
        .iter()
        .map(|shred| Some(shred.as_ref()?.erasure_shard_as_slice().ok()?.to_vec()))
        .collect();
    ReedSolomon::new(num_data_shreds, num_coding_shreds)?.reconstruct(&mut shards)?;
    let mask: Vec<_> = shreds.iter().map(Option::is_some).collect();
    // Reconstruct code and data shreds from erasure encoded shards.
    let mut shreds: Vec<_> = shreds
        .into_iter()
        .zip(shards)
        .enumerate()
        .map(|(index, (shred, shard))| {
            if let Some(shred) = shred {
                return Ok(shred);
            }
            let shard = shard.ok_or(TooFewShards)?;
            if index < num_data_shreds {
                let shred = ShredData::from_recovered_shard(&common_header.signature, shard)?;
                let ShredCommonHeader {
                    signature: _,
                    shred_variant,
                    slot,
                    index: _,
                    version,
                    fec_set_index,
                } = shred.common_header;
                if shred_variant != ShredVariant::MerkleData(proof_size)
                    || common_header.slot != slot
                    || common_header.version != version
                    || common_header.fec_set_index != fec_set_index
                {
                    return Err(Error::InvalidRecoveredShred);
                }
                Ok(Shred::ShredData(shred))
            } else {
                let offset = index - num_data_shreds;
                let coding_header = CodingShredHeader {
                    position: offset as u16,
                    ..coding_header
                };
                let common_header = ShredCommonHeader {
                    index: common_header.index + offset as u32,
                    ..common_header
                };
                let shred = ShredCode::from_recovered_shard(common_header, coding_header, shard)?;
                Ok(Shred::ShredCode(shred))
            }
        })
        .collect::<Result<_, Error>>()?;
    // Compute merkle tree and set the merkle branch on the recovered shreds.
    let nodes: Vec<_> = shreds
        .iter()
        .map(Shred::merkle_tree_node)
        .collect::<Result<_, _>>()?;
    let tree = make_merkle_tree(nodes);
    let merkle_root = &tree.last().unwrap().as_ref()[..SIZE_OF_MERKLE_ROOT];
    let merkle_root = MerkleRoot::try_from(merkle_root).unwrap();
    for (index, (shred, mask)) in shreds.iter_mut().zip(&mask).enumerate() {
        if *mask {
            if shred.merkle_root() != &merkle_root {
                return Err(Error::InvalidMerkleProof);
            }
        } else {
            let merkle_branch =
                make_merkle_branch(index, num_shards, &tree).ok_or(Error::InvalidMerkleProof)?;
            if merkle_branch.proof.len() != usize::from(proof_size) {
                return Err(Error::InvalidMerkleProof);
            }
            shred.set_merkle_branch(merkle_branch)?;
        }
    }
    // TODO: No need to verify merkle proof in sanitize here.
    shreds
        .into_iter()
        .zip(mask)
        .filter(|(_, mask)| !mask)
        .map(|(shred, _)| shred.sanitize().map(|_| shred))
        .collect()
}

#[cfg(test)]
mod test {
    use {
        super::*,
        itertools::Itertools,
        matches::assert_matches,
        rand::{seq::SliceRandom, CryptoRng, Rng},
        solana_sdk::signature::{Keypair, Signer},
        std::{cmp::Ordering, iter::repeat_with},
        test_case::test_case,
    };

    // Total size of a data shred including headers and merkle branch.
    fn shred_data_size_of_payload(proof_size: u8) -> usize {
        ShredData::SIZE_OF_HEADERS
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
            ShredData::SIZE_OF_HEADERS - SIZE_OF_SIGNATURE;
        ShredCode::capacity(proof_size).unwrap() - SIZE_OF_ERASURE_ENCODED_HEADER
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
    fn test_shred_code_capacity() {
        for proof_size in 0..0x15 {
            assert_eq!(
                ShredCode::capacity(proof_size).unwrap(),
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

    #[test_case(37)]
    #[test_case(64)]
    #[test_case(73)]
    fn test_recover_merkle_shreds(num_shreds: usize) {
        let mut rng = rand::thread_rng();
        for num_data_shreds in 1..num_shreds {
            let num_coding_shreds = num_shreds - num_data_shreds;
            run_recover_merkle_shreds(&mut rng, num_data_shreds, num_coding_shreds);
        }
    }

    fn run_recover_merkle_shreds<R: Rng + CryptoRng>(
        rng: &mut R,
        num_data_shreds: usize,
        num_coding_shreds: usize,
    ) {
        let keypair = Keypair::generate(rng);
        let num_shreds = num_data_shreds + num_coding_shreds;
        let proof_size = (num_shreds as f64).log2().ceil() as u8;
        let capacity = ShredData::capacity(proof_size).unwrap();
        let common_header = ShredCommonHeader {
            signature: Signature::default(),
            shred_variant: ShredVariant::MerkleData(proof_size),
            slot: 145865705,
            index: 1835,
            version: 4978,
            fec_set_index: 1835,
        };
        let data_header = DataShredHeader {
            parent_offset: 25,
            flags: unsafe { ShredFlags::from_bits_unchecked(0b0010_1010) },
            size: 0,
        };
        let coding_header = CodingShredHeader {
            num_data_shreds: num_data_shreds as u16,
            num_coding_shreds: num_coding_shreds as u16,
            position: 0,
        };
        let mut shreds = Vec::with_capacity(num_shreds);
        for i in 0..num_data_shreds {
            let common_header = ShredCommonHeader {
                index: common_header.index + i as u32,
                ..common_header
            };
            let size = ShredData::SIZE_OF_HEADERS + rng.gen_range(0, capacity);
            let data_header = DataShredHeader {
                size: size as u16,
                ..data_header
            };
            let mut payload = vec![0u8; ShredData::SIZE_OF_PAYLOAD];
            let mut cursor = Cursor::new(&mut payload[..]);
            bincode::serialize_into(&mut cursor, &common_header).unwrap();
            bincode::serialize_into(&mut cursor, &data_header).unwrap();
            rng.fill(&mut payload[ShredData::SIZE_OF_HEADERS..size]);
            let shred = ShredData {
                common_header,
                data_header,
                merkle_branch: MerkleBranch::new_zeroed(proof_size),
                payload,
            };
            shreds.push(Shred::ShredData(shred));
        }
        let data: Vec<_> = shreds
            .iter()
            .map(Shred::erasure_shard_as_slice)
            .collect::<Result<_, _>>()
            .unwrap();
        let mut parity = vec![vec![0u8; data[0].len()]; num_coding_shreds];
        ReedSolomon::new(num_data_shreds, num_coding_shreds)
            .unwrap()
            .encode_sep(&data, &mut parity[..])
            .unwrap();
        for (i, code) in parity.into_iter().enumerate() {
            let common_header = ShredCommonHeader {
                shred_variant: ShredVariant::MerkleCode(proof_size),
                index: common_header.index + i as u32 + 7,
                ..common_header
            };
            let coding_header = CodingShredHeader {
                position: i as u16,
                ..coding_header
            };
            let mut payload = vec![0u8; ShredCode::SIZE_OF_PAYLOAD];
            let mut cursor = Cursor::new(&mut payload[..]);
            bincode::serialize_into(&mut cursor, &common_header).unwrap();
            bincode::serialize_into(&mut cursor, &coding_header).unwrap();
            payload[ShredCode::SIZE_OF_HEADERS..ShredCode::SIZE_OF_HEADERS + code.len()]
                .copy_from_slice(&code);
            let shred = ShredCode {
                common_header,
                coding_header,
                merkle_branch: MerkleBranch::new_zeroed(proof_size),
                payload,
            };
            shreds.push(Shred::ShredCode(shred));
        }
        let nodes: Vec<_> = shreds
            .iter()
            .map(Shred::merkle_tree_node)
            .collect::<Result<_, _>>()
            .unwrap();
        let tree = make_merkle_tree(nodes);
        for (index, shred) in shreds.iter_mut().enumerate() {
            let merkle_branch = make_merkle_branch(index, num_shreds, &tree).unwrap();
            assert_eq!(merkle_branch.proof.len(), usize::from(proof_size));
            shred.set_merkle_branch(merkle_branch).unwrap();
            let signature = keypair.sign_message(shred.signed_message());
            shred.set_signature(signature);
            assert!(shred.verify(&keypair.pubkey()));
            assert_matches!(shred.sanitize(), Ok(()));
        }
        assert_eq!(shreds.iter().map(Shred::signature).dedup().count(), 1);
        for size in num_data_shreds..num_shreds {
            let mut shreds = shreds.clone();
            let mut removed_shreds = Vec::new();
            while shreds.len() > size {
                let index = rng.gen_range(0, shreds.len());
                removed_shreds.push(shreds.swap_remove(index));
            }
            shreds.shuffle(rng);
            // Should at least contain one coding shred.
            if shreds.iter().all(|shred| {
                matches!(
                    shred.common_header().shred_variant,
                    ShredVariant::MerkleData(_)
                )
            }) {
                assert_matches!(
                    recover(shreds),
                    Err(Error::ErasureError(TooFewParityShards))
                );
                continue;
            }
            let recovered_shreds = recover(shreds).unwrap();
            assert_eq!(size + recovered_shreds.len(), num_shreds);
            assert_eq!(recovered_shreds.len(), removed_shreds.len());
            removed_shreds.sort_by(|a, b| {
                if a.shred_type() == b.shred_type() {
                    a.index().cmp(&b.index())
                } else if a.shred_type() == ShredType::Data {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            });
            assert_eq!(recovered_shreds, removed_shreds);
        }
    }
}
