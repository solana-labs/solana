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
    num::TryFromIntError,
};
use thiserror::Error;

const DUPLICATE_SHRED_HEADER_SIZE: usize = 63;

/// Function returning leader at a given slot.
pub trait LeaderScheduleFn: FnOnce(Slot) -> Option<Pubkey> {}
impl<F> LeaderScheduleFn for F where F: FnOnce(Slot) -> Option<Pubkey> {}

#[derive(Clone, Debug, PartialEq, AbiExample, Deserialize, Serialize)]
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
    #[error("invalid signature")]
    InvalidSignature,
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
    #[error("type conversion error")]
    TryFromIntError(#[from] TryFromIntError),
    #[error("unknown slot leader")]
    UnknownSlotLeader,
}

// Asserts that the two shreds can indicate duplicate proof for
// the same triplet of (slot, shred-index, and shred-type_), and
// that they have valid signatures from the slot leader.
fn check_shreds(
    leader: impl LeaderScheduleFn,
    shred1: &Shred,
    shred2: &Shred,
) -> Result<(), Error> {
    if shred1.slot() != shred2.slot() {
        Err(Error::SlotMismatch)
    } else if shred1.index() != shred2.index() {
        Err(Error::ShredIndexMismatch)
    } else if shred1.common_header.shred_type != shred2.common_header.shred_type {
        Err(Error::ShredTypeMismatch)
    } else if shred1.payload == shred2.payload {
        Err(Error::InvalidDuplicateShreds)
    } else {
        let slot_leader = leader(shred1.slot()).ok_or(Error::UnknownSlotLeader)?;
        if !shred1.verify(&slot_leader) || !shred2.verify(&slot_leader) {
            Err(Error::InvalidSignature)
        } else {
            Ok(())
        }
    }
}

/// Splits a DuplicateSlotProof into DuplicateShred
/// chunks with a size limit on each chunk.
pub fn from_duplicate_slot_proof(
    proof: &DuplicateSlotProof,
    self_pubkey: Pubkey, // Pubkey of my node broadcasting crds value.
    leader: impl LeaderScheduleFn,
    wallclock: u64,
    max_size: usize, // Maximum serialized size of each DuplicateShred.
    encoder: impl FnOnce(Vec<u8>) -> Result<Vec<u8>, std::io::Error>,
) -> Result<impl Iterator<Item = DuplicateShred>, Error> {
    if proof.shred1 == proof.shred2 {
        return Err(Error::InvalidDuplicateSlotProof);
    }
    let shred1 = Shred::new_from_serialized_shred(proof.shred1.clone())?;
    let shred2 = Shred::new_from_serialized_shred(proof.shred2.clone())?;
    check_shreds(leader, &shred1, &shred2)?;
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
    let num_chunks = u8::try_from(chunks.len())?;
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
pub fn into_shreds(
    chunks: impl IntoIterator<Item = DuplicateShred>,
    leader: impl LeaderScheduleFn,
    decoder: impl FnOnce(Vec<u8>) -> Result<Vec<u8>, std::io::Error>,
) -> Result<(Shred, Shred), Error> {
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
    let slot_leader = leader(slot).ok_or(Error::UnknownSlotLeader)?;
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
        Err(Error::SlotMismatch)
    } else if shred1.index() != shred_index || shred2.index() != shred_index {
        Err(Error::ShredIndexMismatch)
    } else if shred1.common_header.shred_type != shred_type
        || shred2.common_header.shred_type != shred_type
    {
        Err(Error::ShredTypeMismatch)
    } else if shred1.payload == shred2.payload {
        Err(Error::InvalidDuplicateShreds)
    } else if !shred1.verify(&slot_leader) || !shred2.verify(&slot_leader) {
        Err(Error::InvalidSignature)
    } else {
        Ok((shred1, shred2))
    }
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
    use rand::Rng;
    use solana_ledger::{entry::Entry, shred::Shredder};
    use solana_sdk::{hash, signature::Keypair, signature::Signer, system_transaction};
    use std::sync::Arc;

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

    fn new_rand_shred<R: Rng>(rng: &mut R, next_shred_index: u32, shredder: &Shredder) -> Shred {
        let entries: Vec<_> = std::iter::repeat_with(|| {
            let tx = system_transaction::transfer(
                &Keypair::new(),       // from
                &Pubkey::new_unique(), // to
                rng.gen(),             // lamports
                hash::new_rand(rng),   // recent blockhash
            );
            Entry::new(
                &hash::new_rand(rng), // prev_hash
                1,                    // num_hashes,
                vec![tx],             // transactions
            )
        })
        .take(5)
        .collect();
        let (mut data_shreds, _coding_shreds, _last_shred_index) = shredder.entries_to_shreds(
            &entries,
            true, // is_last_in_slot
            next_shred_index,
        );
        data_shreds.swap_remove(0)
    }

    #[test]
    fn test_duplicate_shred_round_trip() {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, fec_rate, reference_tick, version) =
            (53084024, 53084023, 0.0, 0, 0);
        let shredder = Shredder::new(
            slot,
            parent_slot,
            fec_rate,
            leader.clone(),
            reference_tick,
            version,
        )
        .unwrap();
        let next_shred_index = rng.gen();
        let shred1 = new_rand_shred(&mut rng, next_shred_index, &shredder);
        let shred2 = new_rand_shred(&mut rng, next_shred_index, &shredder);
        let leader = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let proof = DuplicateSlotProof {
            shred1: shred1.payload.clone(),
            shred2: shred2.payload.clone(),
        };
        let chunks: Vec<_> = from_duplicate_slot_proof(
            &proof,
            Pubkey::new_unique(), // self_pubkey
            leader,
            rng.gen(), // wallclock
            512,       // max_size
            Ok,        // encoder
        )
        .unwrap()
        .collect();
        assert!(chunks.len() > 4);
        let (shred3, shred4) = into_shreds(chunks, leader, Ok).unwrap();
        assert_eq!(shred1, shred3);
        assert_eq!(shred2, shred4);
    }
}
