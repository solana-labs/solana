use {
    crate::shred::{
        self,
        common::dispatch,
        legacy, merkle,
        traits::{Shred as _, ShredData as ShredDataTrait},
        DataShredHeader, Error, ShredCommonHeader, ShredFlags, ShredType, ShredVariant, SignedData,
        MAX_DATA_SHREDS_PER_SLOT,
    },
    solana_sdk::{clock::Slot, hash::Hash, signature::Signature},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShredData {
    Legacy(legacy::ShredData),
    Merkle(merkle::ShredData),
}

impl ShredData {
    dispatch!(fn data_header(&self) -> &DataShredHeader);

    dispatch!(pub(super) fn common_header(&self) -> &ShredCommonHeader);
    dispatch!(pub(super) fn data(&self) -> Result<&[u8], Error>);
    dispatch!(pub(super) fn erasure_shard(self) -> Result<Vec<u8>, Error>);
    dispatch!(pub(super) fn erasure_shard_as_slice(&self) -> Result<&[u8], Error>);
    dispatch!(pub(super) fn erasure_shard_index(&self) -> Result<usize, Error>);
    dispatch!(pub(super) fn into_payload(self) -> Vec<u8>);
    dispatch!(pub(super) fn parent(&self) -> Result<Slot, Error>);
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

    pub(super) fn merkle_root(&self) -> Result<Hash, Error> {
        match self {
            Self::Legacy(_) => Err(Error::InvalidShredType),
            Self::Merkle(shred) => shred.merkle_root(),
        }
    }

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
        Self::from(legacy::ShredData::new_from_data(
            slot,
            index,
            parent_offset,
            data,
            flags,
            reference_tick,
            version,
            fec_set_index,
        ))
    }

    pub(super) fn last_in_slot(&self) -> bool {
        let flags = self.data_header().flags;
        flags.contains(ShredFlags::LAST_SHRED_IN_SLOT)
    }

    pub(super) fn data_complete(&self) -> bool {
        let flags = self.data_header().flags;
        flags.contains(ShredFlags::DATA_COMPLETE_SHRED)
    }

    pub(super) fn reference_tick(&self) -> u8 {
        let flags = self.data_header().flags;
        (flags & ShredFlags::SHRED_TICK_REFERENCE_MASK).bits()
    }

    // Possibly trimmed payload;
    // Should only be used when storing shreds to blockstore.
    pub(super) fn bytes_to_store(&self) -> &[u8] {
        match self {
            Self::Legacy(shred) => shred.bytes_to_store(),
            Self::Merkle(shred) => shred.payload(),
        }
    }

    // Possibly zero pads bytes stored in blockstore.
    pub(crate) fn resize_stored_shred(shred: Vec<u8>) -> Result<Vec<u8>, Error> {
        match shred::layout::get_shred_variant(&shred)? {
            ShredVariant::LegacyCode | ShredVariant::MerkleCode(_) => Err(Error::InvalidShredType),
            ShredVariant::MerkleData(_) => {
                if shred.len() != merkle::ShredData::SIZE_OF_PAYLOAD {
                    return Err(Error::InvalidPayloadSize(shred.len()));
                }
                Ok(shred)
            }
            ShredVariant::LegacyData => legacy::ShredData::resize_stored_shred(shred),
        }
    }

    // Maximum size of ledger data that can be embedded in a data-shred.
    // merkle_proof_size is the number of merkle proof entries.
    // None indicates a legacy data-shred.
    pub fn capacity(merkle_proof_size: Option<u8>) -> Result<usize, Error> {
        match merkle_proof_size {
            None => Ok(legacy::ShredData::CAPACITY),
            Some(proof_size) => merkle::ShredData::capacity(proof_size),
        }
    }

    // Only for tests.
    pub(super) fn set_last_in_slot(&mut self) {
        match self {
            Self::Legacy(shred) => shred.set_last_in_slot(),
            Self::Merkle(_) => panic!("Not Implemented!"),
        }
    }
}

impl From<legacy::ShredData> for ShredData {
    fn from(shred: legacy::ShredData) -> Self {
        Self::Legacy(shred)
    }
}

impl From<merkle::ShredData> for ShredData {
    fn from(shred: merkle::ShredData) -> Self {
        Self::Merkle(shred)
    }
}

#[inline]
pub(super) fn erasure_shard_index<T: ShredDataTrait>(shred: &T) -> Option<usize> {
    let fec_set_index = shred.common_header().fec_set_index;
    let index = shred.common_header().index.checked_sub(fec_set_index)?;
    usize::try_from(index).ok()
}

pub(super) fn sanitize<T: ShredDataTrait>(shred: &T) -> Result<(), Error> {
    if shred.payload().len() != T::SIZE_OF_PAYLOAD {
        return Err(Error::InvalidPayloadSize(shred.payload().len()));
    }
    let common_header = shred.common_header();
    let data_header = shred.data_header();
    if common_header.index as usize >= MAX_DATA_SHREDS_PER_SLOT {
        return Err(Error::InvalidShredIndex(
            ShredType::Data,
            common_header.index,
        ));
    }
    let flags = data_header.flags;
    if flags.intersects(ShredFlags::LAST_SHRED_IN_SLOT)
        && !flags.contains(ShredFlags::DATA_COMPLETE_SHRED)
    {
        return Err(Error::InvalidShredFlags(data_header.flags.bits()));
    }
    let _data = shred.data()?;
    let _parent = shred.parent()?;
    let _shard_index = shred.erasure_shard_index()?;
    let _erasure_shard = shred.erasure_shard_as_slice()?;
    Ok(())
}
