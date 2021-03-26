//! This account contains the clock slot, epoch, and leader_schedule_epoch
//!
pub use crate::clock::Clock;

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarC1ock11111111111111111111111111111111", Clock);

impl Sysvar for Clock {
    #[cfg(target_arch = "bpf")]
    fn call_syscall(var_addr: *mut u8) -> u64 {
        unsafe {
            extern "C" {
                fn sol_get_clock_sysvar(var_addr: *mut u8) -> u64;
            }
            sol_get_clock_sysvar(var_addr)
        }
    }
}
