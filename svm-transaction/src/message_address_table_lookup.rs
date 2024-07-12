use solana_sdk::{message::v0, pubkey::Pubkey};

/// A non-owning version of [`v0::MessageAddressTableLookup`].
/// This simply references the data in the original message.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SVMMessageAddressTableLookup<'a> {
    /// Address lookup table account key
    pub account_key: &'a Pubkey,
    /// List of indexes used to load writable account addresses
    pub writable_indexes: &'a [u8],
    /// List of indexes used to load readonly account addresses
    pub readonly_indexes: &'a [u8],
}

impl<'a> From<&'a v0::MessageAddressTableLookup> for SVMMessageAddressTableLookup<'a> {
    fn from(lookup: &'a v0::MessageAddressTableLookup) -> Self {
        Self {
            account_key: &lookup.account_key,
            writable_indexes: &lookup.writable_indexes,
            readonly_indexes: &lookup.readonly_indexes,
        }
    }
}
