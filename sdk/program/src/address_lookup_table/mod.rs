//! The [address lookup table program][np].
//!
//! [np]: https://docs.solana.com/developing/runtime-facilities/programs#address-lookup-table-program

pub mod error;
pub mod instruction;

pub mod program {
    crate::declare_id!("AddressLookupTab1e1111111111111111111111111");
}
