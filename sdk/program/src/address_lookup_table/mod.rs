//! The [address lookup table program][np].
//!
//! [np]: https://docs.solanalabs.com/runtime/programs#address-lookup-table-program

pub mod error;
pub mod instruction;
pub mod state;

pub mod program {
    pub use solana_sdk_ids::address_lookup_table::{check_id, id, ID};
}

/// The definition of address lookup table accounts.
///
/// As used by the `crate::message::v0` message format.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AddressLookupTableAccount {
    pub key: crate::pubkey::Pubkey,
    pub addresses: Vec<crate::pubkey::Pubkey>,
}
