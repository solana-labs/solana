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
        translate_addr::<BPFError>(
            $vm_addr as u64,
            $len as usize,
            file!(),
            line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
            $regions,
        )
    };
}

#[macro_export]
macro_rules! translate_type_mut {
    ($t:ty, $vm_addr:expr, $regions:expr) => {
        unsafe {
            match translate_addr::<BPFError>(
                $vm_addr as u64,
                size_of::<$t>(),
                file!(),
                line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
                $regions,
            ) {
                Ok(value) => Ok(&mut *(value as *mut $t)),
                Err(e) => Err(e),
            }
        }
    };
}
#[macro_export]
macro_rules! translate_type {
    ($t:ty, $vm_addr:expr, $regions:expr) => {
        match translate_type_mut!($t, $vm_addr, $regions) {
            Ok(value) => Ok(&*value),
            Err(e) => Err(e),
        }
    };
}

#[macro_export]
macro_rules! translate_slice_mut {
    ($t:ty, $vm_addr:expr, $len: expr, $regions:expr) => {
        match translate_addr::<BPFError>(
            $vm_addr as u64,
            $len as usize * size_of::<$t>(),
            file!(),
            line!() as usize - ELF_INSN_DUMP_OFFSET + 1,
            $regions,
        ) {
            Ok(value) => Ok(unsafe { from_raw_parts_mut(value as *mut $t, $len as usize) }),
            Err(e) => Err(e),
        }
    };
}
#[macro_export]
macro_rules! translate_slice {
    ($t:ty, $vm_addr:expr, $len: expr, $regions:expr) => {
        match translate_slice_mut!($t, $vm_addr, $len, $regions) {
            Ok(value) => Ok(&*value),
            Err(e) => Err(e),
        }
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
    let buf = translate_slice!(u8, addr, len, regions)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    };

    #[test]
    fn test_translate() {
        const START: u64 = 100;
        const LENGTH: u64 = 1000;
        let data = vec![0u8; LENGTH as usize];
        let addr = data.as_ptr() as u64;
        let regions = vec![MemoryRegion::new_from_slice(&data, START)];

        let cases = vec![
            (true, START, 0, addr),
            (true, START, 1, addr),
            (true, START, LENGTH, addr),
            (true, START + 1, LENGTH - 1, addr + 1),
            (false, START + 1, LENGTH, 0),
            (true, START + LENGTH - 1, 1, addr + LENGTH - 1),
            (true, START + LENGTH, 0, addr + LENGTH),
            (false, START + LENGTH, 1, 0),
            (false, START, LENGTH + 1, 0),
            (false, 0, 0, 0),
            (false, 0, 1, 0),
            (false, START - 1, 0, 0),
            (false, START - 1, 1, 0),
            (true, START + LENGTH / 2, LENGTH / 2, addr + LENGTH / 2),
        ];
        for (ok, start, length, value) in cases {
            match ok {
                true => assert_eq!(translate!(start, length, &regions).unwrap(), value),
                false => assert!(translate!(start, length, &regions).is_err()),
            }
        }
    }

    #[test]
    fn test_translate_type() {
        // Pubkey
        let pubkey = Pubkey::new_rand();
        let addr = &pubkey as *const _ as u64;
        let regions = vec![MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: std::mem::size_of::<Pubkey>() as u64,
        }];
        let translated_pubkey = translate_type!(Pubkey, 100, &regions).unwrap();
        assert_eq!(pubkey, *translated_pubkey);

        // Instruction
        let instruction = Instruction::new(
            Pubkey::new_rand(),
            &"foobar",
            vec![AccountMeta::new(Pubkey::new_rand(), false)],
        );
        let addr = &instruction as *const _ as u64;
        let regions = vec![MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: std::mem::size_of::<Instruction>() as u64,
        }];
        let translated_instruction = translate_type!(Instruction, 100, &regions).unwrap();
        assert_eq!(instruction, *translated_instruction);
    }

    #[test]
    fn test_translate_slice() {
        // u8
        let mut data = vec![1u8, 2, 3, 4, 5];
        let addr = data.as_ptr() as *const _ as u64;
        let regions = vec![MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: data.len() as u64,
        }];
        let translated_data = translate_slice!(u8, 100, data.len(), &regions).unwrap();
        assert_eq!(data, translated_data);
        data[0] = 10;
        assert_eq!(data, translated_data);

        // Pubkeys
        let mut data = vec![Pubkey::new_rand(); 5];
        let addr = data.as_ptr() as *const _ as u64;
        let regions = vec![MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: (data.len() * std::mem::size_of::<Pubkey>()) as u64,
        }];
        let translated_data = translate_slice!(Pubkey, 100, data.len(), &regions).unwrap();
        assert_eq!(data, translated_data);
        data[0] = Pubkey::new_rand(); // Both should point to same place
        assert_eq!(data, translated_data);
    }

    #[test]
    fn test_translate_string_and_do() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let regions = vec![MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: string.len() as u64,
        }];
        assert_eq!(
            42,
            translate_string_and_do(100, string.len() as u64, &regions, &|string: &str| {
                assert_eq!(string, "Gaggablaghblagh!");
                Ok(42)
            })
            .unwrap()
        );
    }

    #[test]
    #[should_panic(expected = "UserError(HelperError(Abort))")]
    fn test_helper_abort() {
        let ro_region = MemoryRegion::default();
        let rw_region = MemoryRegion::default();
        helper_abort(0, 0, 0, 0, 0, &[ro_region], &[rw_region]).unwrap();
    }

    #[test]
    #[should_panic(expected = "UserError(HelperError(Panic(\"Gaggablaghblagh!\", 42, 84)))")]
    fn test_helper_sol_panic() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let ro_region = MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: string.len() as u64,
        };
        let rw_region = MemoryRegion::default();
        helper_sol_panic(
            100,
            string.len() as u64,
            42,
            84,
            0,
            &[ro_region],
            &[rw_region],
        )
        .unwrap();
    }

    // Ignore this test: solana_logger conflicts when running tests concurrently,
    // this results in the bad string length being ignored and not returning an error
    #[test]
    #[ignore]
    fn test_helper_sol_log() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let ro_regions = &[MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: string.len() as u64,
        }];
        let rw_regions = &[MemoryRegion::default()];
        solana_logger::setup_with_default("solana=info");
        helper_sol_log(100, string.len() as u64, 0, 0, 0, ro_regions, rw_regions).unwrap();
        solana_logger::setup_with_default("solana=info");
        helper_sol_log(
            100,
            string.len() as u64 * 2,
            0,
            0,
            0,
            ro_regions,
            rw_regions,
        )
        .unwrap_err();
    }

    // Ignore this test: solana_logger conflicts when running tests concurrently,
    // this results in the bad string length being ignored and not returning an error
    #[test]
    #[ignore]
    fn test_helper_sol_log_u64() {
        solana_logger::setup_with_default("solana=info");

        let ro_regions = &[MemoryRegion::default()];
        let rw_regions = &[MemoryRegion::default()];
        helper_sol_log_u64(1, 2, 3, 4, 5, ro_regions, rw_regions).unwrap();
    }

    #[test]
    fn test_helper_sol_alloc_free() {
        // large alloc
        {
            let heap = vec![0_u8; 100];
            let ro_regions = &[MemoryRegion::default()];
            let rw_regions = &[MemoryRegion::new_from_slice(&heap, MM_HEAP_START)];
            let mut helper = HelperSolAllocFree {
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            assert_ne!(
                helper
                    .call(100, 0, 0, 0, 0, ro_regions, rw_regions)
                    .unwrap(),
                0
            );
            assert_eq!(
                helper
                    .call(100, 0, 0, 0, 0, ro_regions, rw_regions)
                    .unwrap(),
                0
            );
        }
        // many small allocs
        {
            let heap = vec![0_u8; 100];
            let ro_regions = &[MemoryRegion::default()];
            let rw_regions = &[MemoryRegion::new_from_slice(&heap, MM_HEAP_START)];
            let mut helper = HelperSolAllocFree {
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            for _ in 0..100 {
                assert_ne!(
                    helper.call(1, 0, 0, 0, 0, ro_regions, rw_regions).unwrap(),
                    0
                );
            }
            assert_eq!(
                helper
                    .call(100, 0, 0, 0, 0, ro_regions, rw_regions)
                    .unwrap(),
                0
            );
        }
    }
}
