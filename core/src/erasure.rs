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

use reed_solomon_erasure::galois_8::Field;
use reed_solomon_erasure::ReedSolomon;

//TODO(sakridge) pick these values
/// Number of data blobs
pub const NUM_DATA: usize = 8;
/// Number of coding blobs; also the maximum number that can go missing.
pub const NUM_CODING: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasureConfig {
    num_data: usize,
    num_coding: usize,
}

impl Default for ErasureConfig {
    fn default() -> ErasureConfig {
        ErasureConfig {
            num_data: NUM_DATA,
            num_coding: NUM_CODING,
        }
    }
}

impl ErasureConfig {
    pub fn new(num_data: usize, num_coding: usize) -> ErasureConfig {
        ErasureConfig {
            num_data,
            num_coding,
        }
    }

    pub fn num_data(self) -> usize {
        self.num_data
    }

    pub fn num_coding(self) -> usize {
        self.num_coding
    }
}

type Result<T> = std::result::Result<T, reed_solomon_erasure::Error>;

/// Represents an erasure "session" with a particular configuration and number of data and coding
/// blobs
#[derive(Debug, Clone)]
pub struct Session(ReedSolomon<Field>);

impl Session {
    pub fn new(data_count: usize, coding_count: usize) -> Result<Session> {
        let rs = ReedSolomon::new(data_count, coding_count)?;

        Ok(Session(rs))
    }

    pub fn new_from_config(config: &ErasureConfig) -> Result<Session> {
        let rs = ReedSolomon::new(config.num_data, config.num_coding)?;

        Ok(Session(rs))
    }

    /// Create coding blocks by overwriting `parity`
    pub fn encode(&self, data: &[&[u8]], parity: &mut [&mut [u8]]) -> Result<()> {
        self.0.encode_sep(data, parity)?;

        Ok(())
    }

    /// Recover data + coding blocks into data blocks
    /// # Arguments
    /// * `data` - array of data blocks to recover into
    /// * `coding` - array of coding blocks
    /// * `erasures` - list of indices in data where blocks should be recovered
    pub fn decode_blocks(&self, blocks: &mut [(&mut [u8], bool)]) -> Result<()> {
        self.0.reconstruct(blocks)?;

        Ok(())
    }
}

impl Default for Session {
    fn default() -> Session {
        Session::new(NUM_DATA, NUM_CODING).unwrap()
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

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

    #[test]
    fn test_coding() {
        const N_DATA: usize = 4;
        const N_CODING: usize = 2;

        let session = Session::new(N_DATA, N_CODING).unwrap();

        let mut vs: Vec<Vec<u8>> = (0..N_DATA as u8).map(|i| (i..(16 + i)).collect()).collect();
        let v_orig: Vec<u8> = vs[0].clone();

        let mut coding_blocks: Vec<_> = (0..N_CODING).map(|_| vec![0u8; 16]).collect();

        let mut coding_blocks_slices: Vec<_> =
            coding_blocks.iter_mut().map(Vec::as_mut_slice).collect();
        let v_slices: Vec<_> = vs.iter().map(Vec::as_slice).collect();

        session
            .encode(v_slices.as_slice(), coding_blocks_slices.as_mut_slice())
            .expect("encoding must succeed");

        trace!("test_coding: coding blocks:");
        for b in &coding_blocks {
            trace!("test_coding: {:?}", b);
        }

        let erasure: usize = 1;
        let mut present = vec![true; N_DATA + N_CODING];
        present[erasure] = false;
        let erased = vs[erasure].clone();

        // clear an entry
        vs[erasure as usize].copy_from_slice(&[0; 16]);

        let mut blocks: Vec<_> = vs
            .iter_mut()
            .chain(coding_blocks.iter_mut())
            .map(Vec::as_mut_slice)
            .zip(present)
            .collect();

        session
            .decode_blocks(blocks.as_mut_slice())
            .expect("decoding must succeed");

        trace!("test_coding: vs:");
        for v in &vs {
            trace!("test_coding: {:?}", v);
        }
        assert_eq!(v_orig, vs[0]);
        assert_eq!(erased, vs[erasure]);
    }
}
