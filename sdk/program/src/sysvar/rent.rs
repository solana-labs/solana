//! This account contains the current cluster rent
//!
pub use crate::rent::Rent;

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarRent111111111111111111111111111111111", Rent);

impl Sysvar for Rent {}
