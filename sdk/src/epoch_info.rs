//! Information about the current epoch.
//!
//! As returned by the [`getEpochInfo`] RPC method.
//!
//! [`getEpochInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getepochinfo

use crate::clock::{Epoch, Slot};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EpochInfo {
    /// The current epoch
    pub epoch: Epoch,

    /// The current slot, relative to the start of the current epoch
    pub slot_index: u64,

    /// The number of slots in this epoch
    pub slots_in_epoch: u64,

    /// The absolute current slot
    pub absolute_slot: Slot,

    /// The current block height
    pub block_height: u64,

    /// Total number of transactions processed without error since genesis
    pub transaction_count: Option<u64>,
}
