use crate::Alloc;
use libc::c_char;
use log::*;
use solana_rbpf::{EbpfVmRaw, MemoryRegion};
use std::alloc::Layout;
use std::any::Any;
use std::ffi::CStr;
use std::io::{Error, ErrorKind};
use std::mem;
use std::slice::from_raw_parts;
use std::str::from_utf8;

/// Program heap allocators are intended to allocate/free from a given
/// chunk of memory.  The specific allocator implementation is
/// selectable at build-time.
/// Enable only one of the following BPFAllocator implementations.

/// Simple bump allocator, never frees
use crate::allocator_bump::BPFAllocator;

/// Use the system heap (test purposes only).  This allocator relies on the system heap
/// and there is no mechanism to check read-write access privileges
/// at the moment.  Therefor you must disable memory bounds checking
// use allocator_system::BPFAllocator;

/// Default program heap size, allocators
/// are expected to enforce this
const DEFAULT_HEAP_SIZE: usize = 32 * 1024;

pub fn register_helpers(vm: &mut EbpfVmRaw) -> Result<(MemoryRegion), Error> {
    vm.register_helper_ex("abort", Some(helper_abort_verify), helper_abort, None)?;
    vm.register_helper_ex(
        "sol_panic",
        Some(helper_sol_panic_verify),
        helper_sol_panic,
        None,
    )?;
    vm.register_helper_ex(
        "sol_panic_",
        Some(helper_sol_panic_verify),
        helper_sol_panic,
        None,
    )?;
    vm.register_helper_ex("sol_log", Some(helper_sol_log_verify), helper_sol_log, None)?;
    vm.register_helper_ex(
        "sol_log_",
        Some(helper_sol_log_verify_),
        helper_sol_log_,
        None,
    )?;
    vm.register_helper_ex("sol_log_64", None, helper_sol_log_u64, None)?;
    vm.register_helper_ex("sol_log_64_", None, helper_sol_log_u64, None)?;

    let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
    let heap_region = MemoryRegion::new_from_slice(&heap);
    let context = Box::new(BPFAllocator::new(heap));
    vm.register_helper_ex(
        "sol_alloc_free_",
        None,
        helper_sol_alloc_free,
        Some(context),
    )?;

    Ok(heap_region)
}

/// Verifies a string passed out of the program
fn verify_string(addr: u64, ro_regions: &[MemoryRegion]) -> Result<(()), Error> {
    for region in ro_regions.iter() {
        if region.addr <= addr && (addr as u64) < region.addr + region.len {
            let c_buf: *const c_char = addr as *const c_char;
            let max_size = region.addr + region.len - addr;
            unsafe {
                for i in 0..max_size {
                    if std::ptr::read(c_buf.offset(i as isize)) == 0 {
                        return Ok(());
                    }
                }
            }
            return Err(Error::new(ErrorKind::Other, "Error, Unterminated string"));
        }
    }
    Err(Error::new(
        ErrorKind::Other,
        "Error: Load segfault, bad string pointer",
    ))
}

/// Abort helper functions, called when the BPF program calls `abort()`
/// The verify function returns an error which will cause the BPF program
/// to be halted immediately
pub fn helper_abort_verify(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
    _ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<(()), Error> {
    Err(Error::new(
        ErrorKind::Other,
        "Error: BPF program called abort()!",
    ))
}
pub fn helper_abort(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    // Never called because its verify function always returns an error
    0
}

/// Panic helper functions, called when the BPF program calls 'sol_panic_()`
/// The verify function returns an error which will cause the BPF program
/// to be halted immediately
pub fn helper_sol_panic_verify(
    file: u64,
    line: u64,
    column: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<(()), Error> {
    if verify_string(file, ro_regions).is_ok() {
        let c_buf: *const c_char = file as *const c_char;
        let c_str: &CStr = unsafe { CStr::from_ptr(c_buf) };
        if let Ok(slice) = c_str.to_str() {
            return Err(Error::new(
                ErrorKind::Other,
                format!(
                    "Error: BPF program Panicked at {}, {}:{}",
                    slice, line, column
                ),
            ));
        }
    }
    Err(Error::new(ErrorKind::Other, "Error: BPF program Panicked"))
}
pub fn helper_sol_panic(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    // Never called because its verify function always returns an error
    0
}

/// Logging helper functions, called when the BPF program calls `sol_log_()` or
/// `sol_log_64_()`.
pub fn helper_sol_log_verify(
    addr: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<(()), Error> {
    verify_string(addr, ro_regions)
}
pub fn helper_sol_log(
    addr: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    let c_buf: *const c_char = addr as *const c_char;
    let c_str: &CStr = unsafe { CStr::from_ptr(c_buf) };
    match c_str.to_str() {
        Ok(slice) => info!("info!: {:?}", slice),
        Err(e) => warn!("Error: Cannot print invalid string: {}", e),
    };
    0
}
pub fn helper_sol_log_verify_(
    addr: u64,
    len: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<(()), Error> {
    for region in ro_regions.iter() {
        if region.addr <= addr && (addr as u64) + len <= region.addr + region.len {
            return Ok(());
        }
    }
    Err(Error::new(
        ErrorKind::Other,
        "Error: Load segfault, bad string pointer",
    ))
}
pub fn helper_sol_log_(
    addr: u64,
    len: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    let ptr: *const u8 = addr as *const u8;
    let message = unsafe { from_utf8(from_raw_parts(ptr, len as usize)) };
    info!("sol_log: {:?}", message);
    0
}
pub fn helper_sol_log_u64(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    info!(
        "info!: {:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
        arg1, arg2, arg3, arg4, arg5
    );
    0
}

/// Dynamic memory allocation helper called when the BPF program calls
/// `sol_alloc_free_()`.  The allocator is expected to allocate/free
/// from/to a given chunk of memory and enforce size restrictions.  The
/// memory chunk is given to the allocator during allocator creation and
/// information about that memory (start address and size) is passed
/// to the VM to use for enforcement.
pub fn helper_sol_alloc_free(
    size: u64,
    free_ptr: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    if let Some(context) = context {
        if let Some(allocator) = context.downcast_mut::<BPFAllocator>() {
            return {
                let layout = Layout::from_size_align(size as usize, mem::align_of::<u8>()).unwrap();
                if free_ptr == 0 {
                    match allocator.alloc(layout) {
                        Ok(ptr) => ptr as u64,
                        Err(_) => 0,
                    }
                } else {
                    allocator.dealloc(free_ptr as *mut u8, layout);
                    0
                }
            };
        };
    }
    panic!("Failed to get alloc_free context");
}
