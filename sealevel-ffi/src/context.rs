use {
    crate::program::sealevel_instruction_account, solana_rbpf::vm::SyscallRegistry,
    std::os::raw::c_char,
};

/// The map of syscalls provided by the virtual machine.
pub type sealevel_syscall_registry = Box<SyscallRegistry>;

/// The invoke context holds the state of a single transaction execution.
/// It tracks the execution progress (instruction being executed),
/// interfaces with account data,
/// and specifies the on-chain execution rules (precompiles, syscalls, sysvars).
pub struct sealevel_invoke_context {}

/// Drops an invoke context and all programs created with it.
#[no_mangle]
pub unsafe extern "C" fn sealevel_invoke_context_free(this: *mut sealevel_invoke_context) {
    drop(Box::from_raw(this))
}

/// Processes a transaction instruction.
///
/// Sets `sealevel_errno`.
#[no_mangle]
pub unsafe extern "C" fn sealevel_process_instruction(
    invoke_context: *mut sealevel_invoke_context,
    data: *const c_char,
    data_len: usize,
    accounts: *const sealevel_instruction_account,
    accounts_len: usize,
    compute_units_consumed: *mut u64,
) {
    unimplemented!()
}
