use {
    crate::shred::{
        self, Error, ProcessShredsStats, Shred, ShredData, ShredFlags, DATA_SHREDS_PER_FEC_BLOCK,
    },
    itertools::Itertools,
    lazy_static::lazy_static,
    lru::LruCache,
    rayon::{prelude::*, ThreadPool},
    reed_solomon_erasure::{
        galois_8::ReedSolomon,
        Error::{InvalidIndex, TooFewDataShards, TooFewShardsPresent},
    },
    solana_entry::entry::Entry,
    solana_measure::measure::Measure,
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{clock::Slot, signature::Keypair},
    std::{
        borrow::Borrow,
        fmt::Debug,
        sync::{Arc, Mutex},
    },
};

lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_thread_count())
        .thread_name(|i| format!("solShredder{i:02}"))
        .build()
        .unwrap();
}

// Maps number of data shreds to the optimal erasure batch size which has the
// same recovery probabilities as a 32:32 erasure batch.
pub(crate) const ERASURE_BATCH_SIZE: [usize; 33] = [
    0, 18, 20, 22, 23, 25, 27, 28, 30, // 8
    32, 33, 35, 36, 38, 39, 41, 42, // 16
    43, 45, 46, 48, 49, 51, 52, 53, // 24
    55, 56, 58, 59, 60, 62, 63, 64, // 32
];

pub struct ReedSolomonCache(
    Mutex<LruCache<(/*data_shards:*/ usize, /*parity_shards:*/ usize), Arc<ReedSolomon>>>,
);

#[derive(Debug)]
pub struct Shredder {
    slot: Slot,
    parent_slot: Slot,
    version: u16,
    reference_tick: u8,
}

impl Shredder {
    pub fn new(
        slot: Slot,
        parent_slot: Slot,
        reference_tick: u8,
        version: u16,
    ) -> Result<Self, Error> {
        if slot < parent_slot || slot - parent_slot > u64::from(std::u16::MAX) {
            Err(Error::InvalidParentSlot { slot, parent_slot })
        } else {
            Ok(Self {
                slot,
                parent_slot,
                reference_tick,
                version,
            })
        }
    }

    pub fn entries_to_shreds(
        &self,
        keypair: &Keypair,
        entries: &[Entry],
        is_last_in_slot: bool,
        next_shred_index: u32,
        next_code_index: u32,
        merkle_variant: bool,
        reed_solomon_cache: &ReedSolomonCache,
        stats: &mut ProcessShredsStats,
    ) -> (
        Vec<Shred>, // data shreds
        Vec<Shred>, // coding shreds
    ) {
        if merkle_variant {
            return shred::make_merkle_shreds_from_entries(
                &PAR_THREAD_POOL,
                keypair,
                entries,
                self.slot,
                self.parent_slot,
                self.version,
                self.reference_tick,
                is_last_in_slot,
                next_shred_index,
                next_code_index,
                reed_solomon_cache,
                stats,
            )
            .unwrap()
            .into_iter()
            .partition(Shred::is_data);
        }
        let data_shreds =
            self.entries_to_data_shreds(keypair, entries, is_last_in_slot, next_shred_index, stats);
        let coding_shreds = Self::data_shreds_to_coding_shreds(
            keypair,
            &data_shreds,
            next_code_index,
            reed_solomon_cache,
            stats,
        )
        .unwrap();
        (data_shreds, coding_shreds)
    }

    fn entries_to_data_shreds(
        &self,
        keypair: &Keypair,
        entries: &[Entry],
        is_last_in_slot: bool,
        next_shred_index: u32,
        process_stats: &mut ProcessShredsStats,
    ) -> Vec<Shred> {
        let mut serialize_time = Measure::start("shred_serialize");
        let serialized_shreds =
            bincode::serialize(entries).expect("Expect to serialize all entries");
        serialize_time.stop();

        let mut gen_data_time = Measure::start("shred_gen_data_time");
        let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
        process_stats.data_buffer_residual +=
            (data_buffer_size - serialized_shreds.len() % data_buffer_size) % data_buffer_size;
        // Integer division to ensure we have enough shreds to fit all the data
        let num_shreds = (serialized_shreds.len() + data_buffer_size - 1) / data_buffer_size;
        let last_shred_index = next_shred_index + num_shreds as u32 - 1;
        // 1) Generate data shreds
        let make_data_shred = |data, shred_index: u32, fec_set_index: u32| {
            let flags = if shred_index != last_shred_index {
                ShredFlags::empty()
            } else if is_last_in_slot {
                // LAST_SHRED_IN_SLOT also implies DATA_COMPLETE_SHRED.
                ShredFlags::LAST_SHRED_IN_SLOT
            } else {
                ShredFlags::DATA_COMPLETE_SHRED
            };
            let parent_offset = self.slot - self.parent_slot;
            let mut shred = Shred::new_from_data(
                self.slot,
                shred_index,
                parent_offset as u16,
                data,
                flags,
                self.reference_tick,
                self.version,
                fec_set_index,
            );
            shred.sign(keypair);
            shred
        };
        let shreds: Vec<&[u8]> = serialized_shreds.chunks(data_buffer_size).collect();
        let fec_set_offsets: Vec<usize> =
            get_fec_set_offsets(shreds.len(), DATA_SHREDS_PER_FEC_BLOCK).collect();
        assert_eq!(shreds.len(), fec_set_offsets.len());
        let shreds: Vec<Shred> = PAR_THREAD_POOL.install(|| {
            shreds
                .into_par_iter()
                .zip(fec_set_offsets)
                .enumerate()
                .map(|(i, (shred, offset))| {
                    let shred_index = next_shred_index + i as u32;
                    let fec_set_index = next_shred_index + offset as u32;
                    make_data_shred(shred, shred_index, fec_set_index)
                })
                .collect()
        });
        gen_data_time.stop();

        process_stats.serialize_elapsed += serialize_time.as_us();
        process_stats.gen_data_elapsed += gen_data_time.as_us();
        process_stats.record_num_data_shreds(shreds.len());

        shreds
    }

    fn data_shreds_to_coding_shreds(
        keypair: &Keypair,
        data_shreds: &[Shred],
        next_code_index: u32,
        reed_solomon_cache: &ReedSolomonCache,
        process_stats: &mut ProcessShredsStats,
    ) -> Result<Vec<Shred>, Error> {
        if data_shreds.is_empty() {
            return Ok(Vec::default());
        }
        let mut gen_coding_time = Measure::start("gen_coding_shreds");
        let chunks: Vec<Vec<&Shred>> = data_shreds
            .iter()
            .group_by(|shred| shred.fec_set_index())
            .into_iter()
            .map(|(_, shreds)| shreds.collect())
            .collect();
        let next_code_index: Vec<_> = std::iter::once(next_code_index)
            .chain(
                chunks
                    .iter()
                    .scan(next_code_index, |next_code_index, chunk| {
                        let num_data_shreds = chunk.len();
                        let is_last_in_slot = chunk
                            .last()
                            .copied()
                            .map(Shred::last_in_slot)
                            .unwrap_or(true);
                        let erasure_batch_size =
                            get_erasure_batch_size(num_data_shreds, is_last_in_slot);
                        *next_code_index += (erasure_batch_size - num_data_shreds) as u32;
                        Some(*next_code_index)
                    }),
            )
            .collect();
        // 1) Generate coding shreds
        let mut coding_shreds: Vec<_> = if chunks.len() <= 1 {
            chunks
                .into_iter()
                .zip(next_code_index)
                .flat_map(|(shreds, next_code_index)| {
                    Shredder::generate_coding_shreds(&shreds, next_code_index, reed_solomon_cache)
                })
                .collect()
        } else {
            PAR_THREAD_POOL.install(|| {
                chunks
                    .into_par_iter()
                    .zip(next_code_index)
                    .flat_map(|(shreds, next_code_index)| {
                        Shredder::generate_coding_shreds(
                            &shreds,
                            next_code_index,
                            reed_solomon_cache,
                        )
                    })
                    .collect()
            })
        };
        gen_coding_time.stop();

        let mut sign_coding_time = Measure::start("sign_coding_shreds");
        // 2) Sign coding shreds
        PAR_THREAD_POOL.install(|| {
            coding_shreds.par_iter_mut().for_each(|coding_shred| {
                coding_shred.sign(keypair);
            })
        });
        sign_coding_time.stop();

        process_stats.gen_coding_elapsed += gen_coding_time.as_us();
        process_stats.sign_coding_elapsed += sign_coding_time.as_us();
        Ok(coding_shreds)
    }

    /// Generates coding shreds for the data shreds in the current FEC set
    pub fn generate_coding_shreds<T: Borrow<Shred>>(
        data: &[T],
        next_code_index: u32,
        reed_solomon_cache: &ReedSolomonCache,
    ) -> Vec<Shred> {
        let (slot, index, version, fec_set_index) = {
            let shred = data.first().unwrap().borrow();
            (
                shred.slot(),
                shred.index(),
                shred.version(),
                shred.fec_set_index(),
            )
        };
        assert_eq!(fec_set_index, index);
        assert!(data
            .iter()
            .map(Borrow::borrow)
            .all(|shred| shred.slot() == slot
                && shred.version() == version
                && shred.fec_set_index() == fec_set_index));
        let num_data = data.len();
        let is_last_in_slot = data
            .last()
            .map(Borrow::borrow)
            .map(Shred::last_in_slot)
            .unwrap_or(true);
        let num_coding = get_erasure_batch_size(num_data, is_last_in_slot)
            .checked_sub(num_data)
            .unwrap();
        assert!(num_coding > 0);
        let data: Vec<_> = data
            .iter()
            .map(Borrow::borrow)
            .map(Shred::erasure_shard_as_slice)
            .collect::<Result<_, _>>()
            .unwrap();
        let mut parity = vec![vec![0u8; data[0].len()]; num_coding];
        reed_solomon_cache
            .get(num_data, num_coding)
            .unwrap()
            .encode_sep(&data, &mut parity[..])
            .unwrap();
        let num_data = u16::try_from(num_data).unwrap();
        let num_coding = u16::try_from(num_coding).unwrap();
        parity
            .iter()
            .enumerate()
            .map(|(i, parity)| {
                let index = next_code_index + u32::try_from(i).unwrap();
                Shred::new_from_parity_shard(
                    slot,
                    index,
                    parity,
                    fec_set_index,
                    num_data,
                    num_coding,
                    u16::try_from(i).unwrap(), // position
                    version,
                )
            })
            .collect()
    }

    pub fn try_recovery(
        shreds: Vec<Shred>,
        reed_solomon_cache: &ReedSolomonCache,
    ) -> Result<Vec<Shred>, Error> {
        let (slot, fec_set_index) = match shreds.first() {
            None => return Err(Error::from(TooFewShardsPresent)),
            Some(shred) => (shred.slot(), shred.fec_set_index()),
        };
        let (num_data_shreds, num_coding_shreds) = match shreds.iter().find(|shred| shred.is_code())
        {
            None => return Ok(Vec::default()),
            Some(shred) => (
                shred.num_data_shreds().unwrap(),
                shred.num_coding_shreds().unwrap(),
            ),
        };
        debug_assert!(shreds
            .iter()
            .all(|shred| shred.slot() == slot && shred.fec_set_index() == fec_set_index));
        debug_assert!(shreds
            .iter()
            .filter(|shred| shred.is_code())
            .all(|shred| shred.num_data_shreds().unwrap() == num_data_shreds
                && shred.num_coding_shreds().unwrap() == num_coding_shreds));
        let num_data_shreds = num_data_shreds as usize;
        let num_coding_shreds = num_coding_shreds as usize;
        let fec_set_size = num_data_shreds + num_coding_shreds;
        if num_coding_shreds == 0 || shreds.len() >= fec_set_size {
            return Ok(Vec::default());
        }
        // Mask to exclude data shreds already received from the return value.
        let mut mask = vec![false; num_data_shreds];
        let mut shards = vec![None; fec_set_size];
        for shred in shreds {
            let index = match shred.erasure_shard_index() {
                Ok(index) if index < fec_set_size => index,
                _ => return Err(Error::from(InvalidIndex)),
            };
            shards[index] = Some(shred.erasure_shard()?);
            if index < num_data_shreds {
                mask[index] = true;
            }
        }
        reed_solomon_cache
            .get(num_data_shreds, num_coding_shreds)?
            .reconstruct_data(&mut shards)?;
        let recovered_data = mask
            .into_iter()
            .zip(shards)
            .filter(|(mask, _)| !mask)
            .filter_map(|(_, shard)| Shred::new_from_serialized_shred(shard?).ok())
            .filter(|shred| {
                shred.slot() == slot
                    && shred.is_data()
                    && match shred.erasure_shard_index() {
                        Ok(index) => index < num_data_shreds,
                        Err(_) => false,
                    }
            })
            .collect();
        Ok(recovered_data)
    }

    /// Combines all shreds to recreate the original buffer
    pub fn deshred(shreds: &[Shred]) -> Result<Vec<u8>, Error> {
        let index = shreds.first().ok_or(TooFewDataShards)?.index();
        let aligned = shreds.iter().zip(index..).all(|(s, i)| s.index() == i);
        let data_complete = {
            let shred = shreds.last().unwrap();
            shred.data_complete() || shred.last_in_slot()
        };
        if !data_complete || !aligned {
            return Err(Error::from(TooFewDataShards));
        }
        let data: Vec<_> = shreds.iter().map(Shred::data).collect::<Result<_, _>>()?;
        let data: Vec<_> = data.into_iter().flatten().copied().collect();
        if data.is_empty() {
            // For backward compatibility. This is needed when the data shred
            // payload is None, so that deserializing to Vec<Entry> results in
            // an empty vector.
            let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
            Ok(vec![0u8; data_buffer_size])
        } else {
            Ok(data)
        }
    }
}

impl ReedSolomonCache {
    const CAPACITY: usize = 4 * DATA_SHREDS_PER_FEC_BLOCK;

    pub(crate) fn get(
        &self,
        data_shards: usize,
        parity_shards: usize,
    ) -> Result<Arc<ReedSolomon>, reed_solomon_erasure::Error> {
        let key = (data_shards, parity_shards);
        {
            let mut cache = self.0.lock().unwrap();
            if let Some(entry) = cache.get(&key) {
                return Ok(entry.clone());
            }
        }
        let entry = ReedSolomon::new(data_shards, parity_shards)?;
        let entry = Arc::new(entry);
        {
            let entry = entry.clone();
            let mut cache = self.0.lock().unwrap();
            cache.put(key, entry);
        }
        Ok(entry)
    }
}

impl Default for ReedSolomonCache {
    fn default() -> Self {
        Self(Mutex::new(LruCache::new(Self::CAPACITY)))
    }
}

/// Maps number of data shreds in each batch to the erasure batch size.
pub(crate) fn get_erasure_batch_size(num_data_shreds: usize, is_last_in_slot: bool) -> usize {
    let erasure_batch_size = ERASURE_BATCH_SIZE
        .get(num_data_shreds)
        .copied()
        .unwrap_or(2 * num_data_shreds);
    if is_last_in_slot {
        erasure_batch_size.max(2 * DATA_SHREDS_PER_FEC_BLOCK)
    } else {
        erasure_batch_size
    }
}

// Returns offsets to fec_set_index when spliting shreds into erasure batches.
fn get_fec_set_offsets(
    mut num_shreds: usize,
    min_chunk_size: usize,
) -> impl Iterator<Item = usize> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        if num_shreds == 0 {
            return None;
        }
        let num_chunks = (num_shreds / min_chunk_size).max(1);
        let chunk_size = (num_shreds + num_chunks - 1) / num_chunks;
        let offsets = std::iter::repeat(offset).take(chunk_size);
        num_shreds -= chunk_size;
        offset += chunk_size;
        Some(offsets)
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            blockstore::MAX_DATA_SHREDS_PER_SLOT,
            shred::{
                self, max_entries_per_n_shred, max_ticks_per_n_shreds, verify_test_data_shred,
                ShredType, MAX_CODE_SHREDS_PER_SLOT,
            },
        },
        assert_matches::assert_matches,
        bincode::serialized_size,
        rand::{seq::SliceRandom, Rng},
        solana_sdk::{
            hash::{hash, Hash},
            pubkey::Pubkey,
            shred_version,
            signature::{Signature, Signer},
            system_transaction,
        },
        std::{collections::HashSet, convert::TryInto, iter::repeat_with, sync::Arc},
    };

    fn verify_test_code_shred(shred: &Shred, index: u32, slot: Slot, pk: &Pubkey, verify: bool) {
        assert_matches!(shred.sanitize(), Ok(()));
        assert!(!shred.is_data());
        assert_eq!(shred.index(), index);
        assert_eq!(shred.slot(), slot);
        assert_eq!(verify, shred.verify(pk));
    }

    fn run_test_data_shredder(slot: Slot) {
        let keypair = Arc::new(Keypair::new());

        // Test that parent cannot be > current slot
        assert_matches!(
            Shredder::new(slot, slot + 1, 0, 0),
            Err(Error::InvalidParentSlot { .. })
        );
        // Test that slot - parent cannot be > u16 MAX
        assert_matches!(
            Shredder::new(slot, slot - 1 - 0xffff, 0, 0),
            Err(Error::InvalidParentSlot { .. })
        );
        let parent_slot = slot - 5;
        let shredder = Shredder::new(slot, parent_slot, 0, 0).unwrap();
        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let is_last_in_slot = true;
        let size = serialized_size(&entries).unwrap() as usize;
        // Integer division to ensure we have enough shreds to fit all the data
        let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
        let num_expected_data_shreds = (size + data_buffer_size - 1) / data_buffer_size;
        let num_expected_coding_shreds =
            get_erasure_batch_size(num_expected_data_shreds, is_last_in_slot)
                - num_expected_data_shreds;
        let start_index = 0;
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            is_last_in_slot,
            start_index, // next_shred_index
            start_index, // next_code_index
            true,        // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        let next_index = data_shreds.last().unwrap().index() + 1;
        assert_eq!(next_index as usize, num_expected_data_shreds);

        let mut data_shred_indexes = HashSet::new();
        let mut coding_shred_indexes = HashSet::new();
        for shred in data_shreds.iter() {
            assert_eq!(shred.shred_type(), ShredType::Data);
            let index = shred.index();
            let is_last = index as usize == num_expected_data_shreds - 1;
            verify_test_data_shred(
                shred,
                index,
                slot,
                parent_slot,
                &keypair.pubkey(),
                true,
                is_last,
                is_last,
            );
            assert!(!data_shred_indexes.contains(&index));
            data_shred_indexes.insert(index);
        }

        for shred in coding_shreds.iter() {
            let index = shred.index();
            assert_eq!(shred.shred_type(), ShredType::Code);
            verify_test_code_shred(shred, index, slot, &keypair.pubkey(), true);
            assert!(!coding_shred_indexes.contains(&index));
            coding_shred_indexes.insert(index);
        }

        for i in start_index..start_index + num_expected_data_shreds as u32 {
            assert!(data_shred_indexes.contains(&i));
        }

        for i in start_index..start_index + num_expected_coding_shreds as u32 {
            assert!(coding_shred_indexes.contains(&i));
        }

        assert_eq!(data_shred_indexes.len(), num_expected_data_shreds);
        assert_eq!(coding_shred_indexes.len(), num_expected_coding_shreds);

        // Test reassembly
        let deshred_payload = Shredder::deshred(&data_shreds).unwrap();
        let deshred_entries: Vec<Entry> = bincode::deserialize(&deshred_payload).unwrap();
        assert_eq!(entries, deshred_entries);
    }

    #[test]
    fn test_data_shredder() {
        run_test_data_shredder(0x1234_5678_9abc_def0);
    }

    #[test]
    fn test_deserialize_shred_payload() {
        let keypair = Arc::new(Keypair::new());
        let slot = 1;
        let parent_slot = 0;
        let shredder = Shredder::new(slot, parent_slot, 0, 0).unwrap();
        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, _) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        let deserialized_shred =
            Shred::new_from_serialized_shred(data_shreds.last().unwrap().payload().clone())
                .unwrap();
        assert_eq!(deserialized_shred, *data_shreds.last().unwrap());
    }

    #[test]
    fn test_shred_reference_tick() {
        let keypair = Arc::new(Keypair::new());
        let slot = 1;
        let parent_slot = 0;
        let shredder = Shredder::new(slot, parent_slot, 5, 0).unwrap();
        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, _) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        data_shreds.iter().for_each(|s| {
            assert_eq!(s.reference_tick(), 5);
            assert_eq!(shred::layout::get_reference_tick(s.payload()).unwrap(), 5);
        });

        let deserialized_shred =
            Shred::new_from_serialized_shred(data_shreds.last().unwrap().payload().clone())
                .unwrap();
        assert_eq!(deserialized_shred.reference_tick(), 5);
    }

    #[test]
    fn test_shred_reference_tick_overflow() {
        let keypair = Arc::new(Keypair::new());
        let slot = 1;
        let parent_slot = 0;
        let shredder = Shredder::new(slot, parent_slot, u8::max_value(), 0).unwrap();
        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, _) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        data_shreds.iter().for_each(|s| {
            assert_eq!(
                s.reference_tick(),
                ShredFlags::SHRED_TICK_REFERENCE_MASK.bits()
            );
            assert_eq!(
                shred::layout::get_reference_tick(s.payload()).unwrap(),
                ShredFlags::SHRED_TICK_REFERENCE_MASK.bits()
            );
        });

        let deserialized_shred =
            Shred::new_from_serialized_shred(data_shreds.last().unwrap().payload().clone())
                .unwrap();
        assert_eq!(
            deserialized_shred.reference_tick(),
            ShredFlags::SHRED_TICK_REFERENCE_MASK.bits(),
        );
    }

    fn run_test_data_and_code_shredder(slot: Slot) {
        let keypair = Arc::new(Keypair::new());
        let shredder = Shredder::new(slot, slot - 5, 0, 0).unwrap();
        // Create enough entries to make > 1 shred
        let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
        let num_entries = max_ticks_per_n_shreds(1, Some(data_buffer_size)) + 1;
        let entries: Vec<_> = (0..num_entries)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        for (i, s) in data_shreds.iter().enumerate() {
            verify_test_data_shred(
                s,
                s.index(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                i == data_shreds.len() - 1,
                i == data_shreds.len() - 1,
            );
        }

        for s in coding_shreds {
            verify_test_code_shred(&s, s.index(), slot, &keypair.pubkey(), true);
        }
    }

    #[test]
    fn test_data_and_code_shredder() {
        run_test_data_and_code_shredder(0x1234_5678_9abc_def0);
    }

    fn run_test_recovery_and_reassembly(slot: Slot, is_last_in_slot: bool) {
        let keypair = Arc::new(Keypair::new());
        let shredder = Shredder::new(slot, slot - 5, 0, 0).unwrap();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
        let entry = Entry::new(&Hash::default(), 1, vec![tx0]);

        let num_data_shreds: usize = 5;
        let data_buffer_size = ShredData::capacity(/*merkle_proof_size:*/ None).unwrap();
        let num_entries =
            max_entries_per_n_shred(&entry, num_data_shreds as u64, Some(data_buffer_size));
        let entries: Vec<_> = (0..num_entries)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let reed_solomon_cache = ReedSolomonCache::default();
        let serialized_entries = bincode::serialize(&entries).unwrap();
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            is_last_in_slot,
            0,     // next_shred_index
            0,     // next_code_index
            false, // merkle_variant
            &reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );
        let num_coding_shreds = coding_shreds.len();

        // We should have 5 data shreds now
        assert_eq!(data_shreds.len(), num_data_shreds);
        assert_eq!(
            num_coding_shreds,
            get_erasure_batch_size(num_data_shreds, is_last_in_slot) - num_data_shreds
        );

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();

        // Test0: Try recovery/reassembly with only data shreds, but not all data shreds. Hint: should fail
        assert_eq!(
            Shredder::try_recovery(
                data_shreds[..data_shreds.len() - 1].to_vec(),
                &reed_solomon_cache
            )
            .unwrap(),
            Vec::default()
        );

        // Test1: Try recovery/reassembly with only data shreds. Hint: should work
        let recovered_data =
            Shredder::try_recovery(data_shreds[..].to_vec(), &reed_solomon_cache).unwrap();
        assert!(recovered_data.is_empty());

        // Test2: Try recovery/reassembly with missing data shreds + coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 == 0 { Some(b.clone()) } else { None })
            .collect();

        let mut recovered_data =
            Shredder::try_recovery(shred_info.clone(), &reed_solomon_cache).unwrap();

        assert_eq!(recovered_data.len(), 2); // Data shreds 1 and 3 were missing
        let recovered_shred = recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            1,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
            false,
            false,
        );
        shred_info.insert(1, recovered_shred);

        let recovered_shred = recovered_data.remove(0);
        verify_test_data_shred(
            &recovered_shred,
            3,
            slot,
            slot - 5,
            &keypair.pubkey(),
            true,
            false,
            false,
        );
        shred_info.insert(3, recovered_shred);

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test3: Try recovery/reassembly with 3 missing data shreds + 2 coding shreds. Hint: should work
        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let recovered_data =
            Shredder::try_recovery(shred_info.clone(), &reed_solomon_cache).unwrap();

        assert_eq!(recovered_data.len(), 3); // Data shreds 0, 2, 4 were missing
        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = i * 2;
            let is_last_data = recovered_shred.index() as usize == num_data_shreds - 1;
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                is_last_data && is_last_in_slot,
                is_last_data,
            );

            shred_info.insert(i * 2, recovered_shred);
        }

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test4: Try reassembly with 2 missing data shreds, but keeping the last
        // data shred. Hint: should fail
        let shreds: Vec<Shred> = all_shreds[..num_data_shreds]
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if (i < 4 && i % 2 != 0) || i == num_data_shreds - 1 {
                    // Keep 1, 3, 4
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(shreds.len(), 3);
        assert_matches!(
            Shredder::deshred(&shreds),
            Err(Error::ErasureError(TooFewDataShards))
        );

        // Test5: Try recovery/reassembly with non zero index full slot with 3 missing data shreds
        // and 2 missing coding shreds. Hint: should work
        let serialized_entries = bincode::serialize(&entries).unwrap();
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true,  // is_last_in_slot
            25,    // next_shred_index,
            25,    // next_code_index
            false, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        // We should have 10 shreds now
        assert_eq!(data_shreds.len(), num_data_shreds);

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();

        let mut shred_info: Vec<Shred> = all_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let recovered_data =
            Shredder::try_recovery(shred_info.clone(), &reed_solomon_cache).unwrap();

        assert_eq!(recovered_data.len(), 3); // Data shreds 25, 27, 29 were missing
        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = 25 + (i * 2);
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                index == 25 + num_data_shreds - 1,
                index == 25 + num_data_shreds - 1,
            );

            shred_info.insert(i * 2, recovered_shred);
        }

        let result = Shredder::deshred(&shred_info[..num_data_shreds]).unwrap();
        assert!(result.len() >= serialized_entries.len());
        assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);

        // Test6: Try recovery/reassembly with incorrect slot. Hint: does not recover any shreds
        let recovered_data =
            Shredder::try_recovery(shred_info.clone(), &reed_solomon_cache).unwrap();
        assert!(recovered_data.is_empty());
    }

    #[test]
    fn test_recovery_and_reassembly() {
        run_test_recovery_and_reassembly(0x1234_5678_9abc_def0, false);
        run_test_recovery_and_reassembly(0x1234_5678_9abc_def0, true);
    }

    fn run_recovery_with_expanded_coding_shreds(num_tx: usize, is_last_in_slot: bool) {
        let mut rng = rand::thread_rng();
        let txs = repeat_with(|| {
            let from_pubkey = Pubkey::new_unique();
            let instruction = solana_sdk::system_instruction::transfer(
                &from_pubkey,
                &Pubkey::new_unique(), // to
                rng.gen(),             // lamports
            );
            let message = solana_sdk::message::Message::new(&[instruction], Some(&from_pubkey));
            let mut tx = solana_sdk::transaction::Transaction::new_unsigned(message);
            // Also randomize the signatre bytes.
            let mut signature = [0u8; 64];
            rng.fill(&mut signature[..]);
            tx.signatures = vec![Signature::from(signature)];
            tx
        })
        .take(num_tx)
        .collect();
        let entry = Entry::new(
            &Hash::new_unique(),  // prev hash
            rng.gen_range(1..64), // num hashes
            txs,
        );
        let keypair = Arc::new(Keypair::new());
        let slot = 71489660;
        let shredder = Shredder::new(
            slot,
            slot - rng.gen_range(1..27), // parent slot
            0,                           // reference tick
            rng.gen(),                   // version
        )
        .unwrap();
        let next_shred_index = rng.gen_range(1..1024);
        let reed_solomon_cache = ReedSolomonCache::default();
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &[entry],
            is_last_in_slot,
            next_shred_index,
            next_shred_index, // next_code_index
            false,            // merkle_variant
            &reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );
        let num_data_shreds = data_shreds.len();
        let mut shreds = coding_shreds;
        shreds.extend(data_shreds.iter().cloned());
        shreds.shuffle(&mut rng);
        shreds.truncate(num_data_shreds);
        shreds.sort_by_key(|shred| {
            if shred.is_data() {
                shred.index()
            } else {
                shred.index() + num_data_shreds as u32
            }
        });
        let exclude: HashSet<_> = shreds
            .iter()
            .filter(|shred| shred.is_data())
            .map(|shred| shred.index())
            .collect();
        let recovered_shreds = Shredder::try_recovery(shreds, &reed_solomon_cache).unwrap();
        assert_eq!(
            recovered_shreds,
            data_shreds
                .into_iter()
                .filter(|shred| !exclude.contains(&shred.index()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_recovery_with_expanded_coding_shreds() {
        for num_tx in 0..50 {
            run_recovery_with_expanded_coding_shreds(num_tx, false);
            run_recovery_with_expanded_coding_shreds(num_tx, true);
        }
    }

    #[test]
    fn test_shred_version() {
        let keypair = Arc::new(Keypair::new());
        let hash = hash(Hash::default().as_ref());
        let version = shred_version::version_from_hash(&hash);
        assert_ne!(version, 0);
        let shredder = Shredder::new(0, 0, 0, version).unwrap();
        let entries: Vec<_> = (0..5)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            0,    // next_shred_index
            0,    // next_code_index
            true, // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        assert!(!data_shreds
            .iter()
            .chain(coding_shreds.iter())
            .any(|s| s.version() != version));
    }

    #[test]
    fn test_shred_fec_set_index() {
        let keypair = Arc::new(Keypair::new());
        let hash = hash(Hash::default().as_ref());
        let version = shred_version::version_from_hash(&hash);
        assert_ne!(version, 0);
        let shredder = Shredder::new(0, 0, 0, version).unwrap();
        let entries: Vec<_> = (0..500)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let start_index = 0x12;
        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            &keypair,
            &entries,
            true,        // is_last_in_slot
            start_index, // next_shred_index
            start_index, // next_code_index
            true,        // merkle_variant
            &ReedSolomonCache::default(),
            &mut ProcessShredsStats::default(),
        );
        const MIN_CHUNK_SIZE: usize = DATA_SHREDS_PER_FEC_BLOCK;
        let chunks: Vec<_> = data_shreds
            .iter()
            .group_by(|shred| shred.fec_set_index())
            .into_iter()
            .map(|(fec_set_index, chunk)| (fec_set_index, chunk.count()))
            .collect();
        assert!(chunks
            .iter()
            .all(|(_, chunk_size)| *chunk_size >= MIN_CHUNK_SIZE));
        assert!(chunks
            .iter()
            .all(|(_, chunk_size)| *chunk_size < 2 * MIN_CHUNK_SIZE));
        assert_eq!(chunks[0].0, start_index);
        assert!(chunks.iter().tuple_windows().all(
            |((fec_set_index, chunk_size), (next_fec_set_index, _chunk_size))| fec_set_index
                + *chunk_size as u32
                == *next_fec_set_index
        ));
        assert!(coding_shreds.len() >= data_shreds.len());
        assert!(coding_shreds
            .iter()
            .zip(&data_shreds)
            .all(|(code, data)| code.fec_set_index() == data.fec_set_index()));
        assert_eq!(
            coding_shreds.last().unwrap().fec_set_index(),
            data_shreds.last().unwrap().fec_set_index()
        );
    }

    #[test]
    fn test_max_coding_shreds() {
        let keypair = Arc::new(Keypair::new());
        let hash = hash(Hash::default().as_ref());
        let version = shred_version::version_from_hash(&hash);
        assert_ne!(version, 0);
        let shredder = Shredder::new(0, 0, 0, version).unwrap();
        let entries: Vec<_> = (0..500)
            .map(|_| {
                let keypair0 = Keypair::new();
                let keypair1 = Keypair::new();
                let tx0 =
                    system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
                Entry::new(&Hash::default(), 1, vec![tx0])
            })
            .collect();

        let mut stats = ProcessShredsStats::default();
        let start_index = 0x12;
        let data_shreds = shredder.entries_to_data_shreds(
            &keypair,
            &entries,
            true, // is_last_in_slot
            start_index,
            &mut stats,
        );

        let next_code_index = data_shreds[0].index();
        let reed_solomon_cache = ReedSolomonCache::default();

        for size in (1..data_shreds.len()).step_by(5) {
            let data_shreds = &data_shreds[..size];
            let coding_shreds = Shredder::data_shreds_to_coding_shreds(
                &keypair,
                data_shreds,
                next_code_index,
                &reed_solomon_cache,
                &mut stats,
            )
            .unwrap();
            let num_shreds: usize = data_shreds
                .iter()
                .group_by(|shred| shred.fec_set_index())
                .into_iter()
                .map(|(_, chunk)| {
                    let chunk: Vec<_> = chunk.collect();
                    get_erasure_batch_size(chunk.len(), chunk.last().unwrap().last_in_slot())
                })
                .sum();
            assert_eq!(coding_shreds.len(), num_shreds - data_shreds.len());
        }
    }

    #[test]
    fn test_get_fec_set_offsets() {
        const MIN_CHUNK_SIZE: usize = 32usize;
        for num_shreds in 0usize..MIN_CHUNK_SIZE {
            let offsets: Vec<_> = get_fec_set_offsets(num_shreds, MIN_CHUNK_SIZE).collect();
            assert_eq!(offsets, vec![0usize; num_shreds]);
        }
        for num_shreds in MIN_CHUNK_SIZE..MIN_CHUNK_SIZE * 8 {
            let chunks: Vec<_> = get_fec_set_offsets(num_shreds, MIN_CHUNK_SIZE)
                .group_by(|offset| *offset)
                .into_iter()
                .map(|(offset, chunk)| (offset, chunk.count()))
                .collect();
            assert_eq!(
                chunks
                    .iter()
                    .map(|(_offset, chunk_size)| chunk_size)
                    .sum::<usize>(),
                num_shreds
            );
            assert!(chunks
                .iter()
                .all(|(_offset, chunk_size)| *chunk_size >= MIN_CHUNK_SIZE));
            assert!(chunks
                .iter()
                .all(|(_offset, chunk_size)| *chunk_size < 2 * MIN_CHUNK_SIZE));
            assert_eq!(chunks[0].0, 0);
            assert!(chunks.iter().tuple_windows().all(
                |((offset, chunk_size), (next_offset, _chunk_size))| offset + chunk_size
                    == *next_offset
            ));
        }
    }

    #[test]
    fn test_max_shreds_per_slot() {
        for num_data_shreds in 32..128 {
            let num_coding_shreds =
                get_erasure_batch_size(num_data_shreds, /*is_last_in_slot:*/ false)
                    .checked_sub(num_data_shreds)
                    .unwrap();
            assert!(
                MAX_DATA_SHREDS_PER_SLOT * num_coding_shreds
                    <= MAX_CODE_SHREDS_PER_SLOT * num_data_shreds
            );
        }
    }
}
