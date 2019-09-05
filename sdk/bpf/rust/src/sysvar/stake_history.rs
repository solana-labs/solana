//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries history about stake activations and de-activations
//!
use crate::account::Account;
use bincode::serialized_size;
use std::collections::HashMap;
use std::ops::Deref;

pub use crate::timing::Epoch;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 53, 132, 208, 254, 237, 155, 179, 67, 29, 19, 32, 107, 229, 68, 40, 27,
    87, 184, 86, 108, 197, 55, 95, 244, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarStakeHistory1111111111111111111111111");

pub const MAX_STAKE_HISTORY: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistoryEntry {
    /// effective stake at this epoch
    pub effective: u64,
    /// sum of portion of stakes not fully warmed up
    pub activating: u64,
    /// requested to be cooled down, not fully deactivated yet
    pub deactivating: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistory {
    inner: HashMap<Epoch, StakeHistoryEntry>,
}

impl StakeHistory {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(
            &(0..MAX_STAKE_HISTORY)
                .map(|i| (i as u64, StakeHistoryEntry::default()))
                .collect::<HashMap<_, _>>(),
        )
        .unwrap() as usize
    }
}

impl Deref for StakeHistory {
    type Target = HashMap<Epoch, StakeHistoryEntry>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
