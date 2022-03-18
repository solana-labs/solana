//! This account contains the current cluster rent
//!
pub use crate::rent::Rent;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::LegacySysvar};

crate::declare_sysvar_id!("SysvarRent111111111111111111111111111111111", Rent);

impl LegacySysvar for Rent {
    impl_sysvar_get!(sol_get_rent_sysvar);
}
