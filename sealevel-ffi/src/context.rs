use solana_rbpf::vm::SyscallRegistry;

/// The map of syscalls provided by the virtual machine.
pub type sealevel_syscall_registry = Box<SyscallRegistry>;

/// The invoke context holds the state of a single transaction execution.
/// It tracks the execution progress (instruction being executed),
/// interfaces with account data,
/// and specifies the on-chain execution rules (precompiles, syscalls, sysvars).
pub struct sealevel_invoke_context {}

/// Drops an invoke context and all programs created with it. Noop given a null pointer.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function twice on the same object.
#[no_mangle]
pub unsafe extern "C" fn sealevel_invoke_context_free(this: *mut sealevel_invoke_context) {
    if this.is_null() {
        return;
    }
    drop(Box::from_raw(this))
}
