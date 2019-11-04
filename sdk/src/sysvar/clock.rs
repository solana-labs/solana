//! This account contains the clock slot, epoch, and leader_schedule_epoch
//!
pub use crate::clock::Clock;

use crate::{
    account::Account,
    clock::{Epoch, Segment, Slot},
    sysvar::Sysvar,
};

const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184, 163, 155,
    75, 109, 92, 115, 85, 91, 33, 0, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarC1ock11111111111111111111111111111111", Clock);

impl Sysvar for Clock {}

pub fn create_account(
    lamports: u64,
    slot: Slot,
    segment: Segment,
    epoch: Epoch,
    leader_schedule_epoch: Epoch,
) -> Account {
    Clock {
        slot,
        segment,
        epoch,
        leader_schedule_epoch,
    }
    .create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let account = create_account(1, 0, 0, 0, 0);
        let clock = Clock::from_account(&account).unwrap();
        assert_eq!(clock, Clock::default());
    }
}
