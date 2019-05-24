pub mod bpf_verifier;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use libc::c_char;
use log::*;
use solana_rbpf::{EbpfVmRaw, MemoryRegion};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use std::alloc::{self, Layout};
use std::any::Any;
use std::ffi::CStr;
use std::fmt;
use std::io::prelude::*;
use std::io::{Error, ErrorKind};
use std::mem;

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

pub fn helper_sol_panic_verify(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _context: &mut Option<Box<Any + 'static>>,
    _ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<(()), Error> {
    Err(Error::new(ErrorKind::Other, "Error: BPF program Panic!"))
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
        Ok(slice) => info!("sol_log: {:?}", slice),
        Err(e) => warn!("Error: Cannot print invalid string: {}", e),
    };
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
        "sol_log_u64: {:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
        arg1, arg2, arg3, arg4, arg5
    );
    0
}

/// Loosely on the unstable std::alloc::Alloc trait
trait Alloc {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr>;
    fn dealloc(&mut self, ptr: *mut u8, layout: Layout);
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocErr;

impl fmt::Display for AllocErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("memory allocation failed")
    }
}

#[derive(Debug)]
struct BPFAllocator {
    heap: Vec<u8>,
    pos: usize,
}

impl Alloc for BPFAllocator {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        if self.pos + layout.size() < self.heap.len() {
            let ptr = unsafe { self.heap.as_mut_ptr().offset(self.pos as isize) };
            self.pos += layout.size();
            Ok(ptr)
        } else {
            println!("Error: Alloc {:?} bytes failed", layout.size());
            Err(AllocErr)
        }
    }

    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {
        // Using bump allocator, free not supported
    }
}

struct AllocFreeContext {
    allocator: Alloc
}

pub fn helper_sol_alloc_free(
    size: u64,
    free_ptr: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    context: &mut Option<Box<Any + 'static>>,
) -> u64 {
    match context {
        Some(context) => {
            match context.downcast_mut::<BPFAllocator>() {
                Some(info) => {
                    if free_ptr != 0 {
                        println!("Free {:?} bytes at {:?}", size, free_ptr);
                        let layout =
                        Layout::from_size_align(size as usize, mem::align_of::<u8>()).expect("Error: Bad layout");
                        info.dealloc(free_ptr as *mut u8, layout);
                        0

                    // unsafe {
                    //     alloc::dealloc(free_ptr as *mut u8, layout);
                    // }
                    // 0
                    } else {
                            println!("Alloc {} bytes", size);
                            let layout =
                                Layout::from_size_align(size as usize, mem::align_of::<u8>()).expect("Error: Bad layout");
                            match info.alloc(layout) {
                                Ok(ptr) => ptr as u64,
                                Err(_) => 0,
                            }
                            
                            // alloc::alloc(layout) as u64
                    }
                }
                None => {
                    println!("Error: Not able to downcast: {:?}", context);
                    panic!();
                }
            }
        }
        None => {
            println!("Error: No helper context");
            panic!();
        }
    }
}

pub fn create_vm(prog: &[u8]) -> Result<(EbpfVmRaw, MemoryRegion), Error> {
    let mut vm = EbpfVmRaw::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_max_instruction_count(36000)?;
    vm.set_elf(&prog)?;
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
        Some(helper_sol_log_verify),
        helper_sol_log,
        None,
    )?;
    vm.register_helper_ex("sol_log_64", None, helper_sol_log_u64, None)?;
    vm.register_helper_ex("sol_log_64_", None, helper_sol_log_u64, None)?;

    let heap = vec![0_u8; 2048];
    let heap_region = MemoryRegion::new_from_slice(&heap);

    let context = Box::new(BPFAllocator { heap, pos: 0 });

    // TODO alloc helper not the right place to get memory
    vm.register_helper_ex(
        "sol_alloc_free_",
        None,
        helper_sol_alloc_free,
        Some(context),
    )?;

    Ok((vm, heap_region))
}

fn serialize_parameters(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Vec<u8> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for info in keyed_accounts.iter_mut() {
        v.write_u64::<LittleEndian>(info.signer_key().is_some() as u64)
            .unwrap();
        v.write_all(info.unsigned_key().as_ref()).unwrap();
        v.write_u64::<LittleEndian>(info.account.lamports).unwrap();
        v.write_u64::<LittleEndian>(info.account.data.len() as u64)
            .unwrap();
        v.write_all(&info.account.data).unwrap();
        v.write_all(info.account.owner.as_ref()).unwrap();
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v.write_u64::<LittleEndian>(tick_height).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    v
}

fn deserialize_parameters(keyed_accounts: &mut [KeyedAccount], buffer: &[u8]) {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut start = mem::size_of::<u64>();
    for info in keyed_accounts.iter_mut() {
        start += mem::size_of::<u64>(); // skip signer_key boolean
        start += mem::size_of::<Pubkey>(); // skip pubkey
        info.account.lamports = LittleEndian::read_u64(&buffer[start..]);

        start += mem::size_of::<u64>() // skip lamports
                  + mem::size_of::<u64>(); // skip length tag
        let end = start + info.account.data.len();
        info.account.data.clone_from_slice(&buffer[start..end]);

        start += info.account.data.len() // skip data
                  + mem::size_of::<Pubkey>(); // skip owner
    }
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    tx_data: &[u8],
    tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    if keyed_accounts[0].account.executable {
        let (progs, params) = keyed_accounts.split_at_mut(1);
        let prog = &progs[0].account.data;
        info!("Call BPF program");
        let (mut vm, heap_region) = match create_vm(prog) {
            Ok(info) => info,
            Err(e) => {
                warn!("Failed to create BPF VM: {}", e);
                return Err(InstructionError::GenericError);
            }
        };
        let mut v = serialize_parameters(program_id, params, &tx_data, tick_height);

        println!("Heap region: {:?}", heap_region);

        match vm.execute_program(v.as_mut_slice(), &[], &[heap_region]) {
            Ok(status) => {
                if 0 == status {
                    warn!("BPF program failed: {}", status);
                    return Err(InstructionError::GenericError);
                }
            }
            Err(e) => {
                warn!("BPF VM failed to run program: {}", e);
                return Err(InstructionError::GenericError);
            }
        }
        deserialize_parameters(params, &v);
        info!(
            "BPF program executed {} instructions",
            vm.get_last_instruction_count()
        );
    } else if let Ok(instruction) = bincode::deserialize(tx_data) {
        if keyed_accounts[0].signer_key().is_none() {
            warn!("key[0] did not sign the transaction");
            return Err(InstructionError::GenericError);
        }
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                let offset = offset as usize;
                let len = bytes.len();
                debug!("Write: offset={} length={}", offset, len);
                if keyed_accounts[0].account.data.len() < offset + len {
                    warn!(
                        "Write overflow: {} < {}",
                        keyed_accounts[0].account.data.len(),
                        offset + len
                    );
                    return Err(InstructionError::GenericError);
                }
                keyed_accounts[0].account.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                keyed_accounts[0].account.executable = true;
                info!(
                    "Finalize: account {:?}",
                    keyed_accounts[0].signer_key().unwrap()
                );
            }
        }
    } else {
        warn!("Invalid program transaction: {:?}", tx_data);
        return Err(InstructionError::GenericError);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "Error: Execution exceeded maximum number of instructions")]
    fn test_non_terminating_program() {
        #[rustfmt::skip]
        let prog = &[
            0x07, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // r6 + 1
            0x05, 0x00, 0xfe, 0xff, 0x00, 0x00, 0x00, 0x00, // goto -2
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // exit
        ];
        let input = &mut [0x00];

        let mut vm = EbpfVmRaw::new(None).unwrap();
        vm.set_verifier(bpf_verifier::check).unwrap();
        vm.set_max_instruction_count(10).unwrap();
        vm.set_program(prog).unwrap();
        vm.execute_program(input).unwrap();
    }
}
