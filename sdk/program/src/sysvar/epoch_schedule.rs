//! This account contains the current cluster rent
//!
pub use crate::epoch_schedule::EpochSchedule;
use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarEpochSchedu1e111111111111111111111111", EpochSchedule);

impl Sysvar for EpochSchedule {}
