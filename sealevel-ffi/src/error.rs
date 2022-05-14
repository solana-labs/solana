use {
    solana_bpf_loader_program::BpfError,
    solana_rbpf::error::EbpfError,
    std::{
        borrow::{Borrow, BorrowMut},
        cell::{Ref, RefCell},
        ffi::CString,
        os::raw::{c_char, c_int},
        ptr::null_mut,
    },
};

pub(crate) type Error = EbpfError<BpfError>;

thread_local! {
    pub(crate) static ERROR: RefCell<Option<Error>> = RefCell::new(None);
}

pub const SEALEVEL_OK: c_int = 0;
pub const SEALEVEL_ERR_INVALID_ELF: c_int = 1;
pub const SEALEVEL_ERR_SYSCALL_REGISTRATION: c_int = 2;
pub const SEALEVEL_ERR_CALL_DEPTH_EXCEEDED: c_int = 3;
pub const SEALEVEL_ERR_UNKNOWN: c_int = -1;

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
        // TODO other errors
        Some(_) => SEALEVEL_ERR_UNKNOWN,
    })
}

/// Returns a UTF-8 string of this thread's last seen error,
/// or NULL if `sealevel_errno() == SEALEVEL_OK`.
///
/// Must be released using `sealevel_strerror_free` after use.
#[no_mangle]
pub extern "C" fn sealevel_strerror() -> *mut c_char {
    let c_string = ERROR.with(|err_cell| {
        err_cell
            .borrow()
            .as_ref()
            .map(|err| CString::new(format!("{:?}", err)).expect(""))
    });
    match c_string {
        None => null_mut(),
        Some(str) => str.into_raw(),
    }
}

/// Frees an unused error string gained from `sealevel_strerror`.
/// Calling this with a NULL pointer is a no-op.
#[no_mangle]
pub unsafe extern "C" fn sealevel_strerror_free(str: *mut c_char) {
    if str.is_null() {
        return;
    }
    drop(CString::from_raw(str))
}
