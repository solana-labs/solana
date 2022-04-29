//! # Erasure Coding and Recovery
//!
//! Shreds are logically grouped into erasure sets or blocks. Each set contains 16 sequential data
//! shreds and 4 sequential coding shreds.
//!
//! Coding shreds in each set starting from `start_idx`:
//!   For each erasure set:
//!     generate `NUM_CODING` coding_shreds.
//!     index the coding shreds from `start_idx` to `start_idx + NUM_CODING - 1`.
//!
//!  model of an erasure set, with top row being data shreds and second being coding
//!  |<======================= NUM_DATA ==============================>|
//!  |<==== NUM_CODING ===>|
//!  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//!  | D | | D | | D | | D | | D |         | D | | D | | D | | D | | D |
//!  +---+ +---+ +---+ +---+ +---+  . . .  +---+ +---+ +---+ +---+ +---+
//!  | C | | C | | C | | C | |   |         |   | |   | |   | |   | |   |
//!  +---+ +---+ +---+ +---+ +---+         +---+ +---+ +---+ +---+ +---+
//!
//!  shred structure for coding shreds
//!
//!   + ------- meta is set and used by transport, meta.size is actual length
//!   |           of data in the byte array shred.data
//!   |
//!   |          + -- data is stuff shipped over the wire, and has an included
//!   |          |        header
//!   V          V
//!  +----------+------------------------------------------------------------+
//!  | meta     |  data                                                      |
//!  |+---+--   |+---+---+---+---+------------------------------------------+|
//!  || s | .   || i |   | f | s |                                          ||
//!  || i | .   || n | i | l | i |                                          ||
//!  || z | .   || d | d | a | z |     shred.data(), or shred.data_mut()      ||
//!  || e |     || e |   | g | e |                                          ||
//!  |+---+--   || x |   | s |   |                                          ||
//!  |          |+---+---+---+---+------------------------------------------+|
//!  +----------+------------------------------------------------------------+
//!             |                |<=== coding shred part for "coding" =======>|
//!             |                                                            |
//!             |<============== data shred part for "coding"  ==============>|
//!
//!

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasureConfig {
    num_data: usize,
    num_coding: usize,
}

impl ErasureConfig {
    pub(crate) fn new(num_data: usize, num_coding: usize) -> ErasureConfig {
        ErasureConfig {
            num_data,
            num_coding,
        }
    }

    pub(crate) fn num_data(self) -> usize {
        self.num_data
    }

    pub(crate) fn num_coding(self) -> usize {
        self.num_coding
    }
}
