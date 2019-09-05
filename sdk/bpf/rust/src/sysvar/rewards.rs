//! This account contains the current cluster rewards point values
//!
use crate::account::Account;
use bincode::serialized_size;

/// account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 44, 97, 55, 206, 224, 146, 217, 182, 146, 62, 225, 204, 214, 25, 3, 250,
    130, 184, 161, 97, 145, 87, 141, 128, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarRewards111111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub storage_point_value: f64,
}

impl Rewards {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }
    pub fn size_of() -> usize {
        serialized_size(&Self::default()).unwrap() as usize
    }
}
