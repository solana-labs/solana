//! Basic low-level memory operations.
//!
//! Within the SBF environment, these are implemented as syscalls and executed by
//! the runtime in native code.

#[cfg(target_os = "solana")]
pub mod syscalls {
    use solana_define_syscall::define_syscall;
    define_syscall!(fn sol_memcpy_(dst: *mut u8, src: *const u8, n: u64));
    define_syscall!(fn sol_memmove_(dst: *mut u8, src: *const u8, n: u64));
    define_syscall!(fn sol_memcmp_(s1: *const u8, s2: *const u8, n: u64, result: *mut i32));
    define_syscall!(fn sol_memset_(s: *mut u8, c: u8, n: u64));
}

/// Check that two regions do not overlap.
///
/// Hidden to share with bpf_loader without being part of the API surface.
#[doc(hidden)]
pub fn is_nonoverlapping<N>(src: N, src_len: N, dst: N, dst_len: N) -> bool
where
    N: Ord + num_traits::SaturatingSub,
{
    // If the absolute distance between the ptrs is at least as big as the size of the other,
    // they do not overlap.
    if src > dst {
        src.saturating_sub(&dst) >= dst_len
    } else {
        dst.saturating_sub(&src) >= src_len
    }
}

#[cfg(not(target_os = "solana"))]
#[allow(clippy::arithmetic_side_effects)]
pub mod stubs {
    use super::is_nonoverlapping;
    /// # Safety
    pub unsafe fn sol_memcpy(dst: *mut u8, src: *const u8, n: usize) {
        // cannot be overlapping
        assert!(
            is_nonoverlapping(src as usize, n, dst as usize, n),
            "memcpy does not support overlapping regions"
        );
        std::ptr::copy_nonoverlapping(src, dst, n);
    }
    /// # Safety
    pub unsafe fn sol_memmove(dst: *mut u8, src: *const u8, n: usize) {
        std::ptr::copy(src, dst, n);
    }
    /// # Safety
    pub unsafe fn sol_memcmp(s1: *const u8, s2: *const u8, n: usize, result: *mut i32) {
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
    pub unsafe fn sol_memset(s: *mut u8, c: u8, n: usize) {
        let s = std::slice::from_raw_parts_mut(s, n);
        for val in s.iter_mut().take(n) {
            *val = c;
        }
    }
}

/// Like C `memcpy`.
///
/// # Arguments
///
/// - `dst` - Destination
/// - `src` - Source
/// - `n` - Number of bytes to copy
///
/// # Errors
///
/// When executed within a SBF program, the memory regions spanning `n` bytes
/// from from the start of `dst` and `src` must be mapped program memory. If not,
/// the program will abort.
///
/// The memory regions spanning `n` bytes from `dst` and `src` from the start
/// of `dst` and `src` must not overlap. If they do, then the program will abort
/// or, if run outside of the SBF VM, will panic.
///
/// # Safety
///
/// __This function is incorrectly missing an `unsafe` declaration.__
///
/// This function does not verify that `n` is less than or equal to the
/// lengths of the `dst` and `src` slices passed to it &mdash; it will copy
/// bytes to and from beyond the slices.
///
/// Specifying an `n` greater than either the length of `dst` or `src` will
/// likely introduce undefined behavior.
#[inline]
pub fn sol_memcpy(dst: &mut [u8], src: &[u8], n: usize) {
    #[cfg(target_os = "solana")]
    unsafe {
        syscalls::sol_memcpy_(dst.as_mut_ptr(), src.as_ptr(), n as u64);
    }

    #[cfg(not(target_os = "solana"))]
    unsafe {
        stubs::sol_memcpy(dst.as_mut_ptr(), src.as_ptr(), n);
    }
}

/// Like C `memmove`.
///
/// # Arguments
///
/// - `dst` - Destination
/// - `src` - Source
/// - `n` - Number of bytes to copy
///
/// # Errors
///
/// When executed within a SBF program, the memory regions spanning `n` bytes
/// from from `dst` and `src` must be mapped program memory. If not, the program
/// will abort.
///
/// # Safety
///
/// The same safety rules apply as in [`ptr::copy`].
///
/// [`ptr::copy`]: https://doc.rust-lang.org/std/ptr/fn.copy.html
#[inline]
pub unsafe fn sol_memmove(dst: *mut u8, src: *mut u8, n: usize) {
    #[cfg(target_os = "solana")]
    syscalls::sol_memmove_(dst, src, n as u64);

    #[cfg(not(target_os = "solana"))]
    stubs::sol_memmove(dst, src, n);
}

/// Like C `memcmp`.
///
/// # Arguments
///
/// - `s1` - Slice to be compared
/// - `s2` - Slice to be compared
/// - `n` - Number of bytes to compare
///
/// # Errors
///
/// When executed within a SBF program, the memory regions spanning `n` bytes
/// from from the start of `dst` and `src` must be mapped program memory. If not,
/// the program will abort.
///
/// # Safety
///
/// __This function is incorrectly missing an `unsafe` declaration.__
///
/// It does not verify that `n` is less than or equal to the lengths of the
/// `dst` and `src` slices passed to it &mdash; it will read bytes beyond the
/// slices.
///
/// Specifying an `n` greater than either the length of `dst` or `src` will
/// likely introduce undefined behavior.
#[inline]
pub fn sol_memcmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    let mut result = 0;

    #[cfg(target_os = "solana")]
    unsafe {
        syscalls::sol_memcmp_(s1.as_ptr(), s2.as_ptr(), n as u64, &mut result as *mut i32);
    }

    #[cfg(not(target_os = "solana"))]
    unsafe {
        stubs::sol_memcmp(s1.as_ptr(), s2.as_ptr(), n, &mut result as *mut i32);
    }

    result
}

/// Like C `memset`.
///
/// # Arguments
///
/// - `s` - Slice to be set
/// - `c` - Repeated byte to set
/// - `n` - Number of bytes to set
///
/// # Errors
///
/// When executed within a SBF program, the memory region spanning `n` bytes
/// from from the start of `s` must be mapped program memory. If not, the program
/// will abort.
///
/// # Safety
///
/// __This function is incorrectly missing an `unsafe` declaration.__
///
/// This function does not verify that `n` is less than or equal to the length
/// of the `s` slice passed to it &mdash; it will write bytes beyond the
/// slice.
///
/// Specifying an `n` greater than the length of `s` will likely introduce
/// undefined behavior.
#[inline]
pub fn sol_memset(s: &mut [u8], c: u8, n: usize) {
    #[cfg(target_os = "solana")]
    unsafe {
        syscalls::sol_memset_(s.as_mut_ptr(), c, n as u64);
    }

    #[cfg(not(target_os = "solana"))]
    unsafe {
        stubs::sol_memset(s.as_mut_ptr(), c, n);
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
