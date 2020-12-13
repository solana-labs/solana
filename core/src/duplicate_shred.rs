use crate::crds_value::sanitize_wallclock;
use itertools::Itertools;
use solana_ledger::{
    blockstore_meta::DuplicateSlotProof,
    shred::{Shred, ShredError, ShredType},
};
use solana_sdk::{
    clock::Slot,
    pubkey::Pubkey,
    sanitize::{Sanitize, SanitizeError},
};
use std::{
    collections::{hash_map::Entry, HashMap},
    convert::TryFrom,
};
use thiserror::Error;

const DUPLICATE_SHRED_HEADER_SIZE: usize = 63;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct DuplicateShred {
    pub(crate) from: Pubkey,
    pub(crate) wallclock: u64,
    slot: Slot,
    shred_index: u32,
    shred_type: ShredType,
    // Serialized DuplicateSlotProof split into chunks.
    num_chunks: u8,
    chunk_index: u8,
    #[serde(with = "serde_bytes")]
    chunk: Vec<u8>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DuplicateShredIndex {
    slot: Slot,
    shred_index: u32,
    shred_type: ShredType,
    num_chunks: u8,
    chunk_index: u8,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("data chunk mismatch")]
    DataChunkMismatch,
    #[error("decoding error")]
    DecodingError(std::io::Error),
    #[error("encoding error")]
    EncodingError(std::io::Error),
    #[error("invalid chunk index")]
    InvalidChunkIndex,
    #[error("invalid duplicate shreds")]
    InvalidDuplicateShreds,
    #[error("invalid duplicate slot proof")]
    InvalidDuplicateSlotProof,
    #[error("invalid size limit")]
    InvalidSizeLimit,
    #[error("invalid shred")]
    InvalidShred(#[from] ShredError),
    #[error("number of chunks mismatch")]
    NumChunksMismatch,
    #[error("missing data chunk")]
    MissingDataChunk,
    #[error("(de)serialization error")]
    SerializationError(#[from] bincode::Error),
    #[error("shred index mismatch")]
    ShredIndexMismatch,
    #[error("shred type mismatch")]
    ShredTypeMismatch,
    #[error("slot mismatch")]
    SlotMismatch,
    #[error("value out of bounds")]
    ValueOutOfBounds,
}

// Verifies that the two shreds can indicate duplicate proof.
fn check_shreds(shred1: &Shred, shred2: &Shred) -> Result<(), Error> {
    if shred1.slot() != shred2.slot() {
        Err(Error::SlotMismatch)
    } else if shred1.index() != shred2.index() {
        Err(Error::ShredIndexMismatch)
    } else if shred1.common_header.shred_type != shred2.common_header.shred_type {
        Err(Error::ShredTypeMismatch)
    } else if shred1.payload == shred2.payload {
        Err(Error::InvalidDuplicateShreds)
    } else {
        Ok(())
    }
}

/// Splits a DuplicateSlotProof into DuplicateShred
/// chunks with a size limit on each chunk.
pub fn from_duplicate_slot_proof<F>(
    proof: &DuplicateSlotProof,
    self_pubkey: Pubkey,
    wallclock: u64,
    max_size: usize, // Maximum serialized size of each DuplicateShred.
    encoder: F,
) -> Result<impl Iterator<Item = DuplicateShred>, Error>
where
    F: FnOnce(Vec<u8>) -> Result<Vec<u8>, std::io::Error>,
{
    if proof.shred1 == proof.shred2 {
        return Err(Error::InvalidDuplicateSlotProof);
    }
    let shred1 = Shred::new_from_serialized_shred(proof.shred1.clone())?;
    let shred2 = Shred::new_from_serialized_shred(proof.shred2.clone())?;
    check_shreds(&shred1, &shred2)?;
    let (slot, shred_index, shred_type) = (
        shred1.slot(),
        shred1.index(),
        shred1.common_header.shred_type,
    );
    let data = bincode::serialize(proof)?;
    let data = encoder(data).map_err(Error::EncodingError)?;
    let chunk_size = if DUPLICATE_SHRED_HEADER_SIZE < max_size {
        max_size - DUPLICATE_SHRED_HEADER_SIZE
    } else {
        return Err(Error::InvalidSizeLimit);
    };
    let chunks: Vec<_> = data.chunks(chunk_size).map(Vec::from).collect();
    let num_chunks = u8::try_from(chunks.len()).map_err(|_| Error::ValueOutOfBounds)?;
    let chunks = chunks
        .into_iter()
        .enumerate()
        .map(move |(i, chunk)| DuplicateShred {
            from: self_pubkey,
            wallclock,
            slot,
            shred_index,
            shred_type,
            num_chunks,
            chunk_index: i as u8,
            chunk,
        });
    Ok(chunks)
}

// Returns a predicate checking if a duplicate-shred chunk matches
// (slot, shred_index, shred_type) and has valid chunk_index.
fn check_chunk(
    slot: Slot,
    shred_index: u32,
    shred_type: ShredType,
    num_chunks: u8,
) -> impl Fn(&DuplicateShred) -> Result<(), Error> {
    move |dup| {
        if dup.slot != slot {
            Err(Error::SlotMismatch)
        } else if dup.shred_index != shred_index {
            Err(Error::ShredIndexMismatch)
        } else if dup.shred_type != shred_type {
            Err(Error::ShredTypeMismatch)
        } else if dup.num_chunks != num_chunks {
            Err(Error::NumChunksMismatch)
        } else if dup.chunk_index >= num_chunks {
            Err(Error::InvalidChunkIndex)
        } else {
            Ok(())
        }
    }
}

/// Reconstructs the duplicate shreds from chunks of DuplicateShred.
pub fn into_shreds<F, I>(chunks: I, decoder: F) -> Result<(Shred, Shred), Error>
where
    I: IntoIterator<Item = DuplicateShred>,
    F: FnOnce(Vec<u8>) -> Result<Vec<u8>, std::io::Error>,
{
    let mut chunks = chunks.into_iter();
    let DuplicateShred {
        slot,
        shred_index,
        shred_type,
        num_chunks,
        chunk_index,
        chunk,
        ..
    } = match chunks.next() {
        None => return Err(Error::InvalidDuplicateShreds),
        Some(chunk) => chunk,
    };
    let check_chunk = check_chunk(slot, shred_index, shred_type, num_chunks);
    let mut data = HashMap::new();
    data.insert(chunk_index, chunk);
    for chunk in chunks {
        check_chunk(&chunk)?;
        match data.entry(chunk.chunk_index) {
            Entry::Vacant(entry) => {
                entry.insert(chunk.chunk);
            }
            Entry::Occupied(entry) => {
                if *entry.get() != chunk.chunk {
                    return Err(Error::DataChunkMismatch);
                }
            }
        }
    }
    if data.len() != num_chunks as usize {
        return Err(Error::MissingDataChunk);
    }
    let data = (0..num_chunks).map(|k| data.remove(&k).unwrap());
    let data = decoder(data.concat()).map_err(Error::DecodingError)?;
    let proof: DuplicateSlotProof = bincode::deserialize(&data)?;
    if proof.shred1 == proof.shred2 {
        return Err(Error::InvalidDuplicateSlotProof);
    }
    let shred1 = Shred::new_from_serialized_shred(proof.shred1)?;
    let shred2 = Shred::new_from_serialized_shred(proof.shred2)?;
    if shred1.slot() != slot || shred2.slot() != slot {
        return Err(Error::SlotMismatch);
    }
    if shred1.index() != shred_index || shred2.index() != shred_index {
        return Err(Error::ShredIndexMismatch);
    }
    if shred1.common_header.shred_type != shred_type
        || shred2.common_header.shred_type != shred_type
    {
        return Err(Error::ShredTypeMismatch);
    }
    Ok((shred1, shred2))
}

impl Sanitize for DuplicateShred {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        sanitize_wallclock(self.wallclock)?;
        if self.chunk_index >= self.num_chunks {
            return Err(SanitizeError::IndexOutOfBounds);
        }
        self.from.sanitize()
    }
}

impl From<&DuplicateShred> for DuplicateShredIndex {
    fn from(shred: &DuplicateShred) -> Self {
        Self {
            slot: shred.slot,
            shred_index: shred.shred_index,
            shred_type: shred.shred_type,
            num_chunks: shred.num_chunks,
            chunk_index: shred.chunk_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplicate_shred_header_size() {
        let dup = DuplicateShred {
            from: Pubkey::new_unique(),
            wallclock: u64::MAX,
            slot: Slot::MAX,
            shred_index: u32::MAX,
            shred_type: ShredType(u8::MAX),
            num_chunks: u8::MAX,
            chunk_index: u8::MAX,
            chunk: Vec::default(),
        };
        assert_eq!(
            bincode::serialize(&dup).unwrap().len(),
            DUPLICATE_SHRED_HEADER_SIZE
        );
        assert_eq!(
            bincode::serialized_size(&dup).unwrap(),
            DUPLICATE_SHRED_HEADER_SIZE as u64
        );
    }
}
