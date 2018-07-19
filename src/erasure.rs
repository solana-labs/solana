// Support erasure coding
use packet::{BlobRecycler, SharedBlob, BLOB_HEADER_SIZE};
use std::cmp;
use std::result;
use streamer::WindowSlot;

//TODO(sakridge) pick these values
pub const NUM_DATA: usize = 16; // number of data blobs
pub const NUM_CODING: usize = 4; // number of coding blobs, also the maximum number that can go missing
pub const ERASURE_SET_SIZE: usize = NUM_DATA + NUM_CODING; // total number of blobs in an erasure set, includes data and coding blobs

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
    if data.len() == 0 {
        return Ok(());
    }
    let k = data.len() as i32;
    let m = coding.len() as i32;
    let block_len = data[0].len() as i32;
    let matrix: Vec<i32> = get_matrix(m, k, ERASURE_W);
    let mut data_arg = Vec::with_capacity(data.len());
    for block in data {
        if block_len != block.len() as i32 {
            trace!(
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
            trace!(
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
    if data.len() == 0 {
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
    unsafe {
        let ret = jerasure_matrix_decode(
            data.len() as i32,
            coding.len() as i32,
            ERASURE_W,
            matrix.as_ptr(),
            0,
            erasures.as_ptr(),
            data_arg.as_ptr(),
            coding_arg.as_ptr(),
            data[0].len() as i32,
        );
        trace!("jerasure_matrix_decode ret: {}", ret);
        for x in data[erasures[0] as usize][0..8].iter() {
            trace!("{} ", x)
        }
        trace!("");
        if ret < 0 {
            return Err(ErasureError::DecodeError);
        }
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
pub fn generate_coding(
    window: &mut [WindowSlot],
    recycler: &BlobRecycler,
    start_idx: usize,
    num_blobs: usize,
) -> Result<()> {
    let mut block_start = start_idx - (start_idx % NUM_DATA);

    loop {
        if (block_start + NUM_DATA) > (start_idx + num_blobs) {
            break;
        }
        info!(
            "generate_coding start: {} end: {} start_idx: {} num_blobs: {}",
            block_start,
            block_start + NUM_DATA,
            start_idx,
            num_blobs
        );

        let mut data_blobs = Vec::with_capacity(NUM_DATA);
        let mut max_data_size = 0;

        // find max_data_size, maybe bail if not all the data is here
        for i in block_start..block_start + NUM_DATA {
            let n = i % window.len();
            trace!("window[{}] = {:?}", n, window[n].data);

            if let Some(b) = &window[n].data {
                max_data_size = cmp::max(b.read().unwrap().meta.size, max_data_size);
            } else {
                trace!("data block is null @ {}", n);
                return Ok(());
            }
        }

        trace!("max_data_size: {}", max_data_size);

        // make sure extra bytes in each blob are zero-d out for generation of
        //  coding blobs
        for i in block_start..block_start + NUM_DATA {
            let n = i % window.len();

            if let Some(b) = &window[n].data {
                let mut b_wl = b.write().unwrap();
                for i in b_wl.meta.size..max_data_size {
                    b_wl.data[i] = 0;
                }
                data_blobs.push(b);
            }
        }

        let mut coding_blobs = Vec::with_capacity(NUM_CODING);

        let coding_start = block_start + NUM_DATA - NUM_CODING;
        let coding_end = block_start + NUM_DATA;
        for i in coding_start..coding_end {
            let n = i % window.len();
            if window[n].coding.is_none() {
                window[n].coding = Some(recycler.allocate());
            }

            let coding = window[n].coding.clone().unwrap();
            let mut coding_wl = coding.write().unwrap();
            {
                // copy index and id from the data blob
                let data = window[n].data.clone().unwrap();
                let data_rl = data.read().unwrap();
                coding_wl.set_index(data_rl.get_index().unwrap()).unwrap();
                coding_wl.set_id(data_rl.get_id().unwrap()).unwrap();
            }
            coding_wl.set_size(max_data_size);
            if coding_wl.set_coding().is_err() {
                return Err(ErasureError::EncodeError);
            }

            coding_blobs.push(
                window[n]
                    .coding
                    .clone()
                    .expect("'coding_blobs' arr in pub fn generate_coding"),
            );
        }

        trace!("max_data_size {}", max_data_size);

        let mut data_locks = Vec::with_capacity(NUM_DATA);
        for b in &data_blobs {
            data_locks.push(
                b.write()
                    .expect("'data_locks' write lock in pub fn generate_coding"),
            );
        }

        let mut data_ptrs: Vec<&[u8]> = Vec::with_capacity(NUM_DATA);
        for (i, l) in data_locks.iter_mut().enumerate() {
            trace!("i: {} data: {}", i, l.data[0]);
            data_ptrs.push(&l.data[..max_data_size]);
        }

        let mut coding_locks = Vec::with_capacity(NUM_CODING);
        for b in &coding_blobs {
            coding_locks.push(
                b.write()
                    .expect("'coding_locks' arr in pub fn generate_coding"),
            );
        }

        let mut coding_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_CODING);
        for (i, l) in coding_locks.iter_mut().enumerate() {
            trace!("i: {} coding: {} size: {}", i, l.data[0], max_data_size);
            coding_ptrs.push(&mut l.data_mut()[..max_data_size]);
        }

        generate_coding_blocks(coding_ptrs.as_mut_slice(), &data_ptrs)?;
        debug!(
            "start_idx: {} data: {}:{} coding: {}:{}",
            start_idx,
            block_start,
            block_start + NUM_DATA,
            coding_start,
            coding_end
        );
        block_start += NUM_DATA;
    }
    Ok(())
}

// Recover missing blocks into window
//   missing blocks should be None, will use re
//   to allocate new ones. Returns err if not enough
//   coding blocks are present to restore
pub fn recover(
    recycler: &BlobRecycler,
    window: &mut [WindowSlot],
    start: usize,
    num_blobs: usize,
) -> Result<()> {
    let num_blocks = (num_blobs / NUM_DATA) + 1;
    let mut block_start = start - (start % NUM_DATA);

    debug!(
        "num_blocks: {} start: {} num_blobs: {} block_start: {}",
        num_blocks, start, num_blobs, block_start
    );

    for _ in 0..num_blocks {
        let mut data_missing = 0;
        let mut coding_missing = 0;
        let coding_start = block_start + NUM_DATA - NUM_CODING;
        let coding_end = block_start + NUM_DATA;
        trace!(
            "recover: block_start: {} coding_start: {} coding_end: {}",
            block_start,
            coding_start,
            coding_end
        );
        for i in block_start..coding_end {
            let n = i % window.len();
            if window[n].coding.is_none() && i >= coding_start {
                coding_missing += 1;
            }
            if window[n].data.is_none() {
                data_missing += 1;
            }
        }

        // if we're not missing data, or if we have too much missin but have enough coding
        if data_missing == 0 || (data_missing + coding_missing) > NUM_CODING {
            trace!(
                "1: start: {} skipping recovery data: {} coding: {}",
                block_start,
                data_missing,
                coding_missing
            );
            block_start += NUM_DATA;
            continue;
        }
        trace!(
            "2: recovering: data: {} coding: {}",
            data_missing,
            coding_missing
        );
        let mut blobs: Vec<SharedBlob> = Vec::with_capacity(NUM_DATA + NUM_CODING);
        let mut locks = Vec::with_capacity(NUM_DATA + NUM_CODING);
        let mut erasures: Vec<i32> = Vec::with_capacity(NUM_CODING);
        let mut meta = None;
        let mut size = None;

        // add the data blobs we have into recovery blob vector
        for i in block_start..coding_end {
            let j = i % window.len();

            if let Some(b) = window[j].data.clone() {
                if meta.is_none() {
                    meta = Some(b.read().unwrap().meta.clone());
                    trace!("meta at {} {:?}", i, meta);
                }
                blobs.push(b);
            } else {
                let n = recycler.allocate();
                window[j].data = Some(n.clone());
                // mark the missing memory
                blobs.push(n);
                erasures.push((i - block_start) as i32);
            }
        }
        for i in coding_start..coding_end {
            let j = i % window.len();
            if let Some(b) = window[j].coding.clone() {
                if size.is_none() {
                    size = Some(b.read().unwrap().meta.size - BLOB_HEADER_SIZE);
                }
                blobs.push(b);
            } else {
                let n = recycler.allocate();
                window[j].coding = Some(n.clone());
                //mark the missing memory
                blobs.push(n);
                erasures.push(((i - coding_start) + NUM_DATA) as i32);
            }
        }
        // now that we have size, zero out data blob tails
        for i in block_start..coding_end {
            let j = i % window.len();

            if let Some(b) = &window[j].data {
                let size = size.unwrap();
                let mut b_wl = b.write().unwrap();
                for i in b_wl.meta.size..size {
                    b_wl.data[i] = 0;
                }
            }
        }

        erasures.push(-1);
        trace!(
            "erasures: {:?} data_size: {} header_size: {}",
            erasures,
            size.unwrap(),
            BLOB_HEADER_SIZE
        );
        //lock everything
        for b in &blobs {
            locks.push(b.write().expect("'locks' arr in pb fn recover"));
        }

        {
            let mut coding_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_CODING);
            let mut data_ptrs: Vec<&mut [u8]> = Vec::with_capacity(NUM_DATA);
            for (i, l) in locks.iter_mut().enumerate() {
                if i < NUM_DATA {
                    trace!("pushing data: {}", i);
                    data_ptrs.push(&mut l.data[..size.unwrap()]);
                } else {
                    trace!("pushing coding: {}", i);
                    coding_ptrs.push(&mut l.data_mut()[..size.unwrap()]);
                }
            }
            trace!(
                "coding_ptrs.len: {} data_ptrs.len {}",
                coding_ptrs.len(),
                data_ptrs.len()
            );
            decode_blocks(
                data_ptrs.as_mut_slice(),
                coding_ptrs.as_mut_slice(),
                &erasures,
            )?;
        }
        for i in &erasures[..erasures.len() - 1] {
            let idx = *i as usize;

            let mut data_size;
            if idx < NUM_DATA {
                data_size = locks[idx].get_data_size().unwrap();
                data_size -= BLOB_HEADER_SIZE as u64;
            } else {
                data_size = size.unwrap() as u64;
            }

            locks[idx].meta = meta.clone().unwrap();
            locks[idx].set_size(data_size as usize);
            trace!(
                "erasures[{}] size: {} data[0]: {}",
                *i,
                data_size,
                locks[idx].data()[0]
            );
        }
        block_start += NUM_DATA;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crdt;
    use erasure;
    use logger;
    use packet::BlobRecycler;
    use rand::{thread_rng, Rng};
    use signature::KeyPair;
    use signature::KeyPairUtil;
    //    use std::sync::{Arc, RwLock};
    use streamer::WindowSlot;

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

            assert!(
                erasure::generate_coding_blocks(
                    coding_blocks_slices.as_mut_slice(),
                    v_slices.as_slice()
                ).is_ok()
            );
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

            assert!(
                erasure::decode_blocks(
                    v_slices.as_mut_slice(),
                    coding_blocks_slices.as_mut_slice(),
                    erasures.as_slice(),
                ).is_ok()
            );
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
                    window_l2.get_index(),
                    window_l2.meta.size
                );
                for i in 0..8 {
                    print!("{:>w$} ", window_l2.data()[i], w = 2);
                }
            } else {
                print!("data null ");
            }
            if w.coding.is_some() {
                let window_l1 = w.coding.clone().unwrap();
                let window_l2 = window_l1.read().unwrap();
                print!(
                    "coding index: {:?} meta.size: {} data: ",
                    window_l2.get_index(),
                    window_l2.meta.size
                );
                for i in 0..8 {
                    print!("{:>w$} ", window_l2.data()[i], w = 2);
                }
            } else {
                print!("coding null");
            }

            println!("");
        }
    }

    fn generate_window(
        blob_recycler: &BlobRecycler,
        offset: usize,
        num_blobs: usize,
    ) -> Vec<WindowSlot> {
        let mut window = vec![
            WindowSlot {
                data: None,
                coding: None
            };
            32
        ];
        let mut blobs = Vec::new();
        for i in 0..num_blobs {
            let b = blob_recycler.allocate();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            // generate a random length, multiple of 4 between 8 and 32
            let data_len = thread_rng().gen_range(2, 8) * 4;
            eprintln!("data_len of {} is {}", i, data_len);
            w.set_size(data_len);

            for k in 0..data_len {
                w.data_mut()[k] = (k + i) as u8;
            }
            // overfill, simulates re-used blobs
            w.data_mut()[data_len] = thread_rng().gen();

            blobs.push(b_);
        }

        let d = crdt::NodeInfo::new(
            KeyPair::new().pubkey(),
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:1235".parse().unwrap(),
            "127.0.0.1:1236".parse().unwrap(),
            "127.0.0.1:1237".parse().unwrap(),
            "127.0.0.1:1238".parse().unwrap(),
        );
        assert!(crdt::Crdt::index_blobs(&d, &blobs, &mut (offset as u64)).is_ok());
        for b in blobs {
            let idx = b.read().unwrap().get_index().unwrap() as usize;
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
                b_l.data[size] = thread_rng().gen();
            }
        }
    }

    #[test]
    pub fn test_window_recover_basic() {
        logger::setup();
        let blob_recycler = BlobRecycler::default();

        // Generate a window
        let offset = 1;
        let num_blobs = erasure::NUM_DATA + 2;
        let mut window = generate_window(&blob_recycler, 0, num_blobs);
        println!("** after-gen-window:");
        print_window(&window);

        // Generate the coding blocks
        assert!(erasure::generate_coding(&mut window, &blob_recycler, offset, num_blobs).is_ok());
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
        assert!(erasure::recover(&blob_recycler, &mut window, offset, num_blobs).is_ok());
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
            assert_eq!(window_l2.get_index().unwrap(), erase_offset as u64);
        }

        println!("** whack coding block and data block");
        // tests erasing a coding block
        let erase_offset = offset + erasure::NUM_DATA - erasure::NUM_CODING;
        // Create a hole in the window
        let refwindow = window[erase_offset].data.clone();
        window[erase_offset].data = None;

        blob_recycler.recycle(window[erase_offset].coding.clone().unwrap());
        window[erase_offset].coding = None;

        print_window(&window);

        // Recover it from coding
        assert!(erasure::recover(&blob_recycler, &mut window, offset, num_blobs).is_ok());
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
            assert_eq!(window_l2.get_index().unwrap(), erase_offset as u64);
        }
    }

    //    //TODO This needs to be reworked
    //    #[test]
    //    #[ignore]
    //    pub fn test_window_recover() {
    //        logger::setup();
    //        let blob_recycler = BlobRecycler::default();
    //        let offset = 4;
    //        let data_len = 16;
    //        let num_blobs = erasure::NUM_DATA + 2;
    //        let (mut window, blobs_len) = generate_window(data_len, &blob_recycler, offset, num_blobs);
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
    //        assert!(erasure::recover(&blob_recycler, &mut window, offset, offset + blobs_len).is_ok());
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
