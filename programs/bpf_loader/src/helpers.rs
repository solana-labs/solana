use crate::{alloc, BPFError};
use alloc::Alloc;
use log::*;
use solana_rbpf::{
    ebpf::{EbpfError, HelperObject, ELF_INSN_DUMP_OFFSET, MM_HEAP_START},
    memory_region::{translate_addr, MemoryRegion},
    EbpfVm,
};
use solana_sdk::instruction::InstructionError;
use std::{
    alloc::Layout,
    mem::{align_of, size_of},
    slice::from_raw_parts_mut,
    str::{from_utf8, Utf8Error},
};
use thiserror::Error as ThisError;

/// Error definitions
#[derive(Debug, ThisError)]
pub enum HelperError {
    #[error("{0}: {1:?}")]
    InvalidString(Utf8Error, Vec<u8>),
    #[error("BPF program called abort()!")]
    Abort,
    #[error("BPF program Panicked at {0}, {1}:{2}")]
    Panic(String, u64, u64),
    #[error("{0}")]
    InstructionError(InstructionError),
}
impl From<HelperError> for EbpfError<BPFError> {
    fn from(error: HelperError) -> Self {
        EbpfError::UserError(error.into())
    }
}

/// Program heap allocators are intended to allocate/free from a given
/// chunk of memory.  The specific allocator implementation is
/// selectable at build-time.
/// Only one allocator is currently supported

/// Simple bump allocator, never frees
use crate::allocator_bump::BPFAllocator;

/// Default program heap size, allocators
/// are expected to enforce this
const DEFAULT_HEAP_SIZE: usize = 32 * 1024;

pub fn register_helpers<'a>(
    vm: &mut EbpfVm<'a, BPFError>,
) -> Result<MemoryRegion, EbpfError<BPFError>> {
    vm.register_helper_ex("abort", helper_abort)?;
    vm.register_helper_ex("sol_panic", helper_sol_panic)?;
    vm.register_helper_ex("sol_panic_", helper_sol_panic)?;
    vm.register_helper_ex("sol_log", helper_sol_log)?;
    vm.register_helper_ex("sol_log_", helper_sol_log)?;
    vm.register_helper_ex("sol_log_64", helper_sol_log_u64)?;
    vm.register_helper_ex("sol_log_64_", helper_sol_log_u64)?;

    let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
    let heap_region = MemoryRegion::new_from_slice(&heap, MM_HEAP_START);
    vm.register_helper_with_context_ex(
        "sol_alloc_free_",
        Box::new(HelperSolAllocFree {
            allocator: BPFAllocator::new(heap, MM_HEAP_START),
        }),
    )?;

    Ok(heap_region)
}

#[macro_export]
macro_rules! translate {
    ($vm_addr:expr, $len:expr, $regions:expr) => {
        translate_addr(
            $vm_addr as u64,
            $len as usize,
            file!(),
            line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
            $regions,
        )?
    };
}

#[macro_export]
macro_rules! translate_type_mut {
    ($t:ty, $vm_addr:expr, $regions:expr) => {
        unsafe {
            &mut *(translate_addr(
                $vm_addr as u64,
                size_of::<$t>(),
                file!(),
                line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
                $regions,
            )? as *mut $t)
        }
    };
}
#[macro_export]
macro_rules! translate_type {
    ($t:ty, $vm_addr:expr, $regions:expr) => {
        &*translate_type_mut!($t, $vm_addr, $regions)
    };
}

#[macro_export]
macro_rules! translate_slice_mut {
    ($t:ty, $vm_addr:expr, $len: expr, $regions:expr) => {{
        let host_addr = translate_addr(
            $vm_addr as u64,
            $len as usize * size_of::<$t>(),
            file!(),
            line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
            $regions,
        )? as *mut $t;
        unsafe { from_raw_parts_mut(host_addr, $len as usize) }
    }};
}
#[macro_export]
macro_rules! translate_slice {
    ($t:ty, $vm_addr:expr, $len: expr, $regions:expr) => {
        &*translate_slice_mut!($t, $vm_addr, $len, $regions)
    };
}

/// Take a virtual pointer to a string (points to BPF VM memory space), translate it
/// pass it to a user-defined work function
fn translate_string_and_do(
    addr: u64,
    len: u64,
    regions: &[MemoryRegion],
    work: &dyn Fn(&str) -> Result<u64, EbpfError<BPFError>>,
) -> Result<u64, EbpfError<BPFError>> {
    let buf = translate_slice!(u8, addr, len, regions);
    let i = match buf.iter().position(|byte| *byte == 0) {
        Some(i) => i,
        None => len as usize,
    };
    match from_utf8(&buf[..i]) {
        Ok(message) => work(message),
        Err(err) => Err(HelperError::InvalidString(err, buf[..i].to_vec()).into()),
    }
}

/// Abort helper functions, called when the BPF program calls `abort()`
/// The verify function returns an error which will cause the BPF program
/// to be halted immediately
pub fn helper_abort(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    Err(HelperError::Abort.into())
}

/// Panic helper functions, called when the BPF program calls 'sol_panic_()`
/// The verify function returns an error which will cause the BPF program
/// to be halted immediately
pub fn helper_sol_panic(
    file: u64,
    len: u64,
    line: u64,
    column: u64,
    _arg5: u64,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    translate_string_and_do(file, len, ro_regions, &|string: &str| {
        Err(HelperError::Panic(string.to_string(), line, column).into())
    })
}

/// Log a user's info message
pub fn helper_sol_log(
    addr: u64,
    len: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    if log_enabled!(log::Level::Info) {
        translate_string_and_do(addr, len, ro_regions, &|string: &str| {
            info!("info!: {}", string);
            Ok(0)
        })?;
    }
    Ok(0)
}

/// Log 5 u64 values
pub fn helper_sol_log_u64(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    _ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    if log_enabled!(log::Level::Info) {
        info!(
            "info!: {:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
            arg1, arg2, arg3, arg4, arg5
        );
    }
    Ok(0)
}

/// Dynamic memory allocation helper called when the BPF program calls
/// `sol_alloc_free_()`.  The allocator is expected to allocate/free
/// from/to a given chunk of memory and enforce size restrictions.  The
/// memory chunk is given to the allocator during allocator creation and
/// information about that memory (start address and size) is passed
/// to the VM to use for enforcement.
pub struct HelperSolAllocFree {
    allocator: BPFAllocator,
}
impl HelperObject<BPFError> for HelperSolAllocFree {
    fn call(
        &mut self,
        size: u64,
        free_addr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _ro_regions: &[MemoryRegion],
        _rw_regions: &[MemoryRegion],
    ) -> Result<u64, EbpfError<BPFError>> {
        let layout = Layout::from_size_align(size as usize, align_of::<u8>()).unwrap();
        if free_addr == 0 {
            match self.allocator.alloc(layout) {
                Ok(addr) => Ok(addr as u64),
                Err(_) => Ok(0),
            }
        } else {
            self.allocator.dealloc(free_addr, layout);
            Ok(0)
        }
    }
}
