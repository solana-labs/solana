//! # Erasure Coding and Recovery
//!
//! Blobs are logically grouped into erasure sets or blocks. Each set contains 16 sequential data
//! blobs and 4 sequential coding blobs.
//!
//! Coding blobs in each set starting from `start_idx`:
//!   For each erasure set:
//!     generate `NUM_CODING` coding_blobs.
//!     index the coding blobs from `start_idx` to `start_idx + NUM_CODING - 1`.
//!
//!  model of an erasure set, with top row being data blobs and second being coding
//!  |<======================= NUM_DATA ==============================>|
//!  |<==== NUM_CODING ===>|
//!  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//!  | D | | D | | D | | D | | D |         | D | | D | | D | | D | | D |
//!  +---+ +---+ +---+ +---+ +---+  . . .  +---+ +---+ +---+ +---+ +---+
//!  | C | | C | | C | | C | |   |         |   | |   | |   | |   | |   |
//!  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//!
//!  blob structure for coding blobs
//!
//!   + ------- meta is set and used by transport, meta.size is actual length
//!   |           of data in the byte array blob.data
//!   |
//!   |          + -- data is stuff shipped over the wire, and has an included
//!   |          |        header
//!   V          V
//!  +----------+------------------------------------------------------------+
//!  | meta     |  data                                                      |
//!  |+---+--   |+---+---+---+---+------------------------------------------+|
//!  || s | .   || i |   | f | s |                                          ||
//!  || i | .   || n | i | l | i |                                          ||
//!  || z | .   || d | d | a | z |     blob.data(), or blob.data_mut()      ||
//!  || e |     || e |   | g | e |                                          ||
//!  |+---+--   || x |   | s |   |                                          ||
//!  |          |+---+---+---+---+------------------------------------------+|
//!  +----------+------------------------------------------------------------+
//!             |                |<=== coding blob part for "coding" =======>|
//!             |                                                            |
//!             |<============== data blob part for "coding"  ==============>|
//!
//!
use crate::packet::{Blob, SharedBlob, BLOB_HEADER_SIZE};
use std::{
    borrow::BorrowMut,
    sync::{Arc, RwLock},
};

use reed_solomon_erasure::ReedSolomon;

/// Max number of data blobs in an erasure set; Also max number of parity blobs.
pub const MAX_SET_SIZE: usize = 255;

type Result<T> = std::result::Result<T, reed_solomon_erasure::Error>;

/// This struct is stored in the header of any data blob that has been encoded
/// Every coding blob contains it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct CodingHeader {
    /// Index of the first data blob in the set
    pub start_index: u64,
    /// Index of the erasure set. Slot relative.
    pub set_index: u64,
    /// Number of data blobs in the set.
    pub data_count: usize,
    /// Number of parity blobs in the set.
    pub parity_count: usize,
    /// Size of the largest data blob in the set, including the header.
    pub shard_size: usize,
}

impl CodingHeader {
    /// returns the set-relative index of the blob with the given index.
    pub fn data_index_in_set(&self, index: u64) -> u64 {
        index - self.start_index
    }

    /// returns the set-relative index of the coding blob with the given index.
    /// in the context of erasure/recovery coding blobs come after data-blobs.
    pub fn coding_index_in_set(&self, index: u64) -> u64 {
        index + self.data_count as u64
    }

    /// returns the end boundary indexes of the data and coding blobs in this set, respectively.
    pub fn end_indexes(&self) -> (u64, u64) {
        let start = self.start_index;
        (start + self.data_count as u64, self.parity_count as u64)
    }
}

/// Erasure code data blobs.
///
/// # Arguments
///
/// * `slot` - slot all blobs belong to
/// * `set_index` -  index of the erasure set being encoded
/// * `start_index` - index of the first data blob
/// * `blobs` - data blobs to be encoded. an amount greater than `MAX_SET_SIZE` causes errors.
/// * `parity` - number of parity blobs to create. values greater than `MAX_SET_SIZE` cause errors.
pub fn encode<B: BorrowMut<Blob>>(
    slot: u64,
    set_index: u64,
    start_index: u64,
    blobs: &mut [B],
    parity: usize,
) -> Result<Vec<Blob>> {
    let data = blobs.len();
    // this would fail if there are too few or too many blobs
    let rs = ReedSolomon::new(data, parity)?;
    let mut header = CodingHeader {
        data_count: data,
        parity_count: parity,
        start_index,
        set_index,
        shard_size: 0,
    };

    let shard_size = blobs
        .iter_mut()
        .map(|blob| (*blob).borrow().data_size() as usize)
        .max()
        .expect("must be >=1 blobs");

    //header.shard_size = crate::packet::BLOB_DATA_SIZE;
    header.shard_size = shard_size;

    let slices = blobs
        .iter_mut()
        .map(|b| {
            let blob: &mut Blob = b.borrow_mut();
            blob.set_coding_header(&header);
            &blob.data[..shard_size]
        })
        .collect::<Vec<_>>();

    let mut parity_blocks = (0..parity).map(|_| vec![0; shard_size]).collect::<Vec<_>>();
    let mut parity_slices = parity_blocks
        .iter_mut()
        .map(|v| &mut v[..])
        .collect::<Vec<_>>();

    rs.encode_sep(&slices[..], &mut parity_slices[..])?;

    let parity = parity_blocks
        .into_iter()
        .enumerate()
        .map(|(idx, block)| {
            let mut blob = Blob::default();
            (&mut blob.data[BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + shard_size])
                .copy_from_slice(&block);
            blob.set_slot(slot);
            blob.set_size(shard_size - BLOB_HEADER_SIZE);
            //blob.set_data_size(shard_size as u64);
            blob.set_coding();
            blob.set_coding_header(&header);
            blob.set_index(idx as u64);

            blob
        })
        .collect();

    Ok(parity)
}

/// See `encode`.
/// Convenience function to encode and return `Arc<RwLock<Blob>>`s
pub fn encode_shared(
    slot: u64,
    set_index: u64,
    start_index: u64,
    blobs: &[SharedBlob],
    parity: usize,
) -> Result<Vec<SharedBlob>> {
    let mut locks = blobs
        .iter()
        .map(|shared_blob| shared_blob.write().unwrap())
        .collect::<Vec<_>>();

    let mut blobs = locks.iter_mut().map(|lock| &mut **lock).collect::<Vec<_>>();

    let parity_blobs = encode(slot, set_index, start_index, &mut blobs[..], parity)?
        .into_iter()
        .map(|blob| Arc::new(RwLock::new(blob)))
        .collect();

    Ok(parity_blobs)
}

/// Attempt to recover missing blobs
/// # Arguments
/// * `info` -  the encoding parameters for this erasure set
/// * `slot` - the slot that these blobs belong to
/// * `blobs` - data blobs, followed by parity blobs. blobs must be in order or the recovery will
/// succeed but return garbage.
/// * `present` - each element indicates the presence of the blob with the same set-relative index
pub fn decode<B>(
    info: &CodingHeader,
    slot: u64,
    blobs: &mut [B],
    present: &[bool],
) -> Result<(Vec<Blob>, Vec<Blob>)>
where
    B: BorrowMut<Blob>,
{
    let rs = ReedSolomon::new(info.data_count as usize, info.parity_count as usize)?;

    let mut blocks = vec![];

    for (idx, blob) in blobs.iter_mut().enumerate() {
        if idx < info.data_count {
            blocks.push(&mut blob.borrow_mut().data[..info.shard_size as usize]);
        } else {
            blocks.push(
                &mut blob.borrow_mut().data
                    [BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + info.shard_size as usize],
            );
        }
    }

    assert_eq!(
        blocks.len(),
        rs.data_shard_count() + rs.parity_shard_count()
    );
    rs.reconstruct(&mut blocks[..], present)?;

    let mut recovered_data = vec![];
    let mut recovered_coding = vec![];

    let erasures = present
        .iter()
        .enumerate()
        .filter_map(|(i, present)| if *present { None } else { Some(i) });

    let shard_size = info.shard_size as usize;

    // Create the missing blobs from the reconstructed data
    for n in erasures {
        let data_size;
        let idx;
        let first_byte;

        if n < info.data_count {
            let mut blob: Box<Blob> = Box::default();
            (&mut blob.data[..shard_size]).copy_from_slice(&blocks[n]);

            data_size = blob.data_size() as usize - BLOB_HEADER_SIZE;
            idx = n as u64 + info.start_index;
            first_byte = blob.data[0];

            blob.set_slot(slot);
            blob.set_index(idx);
            blob.set_size(data_size);
            blob.set_coding_header(info);

            recovered_data.push(*blob);
        } else {
            let mut blob = Blob::default();
            (&mut blob.data[BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + shard_size])
                .copy_from_slice(&blocks[n]);
            data_size = shard_size;
            idx = (n - info.data_count) as u64;
            first_byte = blob.data[0];

            blob.set_slot(slot);
            blob.set_index(idx);
            blob.set_size(data_size);
            blob.set_coding_header(info);
            recovered_coding.push(blob);
        }

        trace!(
            "[reconstruct_blobs] erasures[{}] ({}) data_size: {} data[0]: {}",
            n,
            idx,
            data_size,
            first_byte
        );
    }

    Ok((recovered_data, recovered_coding))
}

/// See `decode`
/// Convenience function to accept and return `Arc<RwLock<Blob>>`s
pub fn decode_shared(
    info: &CodingHeader,
    slot: u64,
    blobs: &[SharedBlob],
    present: &[bool],
) -> Result<(Vec<Blob>, Vec<Blob>)> {
    let mut locks = blobs
        .iter()
        .map(|shared_blob| shared_blob.write().unwrap())
        .collect::<Vec<_>>();

    let mut blobs = locks.iter_mut().map(|lock| &mut **lock).collect::<Vec<_>>();

    decode(info, slot, &mut blobs[..], present)
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::packet::{index_blobs, SharedBlob, BLOB_DATA_SIZE, BLOB_HEADER_SIZE};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signable;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::borrow::Borrow;

    const NUM_DATA: usize = 8;
    const NUM_CODING: usize = 9;
    const ERASURE_SET_SIZE: usize = NUM_DATA + NUM_CODING;

    /// Specifies the contents of a 16-data-blob and 4-coding-blob erasure set
    /// Exists to be passed to `generate_blocktree_with_coding`
    #[derive(Debug, Copy, Clone)]
    pub struct ErasureSpec {
        /// Which 16-blob erasure set this represents
        pub set_index: u64,
        pub num_data: usize,
        pub num_coding: usize,
        pub data_count: usize,
        pub parity_count: usize,
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
        pub data_count: usize,
        pub parity_count: usize,
    }

    fn test_toss_and_recover(slot: u64, data_blobs: &[SharedBlob], coding_blobs: &[SharedBlob]) {
        let mut blobs: Vec<SharedBlob> = Vec::with_capacity(ERASURE_SET_SIZE);

        blobs.push(SharedBlob::default()); // empty data, erasure at zero
        for blob in &data_blobs[1..] {
            // skip first blob
            blobs.push(blob.clone());
        }

        blobs.push(SharedBlob::default()); // empty coding, erasure at zero
        for blob in &coding_blobs[1..] {
            blobs.push(blob.clone());
        }

        let info = coding_blobs[0]
            .read()
            .unwrap()
            .get_coding_header()
            .expect("coding info");

        // toss one data and one coding
        let mut present = vec![true; blobs.len()];
        present[0] = false;
        present[data_blobs.len()] = false;

        let (recovered_data, recovered_coding) =
            decode_shared(&info, slot, &mut blobs[..], &present)
                .expect("reconstruction must succeed");

        assert_eq!(recovered_data.len(), 1);
        assert_eq!(recovered_coding.len(), 1);

        assert_eq!(
            blobs[1].read().unwrap().meta,
            data_blobs[1].read().unwrap().meta
        );
        assert_eq!(
            blobs[1].read().unwrap().data(),
            data_blobs[1].read().unwrap().data()
        );
        assert_eq!(recovered_data[0].meta, data_blobs[0].read().unwrap().meta);
        assert_eq!(
            recovered_data[0].data(),
            data_blobs[0].read().unwrap().data()
        );
        assert_eq!(
            recovered_coding[0].data(),
            coding_blobs[0].read().unwrap().data()
        );
    }

    #[test]
    fn test_erasure_generate_coding() {
        // test coding by iterating one blob at a time
        let test_blobs = generate_shared_test_blobs(0, NUM_DATA * 2);

        for (idx, data_blobs) in test_blobs.chunks_exact(NUM_DATA).enumerate() {
            let coding_blobs = encode_shared(
                0,
                idx as u64,
                (idx * NUM_DATA) as u64,
                &data_blobs[..],
                NUM_CODING,
            )
            .unwrap();

            test_toss_and_recover(0, &data_blobs, &coding_blobs);
        }
    }

    #[test]
    fn test_erasure_generate_blocktree_with_coding() {
        solana_logger::setup();

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
                            data_count: NUM_DATA,
                            parity_count: NUM_CODING,
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
                    let data_end = start_index + erasure_spec.num_data as u64;

                    for idx in start_index..data_end {
                        let opt_bytes = blocktree.get_data_blob_bytes(slot, idx).unwrap();
                        assert!(opt_bytes.is_some());
                    }

                    for idx in 0..erasure_spec.num_coding {
                        let opt_bytes = blocktree
                            .get_coding_blob_bytes(slot, erasure_spec.set_index, idx as u64)
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
    fn test_recovery_with_model() {
        const MAX_ERASURE_SETS: u64 = 16;
        const N_SLOTS: u64 = 10;

        solana_logger::setup();

        let specs = (0..N_SLOTS).map(|slot| {
            let num_erasure_sets = slot % MAX_ERASURE_SETS;

            let set_specs = (0..num_erasure_sets)
                .map(|set_index| ErasureSpec {
                    set_index,
                    num_data: NUM_DATA,
                    num_coding: NUM_CODING,
                    parity_count: NUM_CODING,
                    data_count: NUM_DATA,
                })
                .collect();

            SlotSpec { slot, set_specs }
        });

        for slot_model in generate_ledger_model(specs) {
            for erasure_set in slot_model.chunks {
                let erased_coding = erasure_set.coding[0].clone();
                let erased_data = erasure_set.data[..3].to_vec();
                let info = erasure_set.coding[0]
                    .read()
                    .unwrap()
                    .get_coding_header()
                    .expect("coding info");

                let mut blobs = Vec::with_capacity(ERASURE_SET_SIZE);

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

                let mut present = vec![true; ERASURE_SET_SIZE];
                present[0] = false;
                present[1] = false;
                present[2] = false;
                present[NUM_DATA] = false;

                decode_shared(&info, slot_model.slot, &mut blobs, &present)
                    .expect("reconstruction must succeed");

                for (expected, recovered) in erased_data.iter().zip(blobs.iter()) {
                    let expected = expected.read().unwrap();
                    let mut recovered = recovered.write().unwrap();
                    let data_size = recovered.data_size() as usize - BLOB_HEADER_SIZE;
                    recovered.set_size(data_size);
                    let corrupt = data_size > BLOB_DATA_SIZE;
                    assert!(!corrupt, "CORRUPTION {}", data_size);
                    assert_eq!(expected.data(), recovered.data());
                }

                assert_eq!(
                    erased_coding.read().unwrap().data(),
                    blobs[NUM_DATA].read().unwrap().data()
                );

                debug!("passed set: {}", erasure_set.set_index);
            }
            debug!("passed slot: {}", slot_model.slot);
        }
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
        specs.into_iter().map(move |spec| {
            let spec = spec.borrow();
            let slot = spec.slot;

            let chunks = spec
                .set_specs
                .iter()
                .map(|erasure_spec| {
                    let set_index = erasure_spec.set_index as usize;
                    let start_index = set_index * NUM_DATA;
                    let (parity_count, data_count) =
                        (erasure_spec.parity_count, erasure_spec.data_count);

                    let mut blobs = generate_shared_test_blobs(0, data_count);
                    index_blobs(
                        &blobs,
                        &Keypair::new().pubkey(),
                        start_index as u64,
                        slot,
                        0,
                    );

                    let mut coding_blobs = encode_shared(
                        slot,
                        set_index as u64,
                        start_index as u64,
                        &blobs,
                        parity_count,
                    )
                    .unwrap();

                    blobs.drain(erasure_spec.num_data..);
                    coding_blobs.drain(erasure_spec.num_coding..);

                    ErasureSetModel {
                        start_index: start_index as u64,
                        set_index: set_index as u64,
                        data: blobs,
                        coding: coding_blobs,
                        parity_count,
                        data_count,
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
                            erasure_set.set_index,
                            blob.index(),
                            &blob.data[..blob.size() + BLOB_HEADER_SIZE],
                        )
                        .unwrap();
                }
            }
        }

        blocktree
    }

    pub fn generate_test_blobs(offset: usize, num_blobs: usize) -> Vec<Blob> {
        let data: Vec<_> = (0..BLOB_DATA_SIZE).into_iter().map(|i| i as u8).collect();

        let blobs: Vec<_> = (0..num_blobs)
            .into_iter()
            .map(|_| {
                let mut blob = Blob::default();
                blob.data_mut()[..data.len()].copy_from_slice(&data);
                blob.set_size(data.len());
                blob.sign(&Keypair::new());
                Arc::new(RwLock::new(blob))
            })
            .collect();

        index_blobs(&blobs, &Pubkey::new_rand(), offset as u64, 0, 0);

        blobs
            .into_iter()
            .map(|shared| shared.read().unwrap().clone())
            .collect()
    }

    pub fn generate_shared_test_blobs(offset: usize, num_blobs: usize) -> Vec<SharedBlob> {
        let data: Vec<_> = (0..BLOB_DATA_SIZE).into_iter().map(|i| i as u8).collect();

        let blobs: Vec<_> = (0..num_blobs)
            .into_iter()
            .map(|_| {
                let mut blob = Blob::default();
                blob.data_mut().copy_from_slice(&data);
                blob.set_size(data.len());
                Arc::new(RwLock::new(blob))
            })
            .collect();

        index_blobs(&blobs, &Pubkey::new_rand(), offset as u64, 0, 0);

        blobs
    }
}
