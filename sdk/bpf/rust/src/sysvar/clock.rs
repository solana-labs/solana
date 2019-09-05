//! This account contains the clock slot, epoch, and stakers_epoch
//!
use crate::account::Account;
use bincode::serialized_size;

pub use crate::timing::{Epoch, Segment, Slot};

const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184, 163, 155,
    75, 109, 92, 115, 85, 91, 33, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarC1ock11111111111111111111111111111111");

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Clock {
    pub slot: Slot,
    pub segment: Segment,
    pub epoch: Epoch,
    pub stakers_epoch: Epoch,
}

impl Clock {
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
