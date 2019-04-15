use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use crate::result::Result;
use std::cmp;
use std::convert::AsMut;
use std::sync::{Arc, RwLock};

use reed_solomon_erasure::ReedSolomon;

//TODO(sakridge) pick these values
/// Number of data blobs
pub const NUM_DATA: usize = 16;
/// Number of coding blobs; also the maximum number that can go missing.
pub const NUM_CODING: usize = 4;
/// Total number of blobs in an erasure set; includes data and coding blobs
pub const ERASURE_SET_SIZE: usize = NUM_DATA + NUM_CODING;

pub fn encode(data: &[&[u8]], parity: &mut [&mut [u8]]) -> Result<()> {
    let rs = ReedSolomon::new(data.len(), parity.len())?;
    rs.encode_sep(data, parity)?;

    Ok(())
}

/// Recover data + coding blocks into data blocks
/// # Arguments
/// * `data` - array of data blocks to recover into
/// * `coding` - array of coding blocks
/// * `erasures` - list of indices in data where blocks should be recovered
pub fn decode_blocks(
    blocks: &mut [&mut [u8]],
    erasures: &[usize],
    data_count: usize,
    coding_count: usize,
) -> Result<()> {
    let rs = ReedSolomon::new(data_count, coding_count)?;

    let present: Vec<_> = (0..blocks.len()).map(|i| !erasures.contains(&i)).collect();

    rs.reconstruct(blocks, &present)?;

    Ok(())
}

/// Reconstruct any missing blobs in this erasure set if possible
/// Re-indexes any coding blobs that have been reconstructed and fixes up size in metadata
/// Assumes that the user has sliced into the blobs appropriately already. else recovery will
/// return an error or garbage data
pub fn reconstruct_blobs<B>(
    blobs: &mut [B],
    erasures: &[usize],
    size: usize,
    block_start_idx: u64,
    slot: u64,
) -> Result<(Vec<Blob>, Vec<Blob>)>
where
    B: AsMut<[u8]>,
{
    let mut blocks: Vec<&mut [u8]> = blobs.iter_mut().map(AsMut::as_mut).collect();

    trace!(
        "[reconstruct_blobs] erasures: {:?}, size: {}",
        erasures,
        size,
    );

    // Decode the blocks
    decode_blocks(blocks.as_mut_slice(), &erasures, NUM_DATA, NUM_CODING)?;
    let mut recovered_data = vec![];
    let mut recovered_coding = vec![];

    // Create the missing blobs from the reconstructed data
    for i in erasures {
        let n = *i as usize;
        //let mut blob = Blob::new(blobs[n]);

        let data_size;
        let idx;
        let first_byte;
        if n < NUM_DATA {
            let mut blob = Blob::new(&blocks[n]);

            data_size = blob.data_size() as usize - BLOB_HEADER_SIZE;
            idx = n as u64 + block_start_idx;
            first_byte = blob.data[0];

            blob.set_size(data_size);
            recovered_data.push(blob);
        } else {
            let mut blob = Blob::default();
            blob.data_mut()[..size].copy_from_slice(&blocks[n]);
            data_size = size;
            idx = (n as u64 + block_start_idx) - NUM_DATA as u64;
            first_byte = blob.data[0];

            blob.set_slot(slot);
            blob.set_index(idx);
            blob.set_size(data_size);
            recovered_coding.push(blob);
        }

        trace!(
            "[reconstruct_blobs] erasures[{}] ({}) data_size: {} data[0]: {}",
            *i,
            idx,
            data_size,
            first_byte
        );
    }

    Ok((recovered_data, recovered_coding))
}

/// Generate coding blocks in window starting from start_idx,
///   for num_blobs..  For each block place the coding blobs
///   at the start of the block like so:
///
///  model of an erasure set, with top row being data blobs and second being coding
///  |<======================= NUM_DATA ==============================>|
///  |<==== NUM_CODING ===>|
///  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
///  | D | | D | | D | | D | | D |         | D | | D | | D | | D | | D |
///  +---+ +---+ +---+ +---+ +---+  . . .  +---+ +---+ +---+ +---+ +---+
///  | C | | C | | C | | C | |   |         |   | |   | |   | |   | |   |
///  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
///
///  blob structure for coding, recover
///
///   + ------- meta is set and used by transport, meta.size is actual length
///   |           of data in the byte array blob.data
///   |
///   |          + -- data is stuff shipped over the wire, and has an included
///   |          |        header
///   V          V
///  +----------+------------------------------------------------------------+
///  | meta     |  data                                                      |
///  |+---+--   |+---+---+---+---+------------------------------------------+|
///  || s | .   || i |   | f | s |                                          ||
///  || i | .   || n | i | l | i |                                          ||
///  || z | .   || d | d | a | z |     blob.data(), or blob.data_mut()      ||
///  || e |     || e |   | g | e |                                          ||
///  |+---+--   || x |   | s |   |                                          ||
///  |          |+---+---+---+---+------------------------------------------+|
///  +----------+------------------------------------------------------------+
///             |                |<=== coding blob part for "coding" =======>|
///             |                                                            |
///             |<============== data blob part for "coding"  ==============>|
///
///
///
pub struct CodingGenerator {
    leftover: Vec<SharedBlob>, // SharedBlobs that couldn't be used in last call to next()
}

impl Default for CodingGenerator {
    fn default() -> Self {
        CodingGenerator {
            leftover: Vec::with_capacity(NUM_DATA),
        }
    }
}

impl CodingGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    // must be called with consecutive data blobs from previous invocation
    // blobs from a new slot not start halfway through next_data
    pub fn next(&mut self, next_data: &[SharedBlob]) -> Vec<SharedBlob> {
        let mut next_coding =
            Vec::with_capacity((self.leftover.len() + next_data.len()) / NUM_DATA * NUM_CODING);

        if !self.leftover.is_empty()
            && !next_data.is_empty()
            && self.leftover[0].read().unwrap().slot() != next_data[0].read().unwrap().slot()
        {
            self.leftover.clear(); // reset on slot boundaries
        }

        let next_data: Vec<_> = self.leftover.iter().chain(next_data).cloned().collect();

        for data_blobs in next_data.chunks(NUM_DATA) {
            if data_blobs.len() < NUM_DATA {
                self.leftover = data_blobs.to_vec();
                break;
            }
            self.leftover.clear();

            // find max_data_size for the chunk, round length up to a multiple of wb()
            let max_data_size = data_blobs
                .iter()
                .fold(0, |max, blob| cmp::max(blob.read().unwrap().meta.size, max));

            let data_locks: Vec<_> = data_blobs.iter().map(|b| b.read().unwrap()).collect();
            let data_ptrs: Vec<_> = data_locks
                .iter()
                .map(|l| &l.data[..max_data_size])
                .collect();

            let mut coding_blobs = Vec::with_capacity(NUM_CODING);

            for data_blob in &data_locks[..NUM_CODING] {
                let index = data_blob.index();
                let slot = data_blob.slot();
                let id = data_blob.id();
                let should_forward = data_blob.should_forward();

                let mut coding_blob = Blob::default();
                coding_blob.set_index(index);
                coding_blob.set_slot(slot);
                coding_blob.set_id(&id);
                coding_blob.forward(should_forward);
                coding_blob.set_size(max_data_size);
                coding_blob.set_coding();

                coding_blobs.push(coding_blob);
            }

            if {
                let mut coding_ptrs: Vec<_> = coding_blobs
                    .iter_mut()
                    .map(|blob| &mut blob.data_mut()[..max_data_size])
                    .collect();

                encode(&data_ptrs, coding_ptrs.as_mut_slice())
            }
            .is_ok()
            {
                next_coding.append(&mut coding_blobs);
            }
        }

        next_coding
            .into_iter()
            .map(|blob| Arc::new(RwLock::new(blob)))
            .collect()
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::entry::{make_tiny_test_entries, EntrySlice};
    use crate::packet::{index_blobs, SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::borrow::Borrow;

    /// Specifies the contents of a 16-data-blob and 4-coding-blob erasure set
    /// Exists to be passed to `generate_blocktree_with_coding`
    #[derive(Debug, Copy, Clone)]
    pub struct ErasureSpec {
        /// Which 16-blob erasure set this represents
        pub set_index: u64,
        pub num_data: usize,
        pub num_coding: usize,
    }

    /// Specifies the contents of a slot
    /// Exists to be passed to `generate_blocktree_with_coding`
    #[derive(Debug, Clone)]
    pub struct SlotSpec {
        pub slot: u64,
        pub set_specs: Vec<ErasureSpec>,
    }

    /// Model of a slot in 16-blob chunks with varying amounts of erasure and coding blobs
    /// present
    #[derive(Debug, Clone)]
    pub struct SlotModel {
        pub slot: u64,
        pub chunks: Vec<ErasureSetModel>,
    }

    /// Model of 16-blob chunk
    #[derive(Debug, Clone)]
    pub struct ErasureSetModel {
        pub set_index: u64,
        pub start_index: u64,
        pub coding: Vec<SharedBlob>,
        pub data: Vec<SharedBlob>,
    }

    #[test]
    fn test_coding() {
        let num_data = 4;
        let num_coding = 2;

        let mut vs: Vec<Vec<u8>> = (0..num_data as u8)
            .map(|i| (i..(16 + i)).collect())
            .collect();
        let v_orig: Vec<u8> = vs[0].clone();

        let mut coding_blocks: Vec<_> = (0..num_coding).map(|_| vec![0u8; 16]).collect();

        {
            let mut coding_blocks_slices: Vec<_> =
                coding_blocks.iter_mut().map(Vec::as_mut_slice).collect();
            let v_slices: Vec<_> = vs.iter().map(Vec::as_slice).collect();

            encode(v_slices.as_slice(), coding_blocks_slices.as_mut_slice())
                .expect("encoding must succeed");
        }

        trace!("test_coding: coding blocks:");
        for b in &coding_blocks {
            trace!("test_coding: {:?}", b);
        }

        let erasure: usize = 1;
        let erasures = vec![erasure];
        let erased = vs[erasure].clone();
        // clear an entry
        vs[erasure as usize].copy_from_slice(&[0; 16]);

        {
            let mut blocks: Vec<_> = vs
                .iter_mut()
                .chain(coding_blocks.iter_mut())
                .map(Vec::as_mut_slice)
                .collect();

            decode_blocks(
                blocks.as_mut_slice(),
                erasures.as_slice(),
                num_data,
                num_coding,
            )
            .expect("decoding must succeed");
        }

        trace!("test_coding: vs:");
        for v in &vs {
            trace!("test_coding: {:?}", v);
        }
        assert_eq!(v_orig, vs[0]);
        assert_eq!(erased, vs[erasure]);
    }

    fn test_toss_and_recover(
        data_blobs: &[SharedBlob],
        coding_blobs: &[SharedBlob],
        block_start_idx: usize,
    ) {
        let size = coding_blobs[0].read().unwrap().size();

        // toss one data and one coding
        let erasures = vec![0, NUM_DATA];

        let mut blobs: Vec<SharedBlob> = Vec::with_capacity(ERASURE_SET_SIZE);

        blobs.push(SharedBlob::default()); // empty data, erasure at zero
        for blob in &data_blobs[block_start_idx + 1..block_start_idx + NUM_DATA] {
            // skip first blob
            blobs.push(blob.clone());
        }
        blobs.push(SharedBlob::default()); // empty coding, erasure at zero
        for blob in &coding_blobs[1..NUM_CODING] {
            blobs.push(blob.clone());
        }

        let (recovered_data, recovered_coding) =
            reconstruct_shared_blobs(&mut blobs, &erasures, size, block_start_idx as u64, 0)
                .expect("reconstruction must succeed");

        assert_eq!(recovered_data.len(), 1);
        assert_eq!(recovered_coding.len(), 1);

        assert_eq!(
            blobs[1].read().unwrap().meta,
            data_blobs[block_start_idx + 1].read().unwrap().meta
        );
        assert_eq!(
            blobs[1].read().unwrap().data(),
            data_blobs[block_start_idx + 1].read().unwrap().data()
        );
        assert_eq!(
            recovered_data[0].meta,
            data_blobs[block_start_idx].read().unwrap().meta
        );
        assert_eq!(
            recovered_data[0].data(),
            data_blobs[block_start_idx].read().unwrap().data()
        );
        assert_eq!(
            recovered_coding[0].data(),
            coding_blobs[0].read().unwrap().data()
        );
    }

    #[test]
    fn test_erasure_generate_coding() {
        solana_logger::setup();

        // trivial case
        let mut coding_generator = CodingGenerator::new();
        let blobs = Vec::new();
        for _ in 0..NUM_DATA * 2 {
            let coding = coding_generator.next(&blobs);
            assert_eq!(coding.len(), 0);
        }

        // test coding by iterating one blob at a time
        let data_blobs = generate_test_blobs(0, NUM_DATA * 2);

        for (i, blob) in data_blobs.iter().cloned().enumerate() {
            let coding_blobs = coding_generator.next(&[blob]);
            if !coding_blobs.is_empty() {
                assert_eq!(i % NUM_DATA, NUM_DATA - 1);
                assert_eq!(coding_blobs.len(), NUM_CODING);

                for j in 0..NUM_CODING {
                    assert_eq!(
                        coding_blobs[j].read().unwrap().index(),
                        ((i / NUM_DATA) * NUM_DATA + j) as u64
                    );
                }
                test_toss_and_recover(&data_blobs, &coding_blobs, i - (i % NUM_DATA));
            }
        }
    }

    #[test]
    fn test_erasure_generate_coding_reset_on_new_slot() {
        solana_logger::setup();

        let mut coding_generator = CodingGenerator::new();

        // test coding by iterating one blob at a time
        let data_blobs = generate_test_blobs(0, NUM_DATA * 2);

        for i in NUM_DATA..NUM_DATA * 2 {
            data_blobs[i].write().unwrap().set_slot(1);
        }

        let coding_blobs = coding_generator.next(&data_blobs[0..NUM_DATA - 1]);
        assert_eq!(coding_blobs.len(), 0);

        let coding_blobs = coding_generator.next(&data_blobs[NUM_DATA..]);

        assert_eq!(coding_blobs.len(), NUM_CODING);

        test_toss_and_recover(&data_blobs, &coding_blobs, NUM_DATA);
    }

    #[test]
    fn test_erasure_generate_blocktree_with_coding() {
        let cases = vec![
            (NUM_DATA, NUM_CODING, 7, 5),
            (NUM_DATA - 6, NUM_CODING - 1, 5, 7),
        ];

        for (num_data, num_coding, num_slots, num_sets_per_slot) in cases {
            let ledger_path = get_tmp_ledger_path!();

            let specs = (0..num_slots)
                .map(|slot| {
                    let set_specs = (0..num_sets_per_slot)
                        .map(|set_index| ErasureSpec {
                            set_index,
                            num_data,
                            num_coding,
                        })
                        .collect();

                    SlotSpec { slot, set_specs }
                })
                .collect::<Vec<_>>();

            let blocktree = generate_blocktree_with_coding(&ledger_path, &specs);

            for spec in specs.iter() {
                let slot = spec.slot;

                for erasure_spec in spec.set_specs.iter() {
                    let start_index = erasure_spec.set_index * NUM_DATA as u64;
                    let (data_end, coding_end) = (
                        start_index + erasure_spec.num_data as u64,
                        start_index + erasure_spec.num_coding as u64,
                    );

                    for idx in start_index..data_end {
                        let opt_bytes = blocktree.get_data_blob_bytes(slot, idx).unwrap();
                        assert!(opt_bytes.is_some());
                    }

                    for idx in start_index..coding_end {
                        let opt_bytes = blocktree.get_coding_blob_bytes(slot, idx).unwrap();
                        assert!(opt_bytes.is_some());
                    }
                }
            }

            drop(blocktree);
            Blocktree::destroy(&ledger_path).expect("Expect successful blocktree destruction");
        }
    }

    #[test]
    fn test_recovery_with_model() {
        use std::thread;

        const MAX_ERASURE_SETS: u64 = 16;
        const N_THREADS: usize = 2;
        const N_SLOTS: u64 = 10;

        solana_logger::setup();

        let specs = (0..N_SLOTS).map(|slot| {
            let num_erasure_sets = slot % MAX_ERASURE_SETS;

            let set_specs = (0..num_erasure_sets)
                .map(|set_index| ErasureSpec {
                    set_index,
                    num_data: NUM_DATA,
                    num_coding: NUM_CODING,
                })
                .collect();

            SlotSpec { slot, set_specs }
        });

        let mut handles = vec![];

        for i in 0..N_THREADS {
            let specs = specs.clone();

            let handle = thread::Builder::new()
                .name(i.to_string())
                .spawn(move || {
                    for slot_model in generate_ledger_model(specs) {
                        for erasure_set in slot_model.chunks {
                            let erased_coding = erasure_set.coding[0].clone();
                            let erased_data = erasure_set.data[..3].to_vec();

                            let mut blobs = Vec::with_capacity(ERASURE_SET_SIZE);
                            let erasures = vec![0, 1, 2, NUM_DATA];

                            blobs.push(SharedBlob::default());
                            blobs.push(SharedBlob::default());
                            blobs.push(SharedBlob::default());
                            for blob in erasure_set.data.into_iter().skip(3) {
                                blobs.push(blob);
                            }

                            blobs.push(SharedBlob::default());
                            for blob in erasure_set.coding.into_iter().skip(1) {
                                blobs.push(blob);
                            }

                            let size = erased_coding.read().unwrap().data_size() as usize;

                            reconstruct_shared_blobs(
                                &mut blobs,
                                &erasures,
                                size,
                                erasure_set.set_index * NUM_DATA as u64,
                                slot_model.slot,
                            )
                            .expect("reconstruction must succeed");

                            for (expected, recovered) in erased_data.iter().zip(blobs.iter()) {
                                let expected = expected.read().unwrap();
                                let mut recovered = recovered.write().unwrap();
                                let data_size = recovered.data_size() as usize - BLOB_HEADER_SIZE;
                                recovered.set_size(data_size);
                                let corrupt = data_size > BLOB_DATA_SIZE;
                                assert!(!corrupt, "CORRUPTION");
                                assert_eq!(&*expected, &*recovered);
                            }

                            assert_eq!(
                                erased_coding.read().unwrap().data(),
                                blobs[NUM_DATA].read().unwrap().data()
                            );

                            debug!("passed set: {}", erasure_set.set_index);
                        }
                        debug!("passed slot: {}", slot_model.slot);
                    }
                })
                .expect("thread build error");

            handles.push(handle);
        }

        handles.into_iter().for_each(|h| h.join().unwrap());
    }

    /// Generates a model of a ledger containing certain data and coding blobs according to a spec
    pub fn generate_ledger_model<'a, I, IntoIt, S>(
        specs: I,
    ) -> impl Iterator<Item = SlotModel> + Clone + 'a
    where
        I: IntoIterator<Item = S, IntoIter = IntoIt>,
        IntoIt: Iterator<Item = S> + Clone + 'a,
        S: Borrow<SlotSpec>,
    {
        specs.into_iter().map(|spec| {
            let spec = spec.borrow();
            let slot = spec.slot;

            let chunks = spec
                .set_specs
                .iter()
                .map(|erasure_spec| {
                    let set_index = erasure_spec.set_index as usize;
                    let start_index = set_index * NUM_DATA;

                    let mut blobs = make_tiny_test_entries(NUM_DATA).to_single_entry_shared_blobs();
                    index_blobs(
                        &blobs,
                        &Keypair::new().pubkey(),
                        start_index as u64,
                        slot,
                        0,
                    );

                    let mut coding_generator = CodingGenerator::new();
                    let mut coding_blobs = coding_generator.next(&blobs);

                    blobs.drain(erasure_spec.num_data..);
                    coding_blobs.drain(erasure_spec.num_coding..);

                    ErasureSetModel {
                        start_index: start_index as u64,
                        set_index: set_index as u64,
                        data: blobs,
                        coding: coding_blobs,
                    }
                })
                .collect();

            SlotModel { slot, chunks }
        })
    }

    /// Genarates a ledger according to the given specs.
    /// Blocktree should have correct SlotMeta and ErasureMeta and so on but will not have done any
    /// possible recovery.
    pub fn generate_blocktree_with_coding(ledger_path: &str, specs: &[SlotSpec]) -> Blocktree {
        let blocktree = Blocktree::open(ledger_path).unwrap();

        let model = generate_ledger_model(specs);
        for slot_model in model {
            let slot = slot_model.slot;

            for erasure_set in slot_model.chunks {
                blocktree.write_shared_blobs(erasure_set.data).unwrap();

                for shared_coding_blob in erasure_set.coding.into_iter() {
                    let blob = shared_coding_blob.read().unwrap();
                    blocktree
                        .put_coding_blob_bytes_raw(
                            slot,
                            blob.index(),
                            &blob.data[..blob.size() + BLOB_HEADER_SIZE],
                        )
                        .unwrap();
                }
            }
        }

        blocktree
    }

    fn generate_test_blobs(offset: usize, num_blobs: usize) -> Vec<SharedBlob> {
        let blobs = make_tiny_test_entries(num_blobs).to_single_entry_shared_blobs();

        index_blobs(&blobs, &Pubkey::new_rand(), offset as u64, 0, 0);
        blobs
    }

    fn reconstruct_shared_blobs(
        blobs: &mut [SharedBlob],
        erasures: &[usize],
        size: usize,
        block_start_idx: u64,
        slot: u64,
    ) -> Result<(Vec<Blob>, Vec<Blob>)> {
        let mut locks: Vec<std::sync::RwLockWriteGuard<_>> = blobs
            .iter()
            .map(|shared_blob| shared_blob.write().unwrap())
            .collect();

        let mut slices: Vec<_> = locks
            .iter_mut()
            .enumerate()
            .map(|(i, blob)| {
                if i < NUM_DATA {
                    &mut blob.data[..size]
                } else {
                    &mut blob.data_mut()[..size]
                }
            })
            .collect();

        reconstruct_blobs(&mut slices, erasures, size, block_start_idx, slot)
    }

}
