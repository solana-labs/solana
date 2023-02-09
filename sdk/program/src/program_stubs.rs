//! Implementations of syscalls used when `solana-program` is built for non-SBF targets.

#![cfg(not(target_os = "solana"))]

use {
    crate::{
        account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction,
        program_error::UNSUPPORTED_SYSVAR, pubkey::Pubkey,
    },
    itertools::Itertools,
    std::sync::{Arc, RwLock},
};

lazy_static::lazy_static! {
    static ref SYSCALL_STUBS: Arc<RwLock<Box<dyn SyscallStubs>>> = Arc::new(RwLock::new(Box::new(DefaultSyscallStubs {})));
}

// The default syscall stubs may not do much, but `set_syscalls()` can be used
// to swap in alternatives
pub fn set_syscall_stubs(syscall_stubs: Box<dyn SyscallStubs>) -> Box<dyn SyscallStubs> {
    std::mem::replace(&mut SYSCALL_STUBS.write().unwrap(), syscall_stubs)
}

#[allow(clippy::integer_arithmetic)]
pub trait SyscallStubs: Sync + Send {
    fn sol_log(&self, message: &str) {
        println!("{message}");
    }
    fn sol_log_compute_units(&self) {
        sol_log("SyscallStubs: sol_log_compute_units() not available");
    }
    fn sol_invoke_signed(
        &self,
        _instruction: &Instruction,
        _account_infos: &[AccountInfo],
        _signers_seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        sol_log("SyscallStubs: sol_invoke_signed() not available");
        Ok(())
    }
    fn sol_get_clock_sysvar(&self, _var_addr: *mut u8) -> u64 {
        UNSUPPORTED_SYSVAR
    }
    fn sol_get_epoch_schedule_sysvar(&self, _var_addr: *mut u8) -> u64 {
        UNSUPPORTED_SYSVAR
    }
    fn sol_get_fees_sysvar(&self, _var_addr: *mut u8) -> u64 {
        UNSUPPORTED_SYSVAR
    }
    fn sol_get_rent_sysvar(&self, _var_addr: *mut u8) -> u64 {
        UNSUPPORTED_SYSVAR
    }
    /// # Safety
    unsafe fn sol_memcpy(&self, dst: *mut u8, src: *const u8, n: usize) {
        // cannot be overlapping
        assert!(
            is_nonoverlapping(src as usize, n, dst as usize, n),
            "memcpy does not support overlapping regions"
        );
        std::ptr::copy_nonoverlapping(src, dst, n);
    }
    /// # Safety
    unsafe fn sol_memmove(&self, dst: *mut u8, src: *const u8, n: usize) {
        std::ptr::copy(src, dst, n);
    }
    /// # Safety
    unsafe fn sol_memcmp(&self, s1: *const u8, s2: *const u8, n: usize, result: *mut i32) {
        let mut i = 0;
        while i < n {
            let a = *s1.add(i);
            let b = *s2.add(i);
            if a != b {
                *result = a as i32 - b as i32;
                return;
            }
            i += 1;
        }
        *result = 0
    }
    /// # Safety
    unsafe fn sol_memset(&self, s: *mut u8, c: u8, n: usize) {
        let s = std::slice::from_raw_parts_mut(s, n);
        for val in s.iter_mut().take(n) {
            *val = c;
        }
    }
    fn sol_get_return_data(&self) -> Option<(Pubkey, Vec<u8>)> {
        None
    }
    fn sol_set_return_data(&self, _data: &[u8]) {}
    fn sol_log_data(&self, fields: &[&[u8]]) {
        println!("data: {}", fields.iter().map(base64::encode).join(" "));
    }
    fn sol_get_processed_sibling_instruction(&self, _index: usize) -> Option<Instruction> {
        None
    }
    fn sol_get_stack_height(&self) -> u64 {
        0
    }
}

struct DefaultSyscallStubs {}
impl SyscallStubs for DefaultSyscallStubs {}

pub(crate) fn sol_log(message: &str) {
    SYSCALL_STUBS.read().unwrap().sol_log(message);
}

pub(crate) fn sol_log_64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    sol_log(&format!(
        "{arg1:#x}, {arg2:#x}, {arg3:#x}, {arg4:#x}, {arg5:#x}"
    ));
}

pub(crate) fn sol_log_compute_units() {
    SYSCALL_STUBS.read().unwrap().sol_log_compute_units();
}

pub(crate) fn sol_invoke_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    SYSCALL_STUBS
        .read()
        .unwrap()
        .sol_invoke_signed(instruction, account_infos, signers_seeds)
}

pub(crate) fn sol_get_clock_sysvar(var_addr: *mut u8) -> u64 {
    SYSCALL_STUBS.read().unwrap().sol_get_clock_sysvar(var_addr)
}

pub(crate) fn sol_get_epoch_schedule_sysvar(var_addr: *mut u8) -> u64 {
    SYSCALL_STUBS
        .read()
        .unwrap()
        .sol_get_epoch_schedule_sysvar(var_addr)
}

pub(crate) fn sol_get_fees_sysvar(var_addr: *mut u8) -> u64 {
    SYSCALL_STUBS.read().unwrap().sol_get_fees_sysvar(var_addr)
}

pub(crate) fn sol_get_rent_sysvar(var_addr: *mut u8) -> u64 {
    SYSCALL_STUBS.read().unwrap().sol_get_rent_sysvar(var_addr)
}

pub(crate) fn sol_memcpy(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        SYSCALL_STUBS.read().unwrap().sol_memcpy(dst, src, n);
    }
}

pub(crate) fn sol_memmove(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        SYSCALL_STUBS.read().unwrap().sol_memmove(dst, src, n);
    }
}

pub(crate) fn sol_memcmp(s1: *const u8, s2: *const u8, n: usize, result: *mut i32) {
    unsafe {
        SYSCALL_STUBS.read().unwrap().sol_memcmp(s1, s2, n, result);
    }
}

pub(crate) fn sol_memset(s: *mut u8, c: u8, n: usize) {
    unsafe {
        SYSCALL_STUBS.read().unwrap().sol_memset(s, c, n);
    }
}

pub(crate) fn sol_get_return_data() -> Option<(Pubkey, Vec<u8>)> {
    SYSCALL_STUBS.read().unwrap().sol_get_return_data()
}

pub(crate) fn sol_set_return_data(data: &[u8]) {
    SYSCALL_STUBS.read().unwrap().sol_set_return_data(data)
}

pub(crate) fn sol_log_data(data: &[&[u8]]) {
    SYSCALL_STUBS.read().unwrap().sol_log_data(data)
}

pub(crate) fn sol_get_processed_sibling_instruction(index: usize) -> Option<Instruction> {
    SYSCALL_STUBS
        .read()
        .unwrap()
        .sol_get_processed_sibling_instruction(index)
}

pub(crate) fn sol_get_stack_height() -> u64 {
    SYSCALL_STUBS.read().unwrap().sol_get_stack_height()
}

/// Check that two regions do not overlap.
///
/// Hidden to share with bpf_loader without being part of the API surface.
#[doc(hidden)]
pub fn is_nonoverlapping<N>(src: N, src_len: N, dst: N, dst_len: N) -> bool
where
    N: Ord + std::ops::Sub<Output = N>,
    <N as std::ops::Sub>::Output: Ord,
{
    // If the absolute distance between the ptrs is at least as big as the size of the other,
    // they do not overlap.
    if src > dst {
        src - dst >= dst_len
    } else {
        dst - src >= src_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_nonoverlapping() {
        for dst in 0..8 {
            assert!(is_nonoverlapping(10, 3, dst, 3));
        }
        for dst in 8..13 {
            assert!(!is_nonoverlapping(10, 3, dst, 3));
        }
        for dst in 13..20 {
            assert!(is_nonoverlapping(10, 3, dst, 3));
        }
        assert!(is_nonoverlapping::<u8>(255, 3, 254, 1));
        assert!(!is_nonoverlapping::<u8>(255, 2, 254, 3));
    }
}
