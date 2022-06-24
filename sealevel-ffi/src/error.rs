use {
    solana_bpf_loader_program::BpfError,
    solana_rbpf::error::EbpfError,
    std::{
        cell::RefCell,
        ffi::CString,
        os::raw::{c_char, c_int},
        ptr::null,
    },
};

pub(crate) type Error = EbpfError<BpfError>;

thread_local! {
    pub(crate) static ERROR: RefCell<Option<Error>> = RefCell::new(None);
}

pub const SEALEVEL_ERR_UNKNOWN: c_int = -1;
pub const SEALEVEL_OK: c_int = 0;
pub const SEALEVEL_ERR_INVALID_ELF: c_int = 1;
pub const SEALEVEL_ERR_SYSCALL_REGISTRATION: c_int = 2;
pub const SEALEVEL_ERR_CALL_DEPTH_EXCEEDED: c_int = 3;
pub const SEALEVEL_ERR_EXIT_ROOT_CALL_FRAME: c_int = 4;
pub const SEALEVEL_ERR_DIVIDE_BY_ZERO: c_int = 5;
pub const SEALEVEL_ERR_DIVIDE_OVERFLOW: c_int = 6;
pub const SEALEVEL_ERR_EXECUTION_OVERRUN: c_int = 7;
pub const SEALEVEL_ERR_CALL_OUTSIDE_TEXT_SEGMENT: c_int = 8;
pub const SEALEVEL_ERR_EXCEEDED_MAX_INSTRUCTIONS: c_int = 9;
pub const SEALEVEL_ERR_JIT_NOT_COMPILED: c_int = 10;
pub const SEALEVEL_ERR_INVALID_VIRTUAL_ADDRESS: c_int = 11;
pub const SEALEVEL_ERR_INVALID_MEMORY_REGION: c_int = 12;
pub const SEALEVEL_ERR_ACCESS_VIOLATION: c_int = 13;
pub const SEALEVEL_ERR_STACK_ACCESS_VIOLATION: c_int = 14;
pub const SEALEVEL_ERR_INVALID_INSTRUCTION: c_int = 15;
pub const SEALEVEL_ERR_UNSUPPORTED_INSTRUCTION: c_int = 16;
pub const SEALEVEL_ERR_ERR_EXHAUSTED_TEXT_SEGMENT: c_int = 17;
pub const SEALEVEL_ERR_LIBC_INVOCATION_FAILED: c_int = 18;
pub const SEALEVEL_ERR_VERIFIER_ERROR: c_int = 19;

/// Remembers the given optional error.
/// Returns the success value if there was no error.
pub(crate) fn set_result<T>(val: Result<T, Error>) -> Option<T> {
    let (ok, err) = match val {
        Ok(ok) => (Some(ok), None),
        Err(err) => (None, Some(err)),
    };
    ERROR.with(|err_cell| {
        *err_cell.borrow_mut() = err;
    });
    ok
}

/// Returns the error code of this thread's last seen error.
#[no_mangle]
pub extern "C" fn sealevel_errno() -> c_int {
    ERROR.with(|err_cell| match *err_cell.borrow() {
        None => SEALEVEL_OK,
        Some(EbpfError::ElfError(_)) => SEALEVEL_ERR_INVALID_ELF,
        Some(EbpfError::SycallAlreadyRegistered(_)) // Typo: https://github.com/solana-labs/rbpf/pull/317
        | Some(EbpfError::SyscallNotRegistered(_))
        | Some(EbpfError::SyscallAlreadyBound(_)) => SEALEVEL_ERR_SYSCALL_REGISTRATION,
        Some(EbpfError::CallDepthExceeded(_, _)) => SEALEVEL_ERR_CALL_DEPTH_EXCEEDED,
        Some(EbpfError::ExitRootCallFrame) => SEALEVEL_ERR_EXIT_ROOT_CALL_FRAME,
        Some(EbpfError::DivideByZero(_)) => SEALEVEL_ERR_DIVIDE_BY_ZERO,
        Some(Error::DivideOverflow(_)) => SEALEVEL_ERR_DIVIDE_OVERFLOW,
        Some(Error::ExecutionOverrun(_)) => SEALEVEL_ERR_EXECUTION_OVERRUN,
        Some(Error::CallOutsideTextSegment(_, _)) => SEALEVEL_ERR_CALL_OUTSIDE_TEXT_SEGMENT,
        Some(Error::ExceededMaxInstructions(_, _)) => SEALEVEL_ERR_EXCEEDED_MAX_INSTRUCTIONS,
        Some(Error::JitNotCompiled) => SEALEVEL_ERR_JIT_NOT_COMPILED,
        Some(Error::InvalidVirtualAddress(_)) => SEALEVEL_ERR_INVALID_VIRTUAL_ADDRESS,
        Some(Error::InvalidMemoryRegion(_)) => SEALEVEL_ERR_INVALID_MEMORY_REGION,
        Some(Error::AccessViolation(_, _, _, _, _)) => SEALEVEL_ERR_ACCESS_VIOLATION,
        Some(Error::StackAccessViolation(_, _, _, _, _)) => SEALEVEL_ERR_STACK_ACCESS_VIOLATION,
        Some(Error::InvalidInstruction(_)) => SEALEVEL_ERR_INVALID_INSTRUCTION,
        Some(Error::UnsupportedInstruction(_)) => SEALEVEL_ERR_UNSUPPORTED_INSTRUCTION,
        Some(Error::ExhausedTextSegment(_)) => SEALEVEL_ERR_ERR_EXHAUSTED_TEXT_SEGMENT,
        Some(Error::LibcInvocationFailed(_, _, _)) => SEALEVEL_ERR_LIBC_INVOCATION_FAILED,
        Some(Error::VerifierError(_)) => SEALEVEL_ERR_VERIFIER_ERROR,
        // TODO other errors
        Some(_) => SEALEVEL_ERR_UNKNOWN,
    })
}

/// Returns a UTF-8 string of this thread's last seen error,
/// or NULL if `sealevel_errno() == SEALEVEL_OK`.
///
/// # Safety
/// Call `sealevel_strerror_free` on the return value after you are done using it.
/// Failure to do so results in a memory leak.
#[no_mangle]
pub extern "C" fn sealevel_strerror() -> *const c_char {
    let c_string = ERROR.with(|err_cell| {
        err_cell
            .borrow()
            .as_ref()
            .map(|err| CString::new(format!("{:?}", err)).expect(""))
    });
    match c_string {
        None => null(),
        Some(str) => str.into_raw() as *const c_char,
    }
}

/// Frees an unused error string gained from `sealevel_strerror`.
/// Calling this with a NULL pointer is a no-op.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function given a string that's _not_ the return value of `sealevel_strerror`.
/// - Calling this function more than once on the same string (double free).
/// - Using a string after calling this function (use-after-free).
#[no_mangle]
pub unsafe extern "C" fn sealevel_strerror_free(str: *const c_char) {
    if str.is_null() {
        return;
    }
    drop(CString::from_raw(str as *mut c_char))
}
