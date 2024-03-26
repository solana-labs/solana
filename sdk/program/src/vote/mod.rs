//! The [vote native program][np].
//!
//! [np]: https://docs.solanalabs.com/runtime/programs#vote-program

pub mod authorized_voters;
pub mod error;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("Vote111111111111111111111111111111111111111");
}
