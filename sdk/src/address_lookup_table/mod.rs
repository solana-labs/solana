//! The [address lookup table program][np].
//!
//! [np]: https://docs.solanalabs.com/runtime/programs#address-lookup-table-program

pub mod error;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("AddressLookupTab1e1111111111111111111111111");
}
