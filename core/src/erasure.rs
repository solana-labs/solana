// Support erasure coding
use crate::blocktree::Blocktree;
use crate::packet::{Blob, SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE, BLOB_SIZE};
use crate::result::{Error, Result};
use std::cmp;
use std::sync::{Arc, RwLock};

//TODO(sakridge) pick these values
pub const NUM_DATA: usize = 16; // number of data blobs
pub const NUM_CODING: usize = 4; // number of coding blobs, also the maximum number that can go missing
pub const ERASURE_SET_SIZE: usize = NUM_DATA + NUM_CODING; // total number of blobs in an erasure set, includes data and coding blobs

pub const JERASURE_ALIGN: usize = 4; // data size has to be a multiple of 4 bytes

macro_rules! align {
    ($x:expr, $align:expr) => {
        $x + ($align - 1) & !($align - 1)
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErasureError {
    NotEnoughBlocksToDecode,
    DecodeError,
    EncodeError,
    InvalidBlockSize,
    InvalidBlobData,
}

// k = number of data devices
// m = number of coding devices
// w = word size

extern "C" {
    fn jerasure_matrix_encode(
        k: i32,
        m: i32,
        w: i32,
        matrix: *const i32,
        data_ptrs: *const *const u8,
        coding_ptrs: *const *mut u8,
        size: i32,
    );
    fn jerasure_matrix_decode(
        k: i32,
        m: i32,
        w: i32,
        matrix: *const i32,
        row_k_ones: i32,
        erasures: *const i32,
        data_ptrs: *const *mut u8,
        coding_ptrs: *const *mut u8,
        size: i32,
    ) -> i32;
    fn galois_single_divide(a: i32, b: i32, w: i32) -> i32;
}

fn get_matrix(m: i32, k: i32, w: i32) -> Vec<i32> {
    let mut matrix = vec![0; (m * k) as usize];
    for i in 0..m {
        for j in 0..k {
            unsafe {
                matrix[(i * k + j) as usize] = galois_single_divide(1, i ^ (m + j), w);
            }
        }
    }
    matrix
}

const ERASURE_W: i32 = 32;

// Generate coding blocks into coding
//   There are some alignment restrictions, blocks should be aligned by 16 bytes
//   which means their size should be >= 16 bytes
fn generate_coding_blocks(coding: &mut [&mut [u8]], data: &[&[u8]]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let k = data.len() as i32;
    let m = coding.len() as i32;
    let block_len = data[0].len() as i32;
    let matrix: Vec<i32> = get_matrix(m, k, ERASURE_W);
    let mut data_arg = Vec::with_capacity(data.len());
    for block in data {
        if block_len != block.len() as i32 {
            error!(
                "data block size incorrect {} expected {}",
                block.len(),
                block_len
            );
            return Err(Error::ErasureError(ErasureError::InvalidBlockSize));
        }
        data_arg.push(block.as_ptr());
    }
    let mut coding_arg = Vec::with_capacity(coding.len());
    for block in coding {
        if block_len != block.len() as i32 {
            error!(
                "coding block size incorrect {} expected {}",
                block.len(),
                block_len
            );
            return Err(Error::ErasureError(ErasureError::InvalidBlockSize));
        }
        coding_arg.push(block.as_mut_ptr());
    }

    unsafe {
        jerasure_matrix_encode(
            k,
            m,
            ERASURE_W,
            matrix.as_ptr(),
            data_arg.as_ptr(),
            coding_arg.as_ptr(),
            block_len,
        );
    }
    Ok(())
}

// Recover data + coding blocks into data blocks
//   data: array of blocks to recover into
//   coding: arry of coding blocks
//   erasures: list of indices in data where blocks should be recovered
fn decode_blocks(data: &mut [&mut [u8]], coding: &mut [&mut [u8]], erasures: &[i32]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let block_len = data[0].len();
    let matrix: Vec<i32> = get_matrix(coding.len() as i32, data.len() as i32, ERASURE_W);

    // generate coding pointers, blocks should be the same size
    let mut coding_arg: Vec<*mut u8> = Vec::new();
    for x in coding.iter_mut() {
        if x.len() != block_len {
            return Err(Error::ErasureError(ErasureError::InvalidBlockSize));
        }
        coding_arg.push(x.as_mut_ptr());
    }

    // generate data pointers, blocks should be the same size
    let mut data_arg: Vec<*mut u8> = Vec::new();
    for x in data.iter_mut() {
        if x.len() != block_len {
            return Err(Error::ErasureError(ErasureError::InvalidBlockSize));
        }
        data_arg.push(x.as_mut_ptr());
    }
    let ret = unsafe {
        jerasure_matrix_decode(
            data.len() as i32,
            coding.len() as i32,
            ERASURE_W,
            matrix.as_ptr(),
            0,
            erasures.as_ptr(),
            data_arg.as_ptr(),
            coding_arg.as_ptr(),
            data[0].len() as i32,
        )
    };
    trace!("jerasure_matrix_decode ret: {}", ret);
    for x in data[erasures[0] as usize][0..8].iter() {
        trace!("{} ", x)
    }
    trace!("");
    if ret < 0 {
        return Err(Error::ErasureError(ErasureError::DecodeError));
    }
    Ok(())
}

fn decode_blobs(
    blobs: &[SharedBlob],
    erasures: &[i32],
    size: usize,
    block_start_idx: u64,
    slot: u64,
) -> Result<bool> {
    let mut locks = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut coding_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_CODING);
    let mut data_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_DATA);

    assert!(blobs.len() == NUM_DATA + NUM_CODING);
    for b in blobs {
        locks.push(b.write().unwrap());
    }

    for (i, l) in locks.iter_mut().enumerate() {
        if i < NUM_DATA {
            data_ptrs.push(&mut l.data[..size]);
        } else {
            coding_ptrs.push(&mut l.data_mut()[..size]);
        }
    }

    // Decode the blocks
    decode_blocks(
        data_ptrs.as_mut_slice(),
        coding_ptrs.as_mut_slice(),
        &erasures,
    )?;

    // Create the missing blobs from the reconstructed data
    let mut corrupt = false;

    for i in &erasures[..erasures.len() - 1] {
        let n = *i as usize;
        let mut idx = n as u64 + block_start_idx;

        let mut data_size;
        if n < NUM_DATA {
            data_size = locks[n].data_size() as usize;
            data_size -= BLOB_HEADER_SIZE;
            if data_size > BLOB_DATA_SIZE {
                error!("corrupt data blob[{}] data_size: {}", idx, data_size);
                corrupt = true;
                break;
            }
        } else {
            data_size = size;
            idx -= NUM_CODING as u64;
            locks[n].set_slot(slot);
            locks[n].set_index(idx);

            if data_size - BLOB_HEADER_SIZE > BLOB_DATA_SIZE {
                error!("corrupt coding blob[{}] data_size: {}", idx, data_size);
                corrupt = true;
                break;
            }
        }

        locks[n].set_size(data_size);
        trace!(
            "erasures[{}] ({}) size: {} data[0]: {}",
            *i,
            idx,
            data_size,
            locks[n].data()[0]
        );
    }

    Ok(corrupt)
}

// Generate coding blocks in window starting from start_idx,
//   for num_blobs..  For each block place the coding blobs
//   at the end of the block like so:
//
//  block-size part of a Window, with each element a WindowSlot..
//  |<======================= NUM_DATA ==============================>|
//                                              |<==== NUM_CODING ===>|
//  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//  | D | | D | | D | | D | | D |         | D | | D | | D | | D | | D |
//  +---+ +---+ +---+ +---+ +---+  . . .  +---+ +---+ +---+ +---+ +---+
//  |   | |   | |   | |   | |   |         |   | | C | | C | | C | | C |
//  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//
//  blob structure for coding, recover
//
//   + ------- meta is set and used by transport, meta.size is actual length
//   |           of data in the byte array blob.data
//   |
//   |          + -- data is stuff shipped over the wire, and has an included
//   |          |        header
//   V          V
//  +----------+------------------------------------------------------------+
//  | meta     |  data                                                      |
//  |+---+--   |+---+---+---+---+------------------------------------------+|
//  || s | .   || i |   | f | s |                                          ||
//  || i | .   || n | i | l | i |                                          ||
//  || z | .   || d | d | a | z |     blob.data(), or blob.data_mut()      ||
//  || e |     || e |   | g | e |                                          ||
//  |+---+--   || x |   | s |   |                                          ||
//  |          |+---+---+---+---+------------------------------------------+|
//  +----------+------------------------------------------------------------+
//             |                |<=== coding blob part for "coding" =======>|
//             |                                                            |
//             |<============== data blob part for "coding"  ==============>|
//
//
//
pub struct CodingGenerator {
    leftover: Vec<SharedBlob>, // SharedBlobs that couldn't be used in last call to next()
}

impl CodingGenerator {
    pub fn new() -> Self {
        Self {
            leftover: Vec::with_capacity(NUM_DATA),
        }
    }

    // must be called with consecutive data blobs from previous invocation
    pub fn next(&mut self, next_data: &[SharedBlob]) -> Result<Vec<SharedBlob>> {
        let mut next_coding =
            Vec::with_capacity((self.leftover.len() + next_data.len()) / NUM_DATA * NUM_CODING);

        if self.leftover.len() > 0 && next_data.len() > 0 {
            if self.leftover[0].read().unwrap().slot() != next_data[0].read().unwrap().slot() {
                self.leftover.clear(); // reset on slot boundaries
            }
        }
        let next_data: Vec<_> = self.leftover.iter().chain(next_data).cloned().collect();

        for data_blobs in next_data.chunks(NUM_DATA) {
            if data_blobs.len() < NUM_DATA {
                self.leftover = data_blobs.to_vec();
                break;
            }
            self.leftover.clear();

            // find max_data_size for the chunk
            let max_data_size = align!(
                data_blobs
                    .iter()
                    .fold(0, |max, blob| cmp::max(blob.read().unwrap().meta.size, max)),
                JERASURE_ALIGN
            );

            let data_locks: Vec<_> = data_blobs.iter().map(|b| b.read().unwrap()).collect();
            let data_ptrs: Vec<_> = data_locks
                .iter()
                .map(|l| &l.data[..max_data_size])
                .collect();

            let mut coding_blobs = Vec::with_capacity(NUM_CODING);

            for data_blob in &data_locks[NUM_DATA - NUM_CODING..NUM_DATA] {
                let index = data_blob.index();
                let slot = data_blob.slot();
                let id = data_blob.id();
                let should_forward = data_blob.should_forward();

                let coding_blob = SharedBlob::default();
                {
                    let mut coding_blob = coding_blob.write().unwrap();
                    coding_blob.set_index(index);
                    coding_blob.set_slot(slot);
                    coding_blob.set_id(&id);
                    coding_blob.forward(should_forward);
                    coding_blob.set_size(max_data_size);
                    coding_blob.set_coding();
                }
                coding_blobs.push(coding_blob);
            }

            {
                let mut coding_locks: Vec<_> =
                    coding_blobs.iter().map(|b| b.write().unwrap()).collect();

                let mut coding_ptrs: Vec<_> = coding_locks
                    .iter_mut()
                    .map(|l| &mut l.data_mut()[..max_data_size])
                    .collect();

                generate_coding_blocks(coding_ptrs.as_mut_slice(), &data_ptrs)?;
            }
            next_coding.append(&mut coding_blobs);
        }

        Ok(next_coding)
    }
}

// Recover the missing data and coding blobs from the input ledger. Returns a vector
// of the recovered missing data blobs and a vector of the recovered coding blobs
pub fn recover(
    blocktree: &Blocktree,
    slot: u64,
    start_idx: u64,
) -> Result<(Vec<SharedBlob>, Vec<SharedBlob>)> {
    let block_start_idx = start_idx - (start_idx % NUM_DATA as u64);

    debug!("block_start_idx: {}", block_start_idx);

    let coding_start_idx = block_start_idx + NUM_DATA as u64 - NUM_CODING as u64;
    let block_end_idx = block_start_idx + NUM_DATA as u64;
    trace!(
        "recover: coding_start_idx: {} block_end_idx: {}",
        coding_start_idx,
        block_end_idx
    );

    let data_missing = blocktree
        .find_missing_data_indexes(slot, block_start_idx, block_end_idx, NUM_DATA)
        .len();
    let coding_missing = blocktree
        .find_missing_coding_indexes(slot, coding_start_idx, block_end_idx, NUM_CODING)
        .len();

    // if we're not missing data, or if we have too much missing but have enough coding
    if data_missing == 0 {
        // nothing to do...
        return Ok((vec![], vec![]));
    }

    if (data_missing + coding_missing) > NUM_CODING {
        trace!(
            "recover: start: {} skipping recovery data: {} coding: {}",
            block_start_idx,
            data_missing,
            coding_missing
        );
        // nothing to do...
        return Err(Error::ErasureError(ErasureError::NotEnoughBlocksToDecode));
    }

    trace!(
        "recover: recovering: data: {} coding: {}",
        data_missing,
        coding_missing
    );

    let mut blobs: Vec<SharedBlob> = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut erasures: Vec<i32> = Vec::with_capacity(NUM_CODING);

    let mut missing_data: Vec<SharedBlob> = vec![];
    let mut missing_coding: Vec<SharedBlob> = vec![];

    // Add the data blobs we have into the recovery vector, mark the missing ones
    for i in block_start_idx..block_end_idx {
        let result = blocktree.get_data_blob_bytes(slot, i)?;

        categorize_blob(
            &result,
            &mut blobs,
            &mut missing_data,
            &mut erasures,
            (i - block_start_idx) as i32,
        )?;
    }

    let mut size = None;
    // Add the coding blobs we have into the recovery vector, mark the missing ones
    for i in coding_start_idx..block_end_idx {
        let result = blocktree.get_coding_blob_bytes(slot, i)?;

        categorize_blob(
            &result,
            &mut blobs,
            &mut missing_coding,
            &mut erasures,
            ((i - coding_start_idx) + NUM_DATA as u64) as i32,
        )?;

        if let Some(b) = result {
            if size.is_none() {
                size = Some(b.len() - BLOB_HEADER_SIZE);
            }
        }
    }
    // Due to checks above verifying that (data_missing + coding_missing) <= NUM_CODING and
    //  data_missing > 0, we know at least one coding block must exist, so "size" can
    //  not remain None after the above processing.
    let size = size.unwrap();

    // marks end of erasures
    erasures.push(-1);

    trace!("erasures[]:{:?} data_size: {}", erasures, size,);

    let corrupt = decode_blobs(&blobs, &erasures, size, block_start_idx, slot)?;

    if corrupt {
        // Remove the corrupted coding blobs so there's no effort wasted in trying to
        // reconstruct the blobs again
        for i in coding_start_idx..block_end_idx {
            blocktree.delete_coding_blob(slot, i)?;
        }
        return Ok((vec![], vec![]));
    }

    Ok((missing_data, missing_coding))
}

fn categorize_blob(
    get_blob_result: &Option<Vec<u8>>,
    blobs: &mut Vec<SharedBlob>,
    missing: &mut Vec<SharedBlob>,
    erasures: &mut Vec<i32>,
    erasure_index: i32,
) -> Result<()> {
    match get_blob_result {
        Some(b) => {
            if b.len() <= BLOB_HEADER_SIZE || b.len() > BLOB_SIZE {
                return Err(Error::ErasureError(ErasureError::InvalidBlobData));
            }
            blobs.push(Arc::new(RwLock::new(Blob::new(&b))));
        }
        None => {
            // Mark the missing memory
            erasures.push(erasure_index);
            let b = SharedBlob::default();
            blobs.push(b.clone());
            missing.push(b);
        }
    }

    Ok(())
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::entry::{make_tiny_test_entries, EntrySlice};
    use crate::packet::{index_blobs, SharedBlob};
    use solana_sdk::pubkey::Pubkey;

    /// Specifies the contents of a 16-data-blob and 4-coding-blob erasure set
    /// Exists to be passed to `generate_blocktree_with_coding`
    #[derive(Debug, Copy, Clone)]
    pub struct ErasureSpec {
        /// Which 16-blob erasure set this represents
        pub set_index: usize,
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

    #[test]
    fn test_coding() {
        let zero_vec = vec![0; 16];
        let mut vs: Vec<Vec<u8>> = (0..4).map(|i| (i..(16 + i)).collect()).collect();
        let v_orig: Vec<u8> = vs[0].clone();

        let m = 2;
        let mut coding_blocks: Vec<_> = (0..m).map(|_| vec![0u8; 16]).collect();

        {
            let mut coding_blocks_slices: Vec<_> =
                coding_blocks.iter_mut().map(|x| x.as_mut_slice()).collect();
            let v_slices: Vec<_> = vs.iter().map(|x| x.as_slice()).collect();

            assert!(generate_coding_blocks(
                coding_blocks_slices.as_mut_slice(),
                v_slices.as_slice(),
            )
            .is_ok());
        }
        trace!("coding blocks:");
        for b in &coding_blocks {
            trace!("{:?}", b);
        }
        let erasure: i32 = 1;
        let erasures = vec![erasure, -1];
        // clear an entry
        vs[erasure as usize].copy_from_slice(zero_vec.as_slice());

        {
            let mut coding_blocks_slices: Vec<_> =
                coding_blocks.iter_mut().map(|x| x.as_mut_slice()).collect();
            let mut v_slices: Vec<_> = vs.iter_mut().map(|x| x.as_mut_slice()).collect();

            assert!(decode_blocks(
                v_slices.as_mut_slice(),
                coding_blocks_slices.as_mut_slice(),
                erasures.as_slice(),
            )
            .is_ok());
        }

        trace!("vs:");
        for v in &vs {
            trace!("{:?}", v);
        }
        assert_eq!(v_orig, vs[0]);
    }

    #[test]
    fn test_erasure_generate_coding() {
        solana_logger::setup();

        // trivial case
        let mut coding_generator = CodingGenerator::new();
        let blobs = Vec::new();
        for _ in 0..NUM_DATA * 2 {
            let coding = coding_generator.next(&blobs).unwrap();
            assert_eq!(coding.len(), 0);
        }

        // test coding by iterating one blob at a time
        let data_blobs = generate_test_blobs(0, NUM_DATA * 2);

        for (i, blob) in data_blobs.iter().cloned().enumerate() {
            let coding = coding_generator.next(&[blob]).unwrap();

            if !coding.is_empty() {
                assert_eq!(i % NUM_DATA, NUM_DATA - 1);
                assert_eq!(coding.len(), NUM_CODING);

                let size = coding[0].read().unwrap().size();

                // toss one data and one coding
                let erasures: Vec<i32> = vec![0, NUM_DATA as i32, -1];

                let block_start_idx = i - (i % NUM_DATA);
                let mut blobs: Vec<SharedBlob> = Vec::with_capacity(NUM_DATA + NUM_CODING);

                blobs.push(SharedBlob::default()); // empty data, erasure at zero
                for blob in &data_blobs[block_start_idx + 1..block_start_idx + NUM_DATA] {
                    // skip first blob
                    blobs.push(blob.clone());
                }
                blobs.push(SharedBlob::default()); // empty coding, erasure at NUM_DATA
                for blob in &coding[1..NUM_CODING] {
                    blobs.push(blob.clone());
                }

                let corrupt =
                    decode_blobs(&blobs, &erasures, size, block_start_idx as u64, 0).unwrap();

                assert!(!corrupt);

                assert_eq!(
                    blobs[1].read().unwrap().meta,
                    data_blobs[block_start_idx + 1].read().unwrap().meta
                );
                assert_eq!(
                    blobs[1].read().unwrap().data(),
                    data_blobs[block_start_idx + 1].read().unwrap().data()
                );
                assert_eq!(
                    blobs[0].read().unwrap().meta,
                    data_blobs[block_start_idx].read().unwrap().meta
                );
                assert_eq!(
                    blobs[0].read().unwrap().data(),
                    data_blobs[block_start_idx].read().unwrap().data()
                );
                assert_eq!(
                    blobs[NUM_DATA].read().unwrap().data(),
                    coding[0].read().unwrap().data()
                );
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

        let coding = coding_generator.next(&data_blobs[1..]).unwrap();

        assert_eq!(coding.len(), NUM_CODING);
    }

    #[test]
    fn test_generate_blocktree_with_coding() {
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
                    let set_index = erasure_spec.set_index as u64;
                    let start_index = set_index * NUM_DATA as u64;

                    for i in 0..erasure_spec.num_data as u64 {
                        let opt_bytes = blocktree
                            .get_data_blob_bytes(slot, start_index + i)
                            .unwrap();
                        assert!(opt_bytes.is_some());
                    }

                    for i in 0..erasure_spec.num_coding as u64 {
                        let coding_start_index = start_index as usize + (NUM_DATA - NUM_CODING);
                        let opt_bytes = blocktree
                            .get_coding_blob_bytes(slot, coding_start_index as u64 + i)
                            .unwrap();
                        assert!(opt_bytes.is_some());
                    }
                }
            }

            drop(blocktree);
            Blocktree::destroy(&ledger_path).expect("Expect successful blocktree destruction");
        }
    }

    #[test]
    fn test_blocktree_recover_basic() {
        let ledger_path = get_tmp_ledger_path!();

        // Missing 1 data blob
        let spec = SlotSpec {
            slot: 0,
            set_specs: vec![ErasureSpec {
                set_index: 0,
                num_data: NUM_DATA - 1,
                num_coding: 4,
            }],
        };

        let blocktree = generate_blocktree_with_coding(&ledger_path, &[spec]);

        let (recovered_data, recovered_coding) =
            recover(&blocktree, 0, 0).expect("Expect successful recovery");

        assert!(recovered_coding.is_empty());

        assert!(recovered_data.len() == 1);

        drop(blocktree);
        Blocktree::destroy(&ledger_path).expect("Expect successful blocktree destruction");
    }

    #[test]
    fn test_blocktree_recover_basic2() {
        let ledger_path = get_tmp_ledger_path!();

        // Missing 1 data blob in [0, 16)
        // [16..32) complete
        let spec1 = SlotSpec {
            slot: 0,
            set_specs: vec![
                ErasureSpec {
                    set_index: 0,
                    num_data: NUM_DATA - 1,
                    num_coding: NUM_CODING,
                },
                ErasureSpec {
                    set_index: 1,
                    num_data: NUM_DATA,
                    num_coding: NUM_CODING,
                },
            ],
        };

        // Missing 1 coding and 1 data blbo
        let spec2 = SlotSpec {
            slot: 3,
            set_specs: vec![ErasureSpec {
                set_index: 3,
                num_data: NUM_DATA - 1,
                num_coding: NUM_CODING - 1,
            }],
        };

        let blocktree = generate_blocktree_with_coding(&ledger_path, &[spec1, spec2]);

        let (recovered_data, recovered_coding) =
            recover(&blocktree, 0, 0).expect("Expect successful recovery");

        assert!(recovered_coding.is_empty());
        assert_eq!(recovered_data.len(), 1);

        let (recovered_data, recovered_coding) =
            recover(&blocktree, 0, NUM_DATA as u64).expect("Expect successful recovery");

        assert!(recovered_coding.is_empty());
        assert!(recovered_data.is_empty());

        let (recovered_data, recovered_coding) =
            recover(&blocktree, 3, 3 * NUM_DATA as u64).expect("Expect successful recovery");

        assert_eq!(recovered_coding.len(), 1);
        assert_eq!(recovered_data.len(), 1);

        drop(blocktree);
        Blocktree::destroy(&ledger_path).expect("Expect successful blocktree destruction");
    }

    /// Genarates a ledger according to the given specs. Does not generate a valid ledger with
    /// chaining and etc.
    pub fn generate_blocktree_with_coding(ledger_path: &str, specs: &[SlotSpec]) -> Blocktree {
        let blocktree = Blocktree::open(ledger_path).unwrap();

        for spec in specs {
            let slot = spec.slot;

            for erasure_spec in spec.set_specs.iter() {
                let set_index = erasure_spec.set_index as usize;
                let start_index = set_index * NUM_DATA;

                let mut blobs = make_tiny_test_entries(NUM_DATA).to_single_entry_shared_blobs();
                index_blobs(&blobs, &Pubkey::new_rand(), start_index as u64, slot, 0);

                let mut coding_generator = CodingGenerator::new();
                let mut coding_blobs = coding_generator.next(&blobs).unwrap();

                blobs.drain(erasure_spec.num_data..);
                coding_blobs.drain(erasure_spec.num_coding..);

                for shared_blob in blobs {
                    let blob = shared_blob.read().unwrap();
                    let size = blob.size() as usize + BLOB_HEADER_SIZE;
                    blocktree
                        .put_data_blob_bytes(blob.slot(), blob.index(), &blob.data[..size])
                        .unwrap();
                }

                for shared_blob in coding_blobs {
                    let blob = shared_blob.read().unwrap();
                    let size = blob.size() as usize + BLOB_HEADER_SIZE;
                    blocktree
                        .put_coding_blob_bytes(blob.slot(), blob.index(), &blob.data[..size])
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

}
