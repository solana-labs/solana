use {
    crate::{accounts_file::ALIGN_BOUNDARY_OFFSET, u64_align},
    log::*,
    memmap2::Mmap,
};

pub fn get_type<T>(map: &Mmap, offset: usize) -> std::io::Result<(&T, usize)> {
    let (data, next) = get_slice(map, offset, std::mem::size_of::<T>())?;
    let ptr = data.as_ptr() as *const T;
    debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
    Ok((unsafe { &*ptr }, next))
}

/// Get a reference to the data at `offset` of `size` bytes if that slice
/// doesn't overrun the internal buffer. Otherwise return an Error.
/// Also return the offset of the first byte after the requested data that
/// falls on a 64-byte boundary.
pub fn get_slice(map: &Mmap, offset: usize, size: usize) -> std::io::Result<(&[u8], usize)> {
    let (next, overflow) = offset.overflowing_add(size);
    if overflow || next > map.len() {
        error!(
            "Requested offset {} and size {} while mmap only has length {}",
            offset,
            size,
            map.len()
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "Requested offset and data length exceeds the mmap slice",
        ));
    }
    let data = &map[offset..next];
    let next = u64_align!(next);
    let ptr = data.as_ptr() as *const u8;

    Ok((unsafe { std::slice::from_raw_parts(ptr, size) }, next))
}
