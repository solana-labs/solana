//! This account contains the current cluster rent
//!
pub use crate::epoch_schedule::EpochSchedule;
use crate::{
    impl_sysvar_get,
    program_error::ProgramError,
    sysvar::{Sysvar, SysvarType},
};

crate::declare_sysvar_id!("SysvarEpochSchedu1e111111111111111111111111", EpochSchedule);

impl Sysvar for EpochSchedule {
    const TYPE: SysvarType = SysvarType::EpochSchedule;
    impl_sysvar_get!(sol_get_epoch_schedule_sysvar);
}
