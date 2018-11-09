// Support erasure coding
use packet::{SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE};
use solana_sdk::pubkey::Pubkey;
use std::cmp;
use std::mem;
use std::result;
use window::WindowSlot;

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
}

pub type Result<T> = result::Result<T, ErasureError>;

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

pub const ERASURE_W: i32 = 32;

// Generate coding blocks into coding
//   There are some alignment restrictions, blocks should be aligned by 16 bytes
//   which means their size should be >= 16 bytes
pub fn generate_coding_blocks(coding: &mut [&mut [u8]], data: &[&[u8]]) -> Result<()> {
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
            return Err(ErasureError::InvalidBlockSize);
        }
        data_arg.push(block.as_ptr());
    }
    let mut coding_arg = Vec::with_capacity(coding.len());
    for mut block in coding {
        if block_len != block.len() as i32 {
            error!(
                "coding block size incorrect {} expected {}",
                block.len(),
                block_len
            );
            return Err(ErasureError::InvalidBlockSize);
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
pub fn decode_blocks(
    data: &mut [&mut [u8]],
    coding: &mut [&mut [u8]],
    erasures: &[i32],
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let block_len = data[0].len();
    let matrix: Vec<i32> = get_matrix(coding.len() as i32, data.len() as i32, ERASURE_W);

    // generate coding pointers, blocks should be the same size
    let mut coding_arg: Vec<*mut u8> = Vec::new();
    for x in coding.iter_mut() {
        if x.len() != block_len {
            return Err(ErasureError::InvalidBlockSize);
        }
        coding_arg.push(x.as_mut_ptr());
    }

    // generate data pointers, blocks should be the same size
    let mut data_arg: Vec<*mut u8> = Vec::new();
    for x in data.iter_mut() {
        if x.len() != block_len {
            return Err(ErasureError::InvalidBlockSize);
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
        return Err(ErasureError::DecodeError);
    }
    Ok(())
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
pub fn generate_coding(
    id: &Pubkey,
    window: &mut [WindowSlot],
    receive_index: u64,
    num_blobs: usize,
    transmit_index_coding: &mut u64,
) -> Result<()> {
    // beginning of the coding blobs of the block that receive_index points into
    let coding_index_start =
        receive_index - (receive_index % NUM_DATA as u64) + (NUM_DATA - NUM_CODING) as u64;

    let start_idx = receive_index as usize % window.len();
    let mut block_start = start_idx - (start_idx % NUM_DATA);

    loop {
        let block_end = block_start + NUM_DATA;
        if block_end > (start_idx + num_blobs) {
            break;
        }
        info!(
            "generate_coding {} start: {} end: {} start_idx: {} num_blobs: {}",
            id, block_start, block_end, start_idx, num_blobs
        );

        let mut max_data_size = 0;

        // find max_data_size, maybe bail if not all the data is here
        for i in block_start..block_end {
            let n = i % window.len();
            trace!("{} window[{}] = {:?}", id, n, window[n].data);

            if let Some(b) = &window[n].data {
                max_data_size = cmp::max(b.read().unwrap().meta.size, max_data_size);
            } else {
                trace!("{} data block is null @ {}", id, n);
                return Ok(());
            }
        }

        // round up to the nearest jerasure alignment
        max_data_size = align!(max_data_size, JERASURE_ALIGN);

        trace!("{} max_data_size: {}", id, max_data_size);

        let mut data_blobs = Vec::with_capacity(NUM_DATA);
        for i in block_start..block_end {
            let n = i % window.len();

            if let Some(b) = &window[n].data {
                // make sure extra bytes in each blob are zero-d out for generation of
                //  coding blobs
                let mut b_wl = b.write().unwrap();
                for i in b_wl.meta.size..max_data_size {
                    b_wl.data[i] = 0;
                }
                data_blobs.push(b);
            }
        }

        // getting ready to do erasure coding, means that we're potentially
        // going back in time, tell our caller we've inserted coding blocks
        // starting at coding_index_start
        *transmit_index_coding = cmp::min(*transmit_index_coding, coding_index_start);

        let mut coding_blobs = Vec::with_capacity(NUM_CODING);
        let coding_start = block_end - NUM_CODING;
        for i in coding_start..block_end {
            let n = i % window.len();
            assert!(window[n].coding.is_none());

            window[n].coding = Some(SharedBlob::default());

            let coding = window[n].coding.clone().unwrap();
            let mut coding_wl = coding.write().unwrap();
            for i in 0..max_data_size {
                coding_wl.data[i] = 0;
            }
            // copy index and id from the data blob
            if let Some(data) = &window[n].data {
                let data_rl = data.read().unwrap();

                let index = data_rl.index().unwrap();
                let slot = data_rl.slot().unwrap();
                let id = data_rl.id().unwrap();

                trace!(
                    "{} copying index {} id {:?} from data to coding",
                    id,
                    index,
                    id
                );
                coding_wl.set_index(index).unwrap();
                coding_wl.set_slot(slot).unwrap();
                coding_wl.set_id(&id).unwrap();
            }
            coding_wl.set_size(max_data_size);
            if coding_wl.set_coding().is_err() {
                return Err(ErasureError::EncodeError);
            }

            coding_blobs.push(coding.clone());
        }

        let data_locks: Vec<_> = data_blobs.iter().map(|b| b.read().unwrap()).collect();

        let data_ptrs: Vec<_> = data_locks
            .iter()
            .enumerate()
            .map(|(i, l)| {
                trace!("{} i: {} data: {}", id, i, l.data[0]);
                &l.data[..max_data_size]
            })
            .collect();

        let mut coding_locks: Vec<_> = coding_blobs.iter().map(|b| b.write().unwrap()).collect();

        let mut coding_ptrs: Vec<_> = coding_locks
            .iter_mut()
            .enumerate()
            .map(|(i, l)| {
                trace!("{} i: {} coding: {}", id, i, l.data[0],);
                &mut l.data_mut()[..max_data_size]
            })
            .collect();

        generate_coding_blocks(coding_ptrs.as_mut_slice(), &data_ptrs)?;
        debug!(
            "{} start_idx: {} data: {}:{} coding: {}:{}",
            id, start_idx, block_start, block_end, coding_start, block_end
        );
        block_start = block_end;
    }
    Ok(())
}

// examine the window slot at idx returns
//  true if slot is empty
//  true if slot is stale (i.e. has the wrong index), old blob is flushed
//  false if slot has a blob with the right index
fn is_missing(id: &Pubkey, idx: u64, window_slot: &mut Option<SharedBlob>, c_or_d: &str) -> bool {
    if let Some(blob) = window_slot.take() {
        let blob_idx = blob.read().unwrap().index().unwrap();
        if blob_idx == idx {
            trace!("recover {}: idx: {} good {}", id, idx, c_or_d);
            // put it back
            mem::replace(window_slot, Some(blob));
            false
        } else {
            trace!(
                "recover {}: idx: {} old {} {}, recycling",
                id,
                idx,
                c_or_d,
                blob_idx,
            );
            true
        }
    } else {
        trace!("recover {}: idx: {} None {}", id, idx, c_or_d);
        // nothing there
        true
    }
}

// examine the window beginning at block_start for missing or
//  stale (based on block_start_idx) blobs
// if a blob is stale, remove it from the window slot
//  side effect: block will be cleaned of old blobs
fn find_missing(
    id: &Pubkey,
    block_start_idx: u64,
    block_start: usize,
    window: &mut [WindowSlot],
) -> (usize, usize) {
    let mut data_missing = 0;
    let mut coding_missing = 0;
    let block_end = block_start + NUM_DATA;
    let coding_start = block_start + NUM_DATA - NUM_CODING;

    // count missing blobs in the block
    for i in block_start..block_end {
        let idx = (i - block_start) as u64 + block_start_idx;
        let n = i % window.len();

        if is_missing(id, idx, &mut window[n].data, "data") {
            data_missing += 1;
        }

        if i >= coding_start && is_missing(id, idx, &mut window[n].coding, "coding") {
            coding_missing += 1;
        }
    }
    (data_missing, coding_missing)
}

// Recover a missing block into window
//   missing blocks should be None or old...
//   If not enough coding or data blocks are present to restore
//    any of the blocks, the block is skipped.
//   Side effect: old blobs in a block are None'd
pub fn recover(id: &Pubkey, window: &mut [WindowSlot], start_idx: u64, start: usize) -> Result<()> {
    let block_start = start - (start % NUM_DATA);
    let block_start_idx = start_idx - (start_idx % NUM_DATA as u64);

    debug!("start: {} block_start: {}", start, block_start);

    let coding_start = block_start + NUM_DATA - NUM_CODING;
    let block_end = block_start + NUM_DATA;
    trace!(
        "recover {}: block_start_idx: {} block_start: {} coding_start: {} block_end: {}",
        id,
        block_start_idx,
        block_start,
        coding_start,
        block_end
    );

    let (data_missing, coding_missing) = find_missing(id, block_start_idx, block_start, window);

    // if we're not missing data, or if we have too much missin but have enough coding
    if data_missing == 0 {
        // nothing to do...
        return Ok(());
    }

    if (data_missing + coding_missing) > NUM_CODING {
        trace!(
            "recover {}: start: {} skipping recovery data: {} coding: {}",
            id,
            block_start,
            data_missing,
            coding_missing
        );
        // nothing to do...
        return Err(ErasureError::NotEnoughBlocksToDecode);
    }

    trace!(
        "recover {}: recovering: data: {} coding: {}",
        id,
        data_missing,
        coding_missing
    );
    let mut blobs: Vec<SharedBlob> = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut locks = Vec::with_capacity(NUM_DATA + NUM_CODING);
    let mut erasures: Vec<i32> = Vec::with_capacity(NUM_CODING);
    let mut meta = None;
    let mut size = None;

    // add the data blobs we have into recovery blob vector
    for i in block_start..block_end {
        let j = i % window.len();

        if let Some(b) = window[j].data.clone() {
            if meta.is_none() {
                meta = Some(b.read().unwrap().meta.clone());
                trace!("recover {} meta at {} {:?}", id, j, meta);
            }
            blobs.push(b);
        } else {
            let n = SharedBlob::default();
            window[j].data = Some(n.clone());
            // mark the missing memory
            blobs.push(n);
            erasures.push((i - block_start) as i32);
        }
    }
    for i in coding_start..block_end {
        let j = i % window.len();
        if let Some(b) = window[j].coding.clone() {
            if size.is_none() {
                size = Some(b.read().unwrap().meta.size - BLOB_HEADER_SIZE);
                trace!(
                    "{} recover size {} from {}",
                    id,
                    size.unwrap(),
                    i as u64 + block_start_idx
                );
            }
            blobs.push(b);
        } else {
            let n = SharedBlob::default();
            window[j].coding = Some(n.clone());
            //mark the missing memory
            blobs.push(n);
            erasures.push(((i - coding_start) + NUM_DATA) as i32);
        }
    }

    // now that we have size (from coding), zero out data blob tails
    let size = size.unwrap();
    for i in block_start..block_end {
        let j = i % window.len();

        if let Some(b) = &window[j].data {
            let mut b_wl = b.write().unwrap();
            for i in b_wl.meta.size..size {
                b_wl.data[i] = 0;
            }
        }
    }

    // marks end of erasures
    erasures.push(-1);
    trace!("erasures[]: {} {:?} data_size: {}", id, erasures, size,);
    //lock everything for write
    for b in &blobs {
        locks.push(b.write().unwrap());
    }

    {
        let mut coding_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_CODING);
        let mut data_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_DATA);
        for (i, l) in locks.iter_mut().enumerate() {
            if i < NUM_DATA {
                trace!("{} pushing data: {}", id, i);
                data_ptrs.push(&mut l.data[..size]);
            } else {
                trace!("{} pushing coding: {}", id, i);
                coding_ptrs.push(&mut l.data_mut()[..size]);
            }
        }
        trace!(
            "{} coding_ptrs.len: {} data_ptrs.len {}",
            id,
            coding_ptrs.len(),
            data_ptrs.len()
        );
        decode_blocks(
            data_ptrs.as_mut_slice(),
            coding_ptrs.as_mut_slice(),
            &erasures,
        )?;
    }

    let meta = meta.unwrap();
    let mut corrupt = false;
    // repopulate header data size from recovered blob contents
    for i in &erasures[..erasures.len() - 1] {
        let n = *i as usize;
        let mut idx = n as u64 + block_start_idx;

        let mut data_size;
        if n < NUM_DATA {
            data_size = locks[n].data_size().unwrap() as usize;
            data_size -= BLOB_HEADER_SIZE;
            if data_size > BLOB_DATA_SIZE {
                error!("{} corrupt data blob[{}] data_size: {}", id, idx, data_size);
                corrupt = true;
            }
        } else {
            data_size = size;
            idx -= NUM_CODING as u64;
            locks[n].set_index(idx).unwrap();

            if data_size - BLOB_HEADER_SIZE > BLOB_DATA_SIZE {
                error!(
                    "{} corrupt coding blob[{}] data_size: {}",
                    id, idx, data_size
                );
                corrupt = true;
            }
        }

        locks[n].meta = meta.clone();
        locks[n].set_size(data_size);
        trace!(
            "{} erasures[{}] ({}) size: {} data[0]: {}",
            id,
            *i,
            idx,
            data_size,
            locks[n].data()[0]
        );
    }
    assert!(!corrupt, " {} ", id);

    Ok(())
}

#[cfg(test)]
mod test {
    use erasure;
    use logger;
    use packet::{index_blobs, SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE, BLOB_SIZE};
    use rand::{thread_rng, Rng};
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::pubkey::Pubkey;
    //    use std::sync::{Arc, RwLock};
    use window::WindowSlot;

    #[test]
    pub fn test_coding() {
        let zero_vec = vec![0; 16];
        let mut vs: Vec<Vec<u8>> = (0..4).map(|i| (i..(16 + i)).collect()).collect();
        let v_orig: Vec<u8> = vs[0].clone();

        let m = 2;
        let mut coding_blocks: Vec<_> = (0..m).map(|_| vec![0u8; 16]).collect();

        {
            let mut coding_blocks_slices: Vec<_> =
                coding_blocks.iter_mut().map(|x| x.as_mut_slice()).collect();
            let v_slices: Vec<_> = vs.iter().map(|x| x.as_slice()).collect();

            assert!(erasure::generate_coding_blocks(
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

            assert!(erasure::decode_blocks(
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

    fn print_window(window: &[WindowSlot]) {
        for (i, w) in window.iter().enumerate() {
            print!("window({:>w$}): ", i, w = 2);
            if w.data.is_some() {
                let window_l1 = w.data.clone().unwrap();
                let window_l2 = window_l1.read().unwrap();
                print!(
                    "data index: {:?} meta.size: {} data: ",
                    window_l2.index(),
                    window_l2.meta.size
                );
                for i in 0..64 {
                    print!("{:>w$} ", window_l2.data()[i], w = 3);
                }
            } else {
                print!("data null ");
            }
            println!();
            print!("window({:>w$}): ", i, w = 2);
            if w.coding.is_some() {
                let window_l1 = w.coding.clone().unwrap();
                let window_l2 = window_l1.read().unwrap();
                print!(
                    "coding index: {:?} meta.size: {} data: ",
                    window_l2.index(),
                    window_l2.meta.size
                );
                for i in 0..8 {
                    print!("{:>w$} ", window_l2.data()[i], w = 3);
                }
            } else {
                print!("coding null");
            }
            println!();
        }
    }

    const WINDOW_SIZE: usize = 64;
    fn generate_window(offset: usize, num_blobs: usize) -> Vec<WindowSlot> {
        let mut window = vec![
            WindowSlot {
                data: None,
                coding: None,
                leader_unknown: false,
            };
            WINDOW_SIZE
        ];
        let mut blobs = Vec::with_capacity(num_blobs);
        for i in 0..num_blobs {
            let b = SharedBlob::default();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            // generate a random length, multiple of 4 between 8 and 32
            let data_len = if i == 3 {
                BLOB_DATA_SIZE
            } else {
                (thread_rng().gen_range(2, 8) * 4) + 1
            };

            eprintln!("data_len of {} is {}", i, data_len);
            w.set_size(data_len);

            for k in 0..data_len {
                w.data_mut()[k] = (k + i) as u8;
            }

            // overfill, simulates re-used blobs
            for i in BLOB_HEADER_SIZE + data_len..BLOB_SIZE {
                w.data[i] = thread_rng().gen();
            }

            blobs.push(b_);
        }

        index_blobs(&blobs, &Keypair::new().pubkey(), offset as u64, 13);
        for b in blobs {
            let idx = b.read().unwrap().index().unwrap() as usize % WINDOW_SIZE;

            window[idx].data = Some(b);
        }
        window
    }

    fn scramble_window_tails(window: &mut [WindowSlot], num_blobs: usize) {
        for i in 0..num_blobs {
            if let Some(b) = &window[i].data {
                let size = {
                    let b_l = b.read().unwrap();
                    b_l.meta.size
                } as usize;

                let mut b_l = b.write().unwrap();
                for i in size..BLOB_SIZE {
                    b_l.data[i] = thread_rng().gen();
                }
            }
        }
    }

    #[test]
    pub fn test_window_recover_basic() {
        logger::setup();
        // Generate a window
        let offset = 0;
        let num_blobs = erasure::NUM_DATA + 2;
        let mut window = generate_window(WINDOW_SIZE, num_blobs);

        for slot in &window {
            if let Some(blob) = &slot.data {
                let blob_r = blob.read().unwrap();
                assert!(!blob_r.is_coding());
            }
        }

        println!("** after-gen-window:");
        print_window(&window);

        // Generate the coding blocks
        let mut index = (erasure::NUM_DATA + 2) as u64;
        let id = Pubkey::default();
        assert!(
            erasure::generate_coding(&id, &mut window, offset as u64, num_blobs, &mut index)
                .is_ok()
        );
        assert_eq!(index, (erasure::NUM_DATA - erasure::NUM_CODING) as u64);

        println!("** after-gen-coding:");
        print_window(&window);

        println!("** whack data block:");
        // test erasing a data block
        let erase_offset = offset;
        // Create a hole in the window
        let refwindow = window[erase_offset].data.clone();
        window[erase_offset].data = None;
        print_window(&window);

        // put junk in the tails, simulates re-used blobs
        scramble_window_tails(&mut window, num_blobs);

        // Recover it from coding
        assert!(erasure::recover(&id, &mut window, (offset + WINDOW_SIZE) as u64, offset,).is_ok());
        println!("** after-recover:");
        print_window(&window);

        {
            // Check the result, block is here to drop locks

            let window_l = window[erase_offset].data.clone().unwrap();
            let window_l2 = window_l.read().unwrap();
            let ref_l = refwindow.clone().unwrap();
            let ref_l2 = ref_l.read().unwrap();

            assert_eq!(window_l2.meta.size, ref_l2.meta.size);
            assert_eq!(
                window_l2.data[..window_l2.meta.size],
                ref_l2.data[..window_l2.meta.size]
            );
            assert_eq!(window_l2.meta.addr, ref_l2.meta.addr);
            assert_eq!(window_l2.meta.port, ref_l2.meta.port);
            assert_eq!(window_l2.meta.v6, ref_l2.meta.v6);
            assert_eq!(
                window_l2.index().unwrap(),
                (erase_offset + WINDOW_SIZE) as u64
            );
        }

        println!("** whack coding block and data block");
        // tests erasing a coding block and a data block
        let erase_offset = offset + erasure::NUM_DATA - erasure::NUM_CODING;
        // Create a hole in the window
        let refwindow = window[erase_offset].data.clone();
        window[erase_offset].data = None;
        window[erase_offset].coding = None;

        print_window(&window);

        // Recover it from coding
        assert!(erasure::recover(&id, &mut window, (offset + WINDOW_SIZE) as u64, offset,).is_ok());
        println!("** after-recover:");
        print_window(&window);

        {
            // Check the result, block is here to drop locks
            let window_l = window[erase_offset].data.clone().unwrap();
            let window_l2 = window_l.read().unwrap();
            let ref_l = refwindow.clone().unwrap();
            let ref_l2 = ref_l.read().unwrap();
            assert_eq!(window_l2.meta.size, ref_l2.meta.size);
            assert_eq!(
                window_l2.data[..window_l2.meta.size],
                ref_l2.data[..window_l2.meta.size]
            );
            assert_eq!(window_l2.meta.addr, ref_l2.meta.addr);
            assert_eq!(window_l2.meta.port, ref_l2.meta.port);
            assert_eq!(window_l2.meta.v6, ref_l2.meta.v6);
            assert_eq!(
                window_l2.index().unwrap(),
                (erase_offset + WINDOW_SIZE) as u64
            );
        }

        println!("** make stale data block index");
        // tests erasing a coding block
        let erase_offset = offset;
        // Create a hole in the window by making the blob's index stale
        let refwindow = window[offset].data.clone();
        if let Some(blob) = &window[erase_offset].data {
            blob.write()
                .unwrap()
                .set_index(erase_offset as u64)
                .unwrap(); // this also writes to refwindow...
        }
        print_window(&window);

        // Recover it from coding
        assert!(erasure::recover(&id, &mut window, (offset + WINDOW_SIZE) as u64, offset,).is_ok());
        println!("** after-recover:");
        print_window(&window);

        // fix refwindow, we wrote to it above...
        if let Some(blob) = &refwindow {
            blob.write()
                .unwrap()
                .set_index((erase_offset + WINDOW_SIZE) as u64)
                .unwrap(); // this also writes to refwindow...
        }

        {
            // Check the result, block is here to drop locks
            let window_l = window[erase_offset].data.clone().unwrap();
            let window_l2 = window_l.read().unwrap();
            let ref_l = refwindow.clone().unwrap();
            let ref_l2 = ref_l.read().unwrap();
            assert_eq!(window_l2.meta.size, ref_l2.meta.size);
            assert_eq!(
                window_l2.data[..window_l2.meta.size],
                ref_l2.data[..window_l2.meta.size]
            );
            assert_eq!(window_l2.index().unwrap(), ref_l2.index().unwrap());
            assert_eq!(window_l2.slot().unwrap(), ref_l2.slot().unwrap());
            assert_eq!(window_l2.meta.addr, ref_l2.meta.addr);
            assert_eq!(window_l2.meta.port, ref_l2.meta.port);
            assert_eq!(window_l2.meta.v6, ref_l2.meta.v6);
            assert_eq!(
                window_l2.index().unwrap(),
                (erase_offset + WINDOW_SIZE) as u64
            );
        }
    }

    //    //TODO This needs to be reworked
    //    #[test]
    //    #[ignore]
    //    pub fn test_window_recover() {
    //        logger::setup();
    //        let offset = 4;
    //        let data_len = 16;
    //        let num_blobs = erasure::NUM_DATA + 2;
    //        let (mut window, blobs_len) = generate_window(data_len, offset, num_blobs);
    //        println!("** after-gen:");
    //        print_window(&window);
    //        assert!(erasure::generate_coding(&mut window, offset, blobs_len).is_ok());
    //        println!("** after-coding:");
    //        print_window(&window);
    //        let refwindow = window[offset + 1].clone();
    //        window[offset + 1] = None;
    //        window[offset + 2] = None;
    //        window[offset + erasure::SET_SIZE + 3] = None;
    //        window[offset + (2 * erasure::SET_SIZE) + 0] = None;
    //        window[offset + (2 * erasure::SET_SIZE) + 1] = None;
    //        window[offset + (2 * erasure::SET_SIZE) + 2] = None;
    //        let window_l0 = &(window[offset + (3 * erasure::SET_SIZE)]).clone().unwrap();
    //        window_l0.write().unwrap().data[0] = 55;
    //        println!("** after-nulling:");
    //        print_window(&window);
    //        assert!(erasure::recover(&mut window, offset, offset + blobs_len).is_ok());
    //        println!("** after-restore:");
    //        print_window(&window);
    //        let window_l = window[offset + 1].clone().unwrap();
    //        let ref_l = refwindow.clone().unwrap();
    //        assert_eq!(
    //            window_l.read().unwrap().data()[..data_len],
    //            ref_l.read().unwrap().data()[..data_len]
    //        );
    //    }
}
