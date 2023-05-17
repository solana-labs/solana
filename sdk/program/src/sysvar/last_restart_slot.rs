//! Information about the last restart slot (hard fork).
//!
//! TODO
//! See also the Solana [SIMD proposal][sdoc].
//!
//! [sdoc]: https://docs.solana.com/developing/runtime-facilities/sysvars#clock
//!

pub use crate::last_restart_slot::LastRestartSlot;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar};

crate::declare_sysvar_id!(
    "SysvarLastRestartS1ot1111111111111111111111",
    LastRestartSlot
);

impl Sysvar for LastRestartSlot {
    impl_sysvar_get!(sol_get_last_restart_slot_sysvar);
}
