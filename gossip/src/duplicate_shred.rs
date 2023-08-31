use {
    crate::crds_value::sanitize_wallclock,
    itertools::Itertools,
    solana_ledger::{
        blockstore::BlockstoreError,
        blockstore_meta::{DuplicateSlotProof, ErasureMeta},
        shred::{self, Shred, ShredType},
    },
    solana_sdk::{
        clock::Slot,
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        convert::TryFrom,
        num::TryFromIntError,
    },
    thiserror::Error,
};

const DUPLICATE_SHRED_HEADER_SIZE: usize = 63;

pub(crate) type DuplicateShredIndex = u16;
pub(crate) const MAX_DUPLICATE_SHREDS: DuplicateShredIndex = 512;

#[derive(Clone, Debug, PartialEq, Eq, AbiExample, Deserialize, Serialize)]
pub struct DuplicateShred {
    pub(crate) from: Pubkey,
    pub(crate) wallclock: u64,
    pub(crate) slot: Slot,
    _unused: u32,
    shred_type: ShredType,
    // Serialized DuplicateSlotProof split into chunks.
    num_chunks: u8,
    chunk_index: u8,
    #[serde(with = "serde_bytes")]
    chunk: Vec<u8>,
}

impl DuplicateShred {
    #[inline]
    pub(crate) fn num_chunks(&self) -> u8 {
        self.num_chunks
    }

    #[inline]
    pub(crate) fn chunk_index(&self) -> u8 {
        self.chunk_index
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("block store save error")]
    BlockstoreInsertFailed(#[from] BlockstoreError),
    #[error("data chunk mismatch")]
    DataChunkMismatch,
    #[error("invalid chunk_index: {chunk_index}, num_chunks: {num_chunks}")]
    InvalidChunkIndex { chunk_index: u8, num_chunks: u8 },
    #[error("invalid duplicate shreds")]
    InvalidDuplicateShreds,
    #[error("invalid duplicate slot proof")]
    InvalidDuplicateSlotProof,
    #[error("invalid erasure meta conflict")]
    InvalidErasureMetaConflict,
    #[error("invalid last index conflict")]
    InvalidLastIndexConflict,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid size limit")]
    InvalidSizeLimit,
    #[error(transparent)]
    InvalidShred(#[from] shred::Error),
    #[error("number of chunks mismatch")]
    NumChunksMismatch,
    #[error("missing data chunk")]
    MissingDataChunk,
    #[error("(de)serialization error")]
    SerializationError(#[from] bincode::Error),
    #[error("shred type mismatch")]
    ShredTypeMismatch,
    #[error("slot mismatch")]
    SlotMismatch,
    #[error("type conversion error")]
    TryFromIntError(#[from] TryFromIntError),
    #[error("unknown slot leader: {0}")]
    UnknownSlotLeader(Slot),
}

/// Check that `shred1` and `shred2` indicate a valid duplicate proof
///     - Must be for the same slot
///     - Must have the same `shred_type`
///     - Must both sigverify for the correct leader
///     - If `shred1` and `shred2` share the same index they must be not equal
///     - If `shred1` and `shred2` do not share the same index and are data shreds
///       verify that they indicate an index conflict. One of them must be the
///       LAST_SHRED_IN_SLOT, however the other shred must have a higher index.
///     - If `shred1` and `shred2` do not share the same index and are coding shreds
///       verify that they have conflicting erasure metas
fn check_shreds<F>(leader_schedule: Option<F>, shred1: &Shred, shred2: &Shred) -> Result<(), Error>
where
    F: FnOnce(Slot) -> Option<Pubkey>,
{
    if shred1.slot() != shred2.slot() {
        return Err(Error::SlotMismatch);
    }

    if shred1.shred_type() != shred2.shred_type() {
        return Err(Error::ShredTypeMismatch);
    }

    if let Some(leader_schedule) = leader_schedule {
        let slot_leader =
            leader_schedule(shred1.slot()).ok_or(Error::UnknownSlotLeader(shred1.slot()))?;
        if !shred1.verify(&slot_leader) || !shred2.verify(&slot_leader) {
            return Err(Error::InvalidSignature);
        }
    }

    if shred1.index() == shred2.index() {
        if shred1.payload() != shred2.payload() {
            return Ok(());
        }
        return Err(Error::InvalidDuplicateShreds);
    }

    if shred1.shred_type() == ShredType::Data {
        if shred1.last_in_slot() && shred2.index() > shred1.index() {
            return Ok(());
        }
        if shred2.last_in_slot() && shred1.index() > shred2.index() {
            return Ok(());
        }
        return Err(Error::InvalidLastIndexConflict);
    }

    // This mirrors the current logic in blockstore to detect coding shreds with conflicting
    // erasure sets. However this is not technically exhaustive, as any 2 shreds with
    // different but overlapping erasure sets can be considered duplicate and need not be
    // a part of the same fec set. Further work to enhance detection is planned in
    // https://github.com/solana-labs/solana/issues/33037
    if shred1.fec_set_index() == shred2.fec_set_index()
        && !ErasureMeta::check_erasure_consistency(shred1, shred2)
    {
        return Ok(());
    }
    Err(Error::InvalidErasureMetaConflict)
}

pub(crate) fn from_shred<F>(
    shred: Shred,
    self_pubkey: Pubkey, // Pubkey of my node broadcasting crds value.
    other_payload: Vec<u8>,
    leader_schedule: Option<F>,
    wallclock: u64,
    max_size: usize, // Maximum serialized size of each DuplicateShred.
) -> Result<impl Iterator<Item = DuplicateShred>, Error>
where
    F: FnOnce(Slot) -> Option<Pubkey>,
{
    if shred.payload() == &other_payload {
        return Err(Error::InvalidDuplicateShreds);
    }
    let other_shred = Shred::new_from_serialized_shred(other_payload)?;
    check_shreds(leader_schedule, &shred, &other_shred)?;
    let (slot, shred_type) = (shred.slot(), shred.shred_type());
    let proof = DuplicateSlotProof {
        shred1: shred.into_payload(),
        shred2: other_shred.into_payload(),
    };
    let data = bincode::serialize(&proof)?;
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
            shred_type,
            num_chunks,
            chunk_index: i as u8,
            chunk,
            _unused: 0,
        });
    Ok(chunks)
}

// Returns a predicate checking if a duplicate-shred chunk matches
// (slot, shred_type) and has valid chunk_index.
fn check_chunk(
    slot: Slot,
    shred_type: ShredType,
    num_chunks: u8,
) -> impl Fn(&DuplicateShred) -> Result<(), Error> {
    move |dup| {
        if dup.slot != slot {
            Err(Error::SlotMismatch)
        } else if dup.shred_type != shred_type {
            Err(Error::ShredTypeMismatch)
        } else if dup.num_chunks != num_chunks {
            Err(Error::NumChunksMismatch)
        } else if dup.chunk_index >= num_chunks {
            Err(Error::InvalidChunkIndex {
                chunk_index: dup.chunk_index,
                num_chunks,
            })
        } else {
            Ok(())
        }
    }
}

/// Reconstructs the duplicate shreds from chunks of DuplicateShred.
pub(crate) fn into_shreds(
    slot_leader: &Pubkey,
    chunks: impl IntoIterator<Item = DuplicateShred>,
) -> Result<(Shred, Shred), Error> {
    let mut chunks = chunks.into_iter();
    let DuplicateShred {
        slot,
        shred_type,
        num_chunks,
        chunk_index,
        chunk,
        ..
    } = chunks.next().ok_or(Error::InvalidDuplicateShreds)?;
    let check_chunk = check_chunk(slot, shred_type, num_chunks);
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
    let data = (0..num_chunks).map(|k| data.remove(&k).unwrap()).concat();
    let proof: DuplicateSlotProof = bincode::deserialize(&data)?;
    if proof.shred1 == proof.shred2 {
        return Err(Error::InvalidDuplicateSlotProof);
    }
    let shred1 = Shred::new_from_serialized_shred(proof.shred1)?;
    let shred2 = Shred::new_from_serialized_shred(proof.shred2)?;
    if shred1.slot() != slot || shred2.slot() != slot {
        Err(Error::SlotMismatch)
    } else if shred1.shred_type() != shred_type || shred2.shred_type() != shred_type {
        Err(Error::ShredTypeMismatch)
    } else {
        check_shreds(Some(|_| Some(slot_leader).copied()), &shred1, &shred2)?;
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

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        rand::Rng,
        solana_entry::entry::Entry,
        solana_ledger::shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            system_transaction,
        },
        std::sync::Arc,
        test_case::test_case,
    };

    #[test]
    fn test_duplicate_shred_header_size() {
        let dup = DuplicateShred {
            from: Pubkey::new_unique(),
            wallclock: u64::MAX,
            slot: Slot::MAX,
            shred_type: ShredType::Data,
            num_chunks: u8::MAX,
            chunk_index: u8::MAX,
            chunk: Vec::default(),
            _unused: 0,
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

    pub(crate) fn new_rand_shred<R: Rng>(
        rng: &mut R,
        next_shred_index: u32,
        shredder: &Shredder,
        keypair: &Keypair,
    ) -> Shred {
        let (mut data_shreds, _) = new_rand_shreds(
            rng,
            next_shred_index,
            next_shred_index,
            5,
            true,
            shredder,
            keypair,
            true,
        );
        data_shreds.pop().unwrap()
    }

    fn new_rand_data_shred<R: Rng>(
        rng: &mut R,
        next_shred_index: u32,
        shredder: &Shredder,
        keypair: &Keypair,
        merkle_variant: bool,
        is_last_in_slot: bool,
    ) -> Shred {
        let (mut data_shreds, _) = new_rand_shreds(
            rng,
            next_shred_index,
            next_shred_index,
            5,
            merkle_variant,
            shredder,
            keypair,
            is_last_in_slot,
        );
        data_shreds.pop().unwrap()
    }

    fn new_rand_coding_shreds<R: Rng>(
        rng: &mut R,
        next_shred_index: u32,
        num_entries: usize,
        shredder: &Shredder,
        keypair: &Keypair,
        merkle_variant: bool,
    ) -> Vec<Shred> {
        let (_, coding_shreds) = new_rand_shreds(
            rng,
            next_shred_index,
            next_shred_index,
            num_entries,
            merkle_variant,
            shredder,
            keypair,
            true,
        );
        coding_shreds
    }

    fn new_rand_shreds<R: Rng>(
        rng: &mut R,
        next_shred_index: u32,
        next_code_index: u32,
        num_entries: usize,
        merkle_variant: bool,
        shredder: &Shredder,
        keypair: &Keypair,
        is_last_in_slot: bool,
    ) -> (Vec<Shred>, Vec<Shred>) {
        let entries: Vec<_> = std::iter::repeat_with(|| {
            let tx = system_transaction::transfer(
                &Keypair::new(),       // from
                &Pubkey::new_unique(), // to
                rng.gen(),             // lamports
                Hash::new_unique(),    // recent blockhash
            );
            Entry::new(
                &Hash::new_unique(), // prev_hash
                1,                   // num_hashes,
                vec![tx],            // transactions
            )
        })
        .take(num_entries)
        .collect();
        shredder.entries_to_shreds(
            keypair,
            &entries,
            is_last_in_slot,
            next_shred_index,
            next_code_index, // next_code_index
            merkle_variant,
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        )
    }

    fn from_shred_bypass_checks(
        shred: Shred,
        self_pubkey: Pubkey, // Pubkey of my node broadcasting crds value.
        other_shred: Shred,
        wallclock: u64,
        max_size: usize, // Maximum serialized size of each DuplicateShred.
    ) -> Result<impl Iterator<Item = DuplicateShred>, Error> {
        let (slot, shred_type) = (shred.slot(), shred.shred_type());
        let proof = DuplicateSlotProof {
            shred1: shred.into_payload(),
            shred2: other_shred.into_payload(),
        };
        let data = bincode::serialize(&proof)?;
        let chunk_size = max_size - DUPLICATE_SHRED_HEADER_SIZE;
        let chunks: Vec<_> = data.chunks(chunk_size).map(Vec::from).collect();
        let num_chunks = u8::try_from(chunks.len())?;
        let chunks = chunks
            .into_iter()
            .enumerate()
            .map(move |(i, chunk)| DuplicateShred {
                from: self_pubkey,
                wallclock,
                slot,
                shred_type,
                num_chunks,
                chunk_index: i as u8,
                chunk,
                _unused: 0,
            });
        Ok(chunks)
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_duplicate_shred_round_trip(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..32_000);
        let shred1 = new_rand_data_shred(
            &mut rng,
            next_shred_index,
            &shredder,
            &leader,
            merkle_variant,
            true,
        );
        let shred2 = new_rand_data_shred(
            &mut rng,
            next_shred_index,
            &shredder,
            &leader,
            merkle_variant,
            true,
        );
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let chunks: Vec<_> = from_shred(
            shred1.clone(),
            Pubkey::new_unique(), // self_pubkey
            shred2.payload().clone(),
            Some(leader_schedule),
            rng.gen(), // wallclock
            512,       // max_size
        )
        .unwrap()
        .collect();
        assert!(chunks.len() > 4);
        let (shred3, shred4) = into_shreds(&leader.pubkey(), chunks).unwrap();
        assert_eq!(shred1, shred3);
        assert_eq!(shred2, shred4);
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_duplicate_shred_invalid(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..32_000);
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let data_shred = new_rand_data_shred(
            &mut rng,
            next_shred_index,
            &shredder,
            &leader,
            merkle_variant,
            true,
        );
        let coding_shreds = new_rand_coding_shreds(
            &mut rng,
            next_shred_index,
            10,
            &shredder,
            &leader,
            merkle_variant,
        );
        let test_cases = vec![
            // Same data_shred
            (data_shred.clone(), data_shred),
            // Same coding_shred
            (coding_shreds[0].clone(), coding_shreds[0].clone()),
        ];
        for (shred1, shred2) in test_cases.into_iter() {
            assert_matches!(
                from_shred(
                    shred1.clone(),
                    Pubkey::new_unique(), // self_pubkey
                    shred2.payload().clone(),
                    Some(leader_schedule),
                    rng.gen(), // wallclock
                    512,       // max_size
                )
                .err()
                .unwrap(),
                Error::InvalidDuplicateShreds
            );

            let chunks: Vec<_> = from_shred_bypass_checks(
                shred1.clone(),
                Pubkey::new_unique(), // self_pubkey
                shred2.clone(),
                rng.gen(), // wallclock
                512,       // max_size
            )
            .unwrap()
            .collect();
            assert!(chunks.len() > 4);

            assert_matches!(
                into_shreds(&leader.pubkey(), chunks).err().unwrap(),
                Error::InvalidDuplicateSlotProof
            );
        }
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_latest_index_conflict_round_trip(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..31_000);
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let test_cases = vec![
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 1,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 1,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 100,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 100,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
            ),
        ];
        for (shred1, shred2) in test_cases.into_iter() {
            let chunks: Vec<_> = from_shred(
                shred1.clone(),
                Pubkey::new_unique(), // self_pubkey
                shred2.payload().clone(),
                Some(leader_schedule),
                rng.gen(), // wallclock
                512,       // max_size
            )
            .unwrap()
            .collect();
            assert!(chunks.len() > 4);
            let (shred3, shred4) = into_shreds(&leader.pubkey(), chunks).unwrap();
            assert_eq!(shred1, shred3);
            assert_eq!(shred2, shred4);
        }
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_latest_index_conflict_invalid(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..31_000);
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let test_cases = vec![
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 1,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 1,
                    &shredder,
                    &leader,
                    merkle_variant,
                    true,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 100,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
            ),
            (
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
                new_rand_data_shred(
                    &mut rng,
                    next_shred_index + 100,
                    &shredder,
                    &leader,
                    merkle_variant,
                    false,
                ),
            ),
        ];
        for (shred1, shred2) in test_cases.into_iter() {
            assert_matches!(
                from_shred(
                    shred1.clone(),
                    Pubkey::new_unique(), // self_pubkey
                    shred2.payload().clone(),
                    Some(leader_schedule),
                    rng.gen(), // wallclock
                    512,       // max_size
                )
                .err()
                .unwrap(),
                Error::InvalidLastIndexConflict
            );

            let chunks: Vec<_> = from_shred_bypass_checks(
                shred1.clone(),
                Pubkey::new_unique(), // self_pubkey
                shred2.clone(),
                rng.gen(), // wallclock
                512,       // max_size
            )
            .unwrap()
            .collect();
            assert!(chunks.len() > 4);

            assert_matches!(
                into_shreds(&leader.pubkey(), chunks).err().unwrap(),
                Error::InvalidLastIndexConflict
            );
        }
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_erasure_meta_conflict_round_trip(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..31_000);
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let coding_shreds = new_rand_coding_shreds(
            &mut rng,
            next_shred_index,
            10,
            &shredder,
            &leader,
            merkle_variant,
        );
        let coding_shreds_bigger = new_rand_coding_shreds(
            &mut rng,
            next_shred_index,
            13,
            &shredder,
            &leader,
            merkle_variant,
        );
        let coding_shreds_smaller = new_rand_coding_shreds(
            &mut rng,
            next_shred_index,
            7,
            &shredder,
            &leader,
            merkle_variant,
        );

        // Same fec-set, different index, different erasure meta
        let test_cases = vec![
            (coding_shreds[0].clone(), coding_shreds_bigger[1].clone()),
            (coding_shreds[0].clone(), coding_shreds_smaller[1].clone()),
        ];
        for (shred1, shred2) in test_cases.into_iter() {
            let chunks: Vec<_> = from_shred(
                shred1.clone(),
                Pubkey::new_unique(), // self_pubkey
                shred2.payload().clone(),
                Some(leader_schedule),
                rng.gen(), // wallclock
                512,       // max_size
            )
            .unwrap()
            .collect();
            assert!(chunks.len() > 4);
            let (shred3, shred4) = into_shreds(&leader.pubkey(), chunks).unwrap();
            assert_eq!(shred1, shred3);
            assert_eq!(shred2, shred4);
        }
    }

    #[test_case(true ; "merkle")]
    #[test_case(false ; "legacy")]
    fn test_erasure_meta_conflict_invalid(merkle_variant: bool) {
        let mut rng = rand::thread_rng();
        let leader = Arc::new(Keypair::new());
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = rng.gen_range(0..31_000);
        let leader_schedule = |s| {
            if s == slot {
                Some(leader.pubkey())
            } else {
                None
            }
        };
        let coding_shreds = new_rand_coding_shreds(
            &mut rng,
            next_shred_index,
            10,
            &shredder,
            &leader,
            merkle_variant,
        );
        let coding_shreds_different_fec = new_rand_coding_shreds(
            &mut rng,
            next_shred_index + 1,
            10,
            &shredder,
            &leader,
            merkle_variant,
        );
        let coding_shreds_different_fec_and_size = new_rand_coding_shreds(
            &mut rng,
            next_shred_index + 1,
            13,
            &shredder,
            &leader,
            merkle_variant,
        );

        let test_cases = vec![
            // Different index, different fec set, same erasure meta
            (
                coding_shreds[0].clone(),
                coding_shreds_different_fec[1].clone(),
            ),
            // Different index, different fec set, different erasure meta
            (
                coding_shreds[0].clone(),
                coding_shreds_different_fec_and_size[1].clone(),
            ),
            // Different index, same fec set, same erasure meta
            (coding_shreds[0].clone(), coding_shreds[1].clone()),
            (
                coding_shreds_different_fec[0].clone(),
                coding_shreds_different_fec[1].clone(),
            ),
            (
                coding_shreds_different_fec_and_size[0].clone(),
                coding_shreds_different_fec_and_size[1].clone(),
            ),
        ];
        for (shred1, shred2) in test_cases.into_iter() {
            assert_matches!(
                from_shred(
                    shred1.clone(),
                    Pubkey::new_unique(), // self_pubkey
                    shred2.payload().clone(),
                    Some(leader_schedule),
                    rng.gen(), // wallclock
                    512,       // max_size
                )
                .err()
                .unwrap(),
                Error::InvalidErasureMetaConflict
            );

            let chunks: Vec<_> = from_shred_bypass_checks(
                shred1.clone(),
                Pubkey::new_unique(), // self_pubkey
                shred2.clone(),
                rng.gen(), // wallclock
                512,       // max_size
            )
            .unwrap()
            .collect();
            assert!(chunks.len() > 4);

            assert_matches!(
                into_shreds(&leader.pubkey(), chunks).err().unwrap(),
                Error::InvalidErasureMetaConflict
            );
        }
    }
}
