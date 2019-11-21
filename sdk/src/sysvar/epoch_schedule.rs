//! This account contains the current cluster rent
//!
pub use crate::epoch_schedule::EpochSchedule;
use crate::{account::Account, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarEpochSchedu1e111111111111111111111111", EpochSchedule);

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
