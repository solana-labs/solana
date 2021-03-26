//! This account contains the current cluster rent
//!
pub use crate::rent::Rent;

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarRent111111111111111111111111111111111", Rent);

impl Sysvar for Rent {
    #[cfg(target_arch = "bpf")]
    fn call_syscall(var_addr: *mut u8) -> u64 {
        unsafe {
            extern "C" {
                fn sol_get_rent_sysvar(var_addr: *mut u8) -> u64;
            }
            sol_get_rent_sysvar(var_addr)
        }
    }
}
