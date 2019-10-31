//! This account contains the current cluster rent
//!
pub use crate::epoch_schedule::EpochSchedule;
use crate::{account::Account, sysvar::Sysvar};

///  epoch_schedule account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 220, 63, 238, 2, 211, 228, 127, 1, 0, 248, 176, 84, 247, 148, 46, 96, 89,
    30, 63, 80, 135, 25, 168, 5, 0, 0, 0,
];

crate::solana_sysvar_id!(
    ID,
    "SysvarEpochSchedu1e111111111111111111111111",
    EpochSchedule
);

impl Sysvar for EpochSchedule {}

pub fn create_account(lamports: u64, epoch_schedule: &EpochSchedule) -> Account {
    epoch_schedule.create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(42, &EpochSchedule::default());
        let epoch_schedule = EpochSchedule::from_account(&account).unwrap();
        assert_eq!(epoch_schedule, EpochSchedule::default());
    }
}
