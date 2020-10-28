//! @brief Syscall stubs when building for programs for non-BPF targets

#![cfg(not(target_arch = "bpf"))]

use crate::{account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction};
use std::sync::{Arc, RwLock};

lazy_static::lazy_static! {
    static ref SYSCALL_STUBS: Arc<RwLock<Box<dyn SyscallStubs>>> = Arc::new(RwLock::new(Box::new(DefaultSyscallStubs {})));
}

// The default syscall stubs don't do much, but `set_syscalls()` can be used to swap in
// alternatives
pub fn set_syscall_stubs(syscall_stubs: Box<dyn SyscallStubs>) -> Box<dyn SyscallStubs> {
    std::mem::replace(&mut SYSCALL_STUBS.write().unwrap(), syscall_stubs)
}

pub trait SyscallStubs: Sync + Send {
    fn sol_log(&self, message: &str) {
        println!("{}", message);
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
}

struct DefaultSyscallStubs {}
impl SyscallStubs for DefaultSyscallStubs {}

pub(crate) fn sol_log(message: &str) {
    SYSCALL_STUBS.read().unwrap().sol_log(message);
}

pub(crate) fn sol_log_64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    sol_log(&format!("{} {} {} {} {}", arg1, arg2, arg3, arg4, arg5));
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
