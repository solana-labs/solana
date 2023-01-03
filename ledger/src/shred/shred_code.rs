use {
    crate::{
        shred::{
            common::dispatch,
            legacy, merkle,
            traits::{Shred, ShredCode as ShredCodeTrait},
            CodingShredHeader, Error, ShredCommonHeader, ShredType, SignedData,
            DATA_SHREDS_PER_FEC_BLOCK, MAX_DATA_SHREDS_PER_SLOT, SIZE_OF_NONCE,
        },
        shredder::ERASURE_BATCH_SIZE,
    },
    solana_sdk::{clock::Slot, packet::PACKET_DATA_SIZE, signature::Signature},
    static_assertions::const_assert_eq,
};

const_assert_eq!(MAX_CODE_SHREDS_PER_SLOT, 32_768 * 17);
pub(crate) const MAX_CODE_SHREDS_PER_SLOT: usize =
    MAX_DATA_SHREDS_PER_SLOT * (ERASURE_BATCH_SIZE[1] - 1);

const_assert_eq!(ShredCode::SIZE_OF_PAYLOAD, 1228);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShredCode {
    Legacy(legacy::ShredCode),
    Merkle(merkle::ShredCode),
}

impl ShredCode {
    pub(super) const SIZE_OF_PAYLOAD: usize = PACKET_DATA_SIZE - SIZE_OF_NONCE;

    dispatch!(fn coding_header(&self) -> &CodingShredHeader);

    dispatch!(pub(super) fn common_header(&self) -> &ShredCommonHeader);
    dispatch!(pub(super) fn erasure_shard(self) -> Result<Vec<u8>, Error>);
    dispatch!(pub(super) fn erasure_shard_as_slice(&self) -> Result<&[u8], Error>);
    dispatch!(pub(super) fn erasure_shard_index(&self) -> Result<usize, Error>);
    dispatch!(pub(super) fn first_coding_index(&self) -> Option<u32>);
    dispatch!(pub(super) fn into_payload(self) -> Vec<u8>);
    dispatch!(pub(super) fn payload(&self) -> &Vec<u8>);
    dispatch!(pub(super) fn sanitize(&self) -> Result<(), Error>);
    dispatch!(pub(super) fn set_signature(&mut self, signature: Signature));

    // Only for tests.
    dispatch!(pub(super) fn set_index(&mut self, index: u32));
    dispatch!(pub(super) fn set_slot(&mut self, slot: Slot));

    pub(super) fn signed_data(&self) -> Result<SignedData, Error> {
        match self {
            Self::Legacy(shred) => Ok(SignedData::Chunk(shred.signed_data()?)),
            Self::Merkle(shred) => Ok(SignedData::MerkleRoot(shred.signed_data()?)),
        }
    }

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
        Self::from(legacy::ShredCode::new_from_parity_shard(
            slot,
            index,
            parity_shard,
            fec_set_index,
            num_data_shreds,
            num_coding_shreds,
            position,
            version,
        ))
    }

    pub(super) fn num_data_shreds(&self) -> u16 {
        self.coding_header().num_data_shreds
    }

    pub(super) fn num_coding_shreds(&self) -> u16 {
        self.coding_header().num_coding_shreds
    }

    // Returns true if the erasure coding of the two shreds mismatch.
    pub(super) fn erasure_mismatch(&self, other: &ShredCode) -> bool {
        match (self, other) {
            (Self::Legacy(shred), Self::Legacy(other)) => erasure_mismatch(shred, other),
            (Self::Legacy(_), Self::Merkle(_)) => true,
            (Self::Merkle(_), Self::Legacy(_)) => true,
            (Self::Merkle(shred), Self::Merkle(other)) => {
                // Merkle shreds within the same erasure batch have the same
                // merkle root. The root of the merkle tree is signed. So
                // either the signatures match or one fails sigverify.
                erasure_mismatch(shred, other)
                    || shred.common_header().signature != other.common_header().signature
            }
        }
    }
}

impl From<legacy::ShredCode> for ShredCode {
    fn from(shred: legacy::ShredCode) -> Self {
        Self::Legacy(shred)
    }
}

impl From<merkle::ShredCode> for ShredCode {
    fn from(shred: merkle::ShredCode) -> Self {
        Self::Merkle(shred)
    }
}

#[inline]
pub(super) fn erasure_shard_index<T: ShredCodeTrait>(shred: &T) -> Option<usize> {
    // Assert that the last shred index in the erasure set does not
    // overshoot MAX_{DATA,CODE}_SHREDS_PER_SLOT.
    let common_header = shred.common_header();
    let coding_header = shred.coding_header();
    if common_header
        .fec_set_index
        .checked_add(u32::from(coding_header.num_data_shreds.checked_sub(1)?))? as usize
        >= MAX_DATA_SHREDS_PER_SLOT
    {
        return None;
    }
    if shred
        .first_coding_index()?
        .checked_add(u32::from(coding_header.num_coding_shreds.checked_sub(1)?))? as usize
        >= MAX_CODE_SHREDS_PER_SLOT
    {
        return None;
    }
    let num_data_shreds = usize::from(coding_header.num_data_shreds);
    let num_coding_shreds = usize::from(coding_header.num_coding_shreds);
    let position = usize::from(coding_header.position);
    let fec_set_size = num_data_shreds.checked_add(num_coding_shreds)?;
    let index = position.checked_add(num_data_shreds)?;
    (index < fec_set_size).then_some(index)
}

pub(super) fn sanitize<T: ShredCodeTrait>(shred: &T) -> Result<(), Error> {
    if shred.payload().len() != T::SIZE_OF_PAYLOAD {
        return Err(Error::InvalidPayloadSize(shred.payload().len()));
    }
    let common_header = shred.common_header();
    let coding_header = shred.coding_header();
    if common_header.index as usize >= MAX_CODE_SHREDS_PER_SLOT {
        return Err(Error::InvalidShredIndex(
            ShredType::Code,
            common_header.index,
        ));
    }
    let num_coding_shreds = usize::from(coding_header.num_coding_shreds);
    if num_coding_shreds > 8 * DATA_SHREDS_PER_FEC_BLOCK {
        return Err(Error::InvalidNumCodingShreds(
            coding_header.num_coding_shreds,
        ));
    }
    let _shard_index = shred.erasure_shard_index()?;
    let _erasure_shard = shred.erasure_shard_as_slice()?;
    Ok(())
}

pub(super) fn erasure_mismatch<T: ShredCodeTrait>(shred: &T, other: &T) -> bool {
    let CodingShredHeader {
        num_data_shreds,
        num_coding_shreds,
        position: _,
    } = shred.coding_header();
    *num_coding_shreds != other.coding_header().num_coding_shreds
        || *num_data_shreds != other.coding_header().num_data_shreds
        || shred.first_coding_index() != other.first_coding_index()
}
