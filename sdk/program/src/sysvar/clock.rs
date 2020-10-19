//! This account contains the clock slot, epoch, and leader_schedule_epoch
//!
pub use crate::clock::Clock;

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarC1ock11111111111111111111111111111111", Clock);

impl Sysvar for Clock {}
