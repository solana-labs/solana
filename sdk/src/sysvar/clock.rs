//! This account contains the clock slot, epoch, and leader_schedule_epoch
//!
pub use crate::clock::Clock;

use crate::{
    account::Account,
    clock::{Epoch, Segment, Slot},
    sysvar::Sysvar,
};

crate::declare_sysvar_id!("SysvarC1ock11111111111111111111111111111111", Clock);

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
