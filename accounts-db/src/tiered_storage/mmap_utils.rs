use {
    crate::{accounts_file::ALIGN_BOUNDARY_OFFSET, u64_align},
    log::*,
    memmap2::Mmap,
    std::io::Result as IoResult,
};

/// Borrows a value of type `T` from `mmap`
///
/// Type T must be plain ol' data to ensure no undefined behavior.
pub fn get_pod<T: bytemuck::AnyBitPattern>(mmap: &Mmap, offset: usize) -> IoResult<(&T, usize)> {
    // SAFETY: Since T is AnyBitPattern, it is safe to cast bytes to T.
    unsafe { get_type::<T>(mmap, offset) }
}

/// Borrows a value of type `T` from `mmap`
///
/// Prefer `get_pod()` when possible, because `get_type()` may cause undefined behavior.
///
/// # Safety
///
/// Caller must ensure casting bytes to T is safe.
/// Refer to the Safety sections in std::slice::from_raw_parts()
/// and bytemuck's Pod and AnyBitPattern for more information.
pub unsafe fn get_type<T>(mmap: &Mmap, offset: usize) -> IoResult<(&T, usize)> {
    let (data, next) = get_slice(mmap, offset, std::mem::size_of::<T>())?;
    let ptr = data.as_ptr() as *const T;
    debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
    // SAFETY: The caller ensures it is safe to cast bytes to T,
    // we ensure the size is safe by querying T directly,
    // and we just checked above to ensure the ptr is aligned for T.
    Ok((unsafe { &*ptr }, next))
}

/// Get a reference to the data at `offset` of `size` bytes if that slice
/// doesn't overrun the internal buffer. Otherwise return an Error.
/// Also return the offset of the first byte after the requested data that
/// falls on a 64-byte boundary.
pub fn get_slice(mmap: &Mmap, offset: usize, size: usize) -> IoResult<(&[u8], usize)> {
    let (next, overflow) = offset.overflowing_add(size);
    if overflow || next > mmap.len() {
        error!(
            "Requested offset {} and size {} while mmap only has length {}",
            offset,
            size,
            mmap.len()
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "Requested offset and data length exceeds the mmap slice",
        ));
    }
    let data = &mmap[offset..next];
    let next = u64_align!(next);
    let ptr = data.as_ptr();

    // SAFETY: The Mmap ensures the bytes are safe the read, and we just checked
    // to ensure we don't read past the end of the internal buffer.
    Ok((unsafe { std::slice::from_raw_parts(ptr, size) }, next))
}
