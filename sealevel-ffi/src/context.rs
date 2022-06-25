use {solana_rbpf::vm::SyscallRegistry, std::ffi::c_void};

/// The map of syscalls provided by the virtual machine.
#[repr(transparent)]
pub struct sealevel_syscall_registry(pub(crate) *mut c_void);

/// Creates a new, empty syscall registry.
///
/// # Safety
/// Call `sealevel_syscall_registry_free` on the return value after you are done using it.
/// Failure to do so results in a memory leak.
#[no_mangle]
pub extern "C" fn sealevel_syscall_registry_new() -> sealevel_syscall_registry {
    let registry = SyscallRegistry::default();
    sealevel_syscall_registry(Box::into_raw(Box::new(registry)) as *mut c_void)
}

/// Frees a syscall registry.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function twice on the same object.
/// - Using the syscall registry after calling this function.
#[no_mangle]
pub unsafe extern "C" fn sealevel_syscall_registry_free(registry: sealevel_syscall_registry) {
    if registry.0.is_null() {
        return;
    }
    drop(Box::from_raw(registry.0 as *mut SyscallRegistry))
}

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
/// - Using the invoke context after calling this function.
#[no_mangle]
pub unsafe extern "C" fn sealevel_invoke_context_free(this: *mut sealevel_invoke_context) {
    if this.is_null() {
        return;
    }
    drop(Box::from_raw(this))
}
