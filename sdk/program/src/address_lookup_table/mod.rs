//! The [address lookup table program][np].
//!
//! [np]: https://docs.solana.com/developing/runtime-facilities/programs#address-lookup-table-program

pub mod error;
pub mod instruction;
pub mod state;

pub mod program {
    crate::declare_id!("AddressLookupTab1e1111111111111111111111111");
}

/// The definition of address lookup table accounts.
///
/// As used by the `crate::message::v0` message format.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AddressLookupTableAccount {
    pub key: crate::pubkey::Pubkey,
    pub addresses: Vec<crate::pubkey::Pubkey>,
}
