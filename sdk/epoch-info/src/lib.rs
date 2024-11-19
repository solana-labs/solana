//! Information about the current epoch.
//!
//! As returned by the [`getEpochInfo`] RPC method.
//!
//! [`getEpochInfo`]: https://solana.com/docs/rpc/http/getepochinfo

#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Deserialize, serde_derive::Serialize),
    serde(rename_all = "camelCase")
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochInfo {
    /// The current epoch
    pub epoch: u64,

    /// The current slot, relative to the start of the current epoch
    pub slot_index: u64,

    /// The number of slots in this epoch
    pub slots_in_epoch: u64,

    /// The absolute current slot
    pub absolute_slot: u64,

    /// The current block height
    pub block_height: u64,

    /// Total number of transactions processed without error since genesis
    pub transaction_count: Option<u64>,
}
