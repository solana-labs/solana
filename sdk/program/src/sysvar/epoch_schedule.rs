//! This account contains the current cluster rent
//!
pub use crate::epoch_schedule::EpochSchedule;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::LegacySysvar};

crate::declare_sysvar_id!("SysvarEpochSchedu1e111111111111111111111111", EpochSchedule);

impl LegacySysvar for EpochSchedule {
    impl_sysvar_get!(sol_get_epoch_schedule_sysvar);
}
