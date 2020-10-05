//! This account contains epoch timestamp information
//!
use crate::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    sysvar::Sysvar,
};
use std::collections::HashMap;

crate::declare_sysvar_id!(
    "SysvarEpochTimestamps1111111111111111111111",
    EpochTimestamps
);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct EpochTimestamps {
    pub first_slot: Slot,
    pub timestamp: UnixTimestamp,
    pub samples: HashMap<Slot, Option<UnixTimestamp>>,
}

impl Sysvar for EpochTimestamps {}

pub fn create_account(lamports: u64, first_slot: Slot, timestamp: UnixTimestamp) -> Account {
    EpochTimestamps {
        first_slot,
        timestamp,
        ..EpochTimestamps::default()
    }
    .create_account(lamports)
}
