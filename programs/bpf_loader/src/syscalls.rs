use crate::{alloc, BPFError};
use alloc::Alloc;
use curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar};
use solana_rbpf::{
    ebpf::MM_HEAP_START,
    error::EbpfError,
    memory_region::{AccessType, MemoryMapping},
    question_mark,
    vm::{EbpfVm, SyscallObject, SyscallRegistry},
};
use solana_runtime::message_processor::MessageProcessor;
use solana_sdk::{
    account::Account,
    account_info::AccountInfo,
    account_utils::StateMut,
    bpf_loader, bpf_loader_deprecated,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    entrypoint::{MAX_PERMITTED_DATA_INCREASE, SUCCESS},
    feature_set::{
        abort_on_all_cpi_failures, limit_cpi_loader_invoke, pubkey_log_syscall_enabled,
        ristretto_mul_syscall_enabled, sha256_syscall_enabled, sol_log_compute_units_syscall,
        try_find_program_address_syscall_enabled, use_loaded_executables,
        use_loaded_program_accounts,
    },
    hash::{Hasher, HASH_BYTES},
    ic_msg,
    instruction::{AccountMeta, Instruction, InstructionError},
    keyed_account::KeyedAccount,
    native_loader,
    process_instruction::{stable_log, ComputeMeter, InvokeContext, Logger},
    program_error::ProgramError,
    pubkey::{Pubkey, PubkeyError, MAX_SEEDS},
};
use std::{
    alloc::Layout,
    cell::{Ref, RefCell, RefMut},
    convert::TryFrom,
    mem::{align_of, size_of},
    rc::Rc,
    slice::from_raw_parts_mut,
    str::{from_utf8, Utf8Error},
};
use thiserror::Error as ThisError;

/// Maximum signers
pub const MAX_SIGNERS: usize = 16;

/// Error definitions
#[derive(Debug, ThisError, PartialEq)]
pub enum SyscallError {
    #[error("{0}: {1:?}")]
    InvalidString(Utf8Error, Vec<u8>),
    #[error("BPF program panicked")]
    Abort,
    #[error("BPF program Panicked in {0} at {1}:{2}")]
    Panic(String, u64, u64),
    #[error("cannot borrow invoke context")]
    InvokeContextBorrowFailed,
    #[error("malformed signer seed: {0}: {1:?}")]
    MalformedSignerSeed(Utf8Error, Vec<u8>),
    #[error("Could not create program address with signer seeds: {0}")]
    BadSeeds(PubkeyError),
    #[error("Program {0} not supported by inner instructions")]
    ProgramNotSupported(Pubkey),
    #[error("{0}")]
    InstructionError(InstructionError),
    #[error("Unaligned pointer")]
    UnalignedPointer,
    #[error("Too many signers")]
    TooManySigners,
    #[error("Instruction passed to inner instruction is too large ({0} > {1})")]
    InstructionTooLarge(usize, usize),
    #[error("Too many accounts passed to inner instruction")]
    TooManyAccounts,
}
impl From<SyscallError> for EbpfError<BPFError> {
    fn from(error: SyscallError) -> Self {
        EbpfError::UserError(error.into())
    }
}

trait SyscallConsume {
    fn consume(&mut self, amount: u64) -> Result<(), EbpfError<BPFError>>;
}
impl SyscallConsume for Rc<RefCell<dyn ComputeMeter>> {
    fn consume(&mut self, amount: u64) -> Result<(), EbpfError<BPFError>> {
        self.try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed)?
            .consume(amount)
            .map_err(SyscallError::InstructionError)?;
        Ok(())
    }
}

/// Program heap allocators are intended to allocate/free from a given
/// chunk of memory.  The specific allocator implementation is
/// selectable at build-time.
/// Only one allocator is currently supported

/// Simple bump allocator, never frees
use crate::allocator_bump::BPFAllocator;

pub fn register_syscalls(
    invoke_context: &mut dyn InvokeContext,
) -> Result<SyscallRegistry, EbpfError<BPFError>> {
    let mut syscall_registry = SyscallRegistry::default();

    syscall_registry.register_syscall_by_name(b"abort", SyscallAbort::call)?;
    syscall_registry.register_syscall_by_name(b"sol_panic_", SyscallPanic::call)?;
    syscall_registry.register_syscall_by_name(b"sol_log_", SyscallLog::call)?;
    syscall_registry.register_syscall_by_name(b"sol_log_64_", SyscallLogU64::call)?;

    if invoke_context.is_feature_active(&sol_log_compute_units_syscall::id()) {
        syscall_registry
            .register_syscall_by_name(b"sol_log_compute_units_", SyscallLogBpfComputeUnits::call)?;
    }

    if invoke_context.is_feature_active(&pubkey_log_syscall_enabled::id()) {
        syscall_registry.register_syscall_by_name(b"sol_log_pubkey", SyscallLogPubkey::call)?;
    }

    if invoke_context.is_feature_active(&sha256_syscall_enabled::id()) {
        syscall_registry.register_syscall_by_name(b"sol_sha256", SyscallSha256::call)?;
    }

    if invoke_context.is_feature_active(&ristretto_mul_syscall_enabled::id()) {
        syscall_registry
            .register_syscall_by_name(b"sol_ristretto_mul", SyscallRistrettoMul::call)?;
    }

    syscall_registry.register_syscall_by_name(
        b"sol_create_program_address",
        SyscallCreateProgramAddress::call,
    )?;
    if invoke_context.is_feature_active(&try_find_program_address_syscall_enabled::id()) {
        syscall_registry.register_syscall_by_name(
            b"sol_try_find_program_address",
            SyscallTryFindProgramAddress::call,
        )?;
    }
    syscall_registry
        .register_syscall_by_name(b"sol_invoke_signed_c", SyscallInvokeSignedC::call)?;
    syscall_registry
        .register_syscall_by_name(b"sol_invoke_signed_rust", SyscallInvokeSignedRust::call)?;
    syscall_registry.register_syscall_by_name(b"sol_alloc_free_", SyscallAllocFree::call)?;

    Ok(syscall_registry)
}

macro_rules! bind_feature_gated_syscall_context_object {
    ($vm:expr, $invoke_context:expr, $feature_id:expr, $syscall_context_object:expr $(,)?) => {
        if $invoke_context.is_feature_active($feature_id) {
            match $vm.bind_syscall_context_object($syscall_context_object, None) {
                Err(EbpfError::SyscallNotRegistered(_)) | Ok(()) => {}
                Err(err) => {
                    return Err(err);
                }
            }
        }
    };
}

pub fn bind_syscall_context_objects<'a>(
    loader_id: &'a Pubkey,
    vm: &mut EbpfVm<'a, BPFError, crate::ThisInstructionMeter>,
    callers_keyed_accounts: &'a [KeyedAccount<'a>],
    invoke_context: &'a mut dyn InvokeContext,
    heap: Vec<u8>,
) -> Result<(), EbpfError<BPFError>> {
    let bpf_compute_budget = invoke_context.get_bpf_compute_budget();

    // Syscall functions common across languages

    vm.bind_syscall_context_object(Box::new(SyscallAbort {}), None)?;
    vm.bind_syscall_context_object(Box::new(SyscallPanic { loader_id }), None)?;
    vm.bind_syscall_context_object(
        Box::new(SyscallLog {
            cost: bpf_compute_budget.log_units,
            compute_meter: invoke_context.get_compute_meter(),
            logger: invoke_context.get_logger(),
            loader_id,
        }),
        None,
    )?;
    vm.bind_syscall_context_object(
        Box::new(SyscallLogU64 {
            cost: bpf_compute_budget.log_64_units,
            compute_meter: invoke_context.get_compute_meter(),
            logger: invoke_context.get_logger(),
        }),
        None,
    )?;

    bind_feature_gated_syscall_context_object!(
        vm,
        invoke_context,
        &sol_log_compute_units_syscall::id(),
        Box::new(SyscallLogBpfComputeUnits {
            cost: 0,
            compute_meter: invoke_context.get_compute_meter(),
            logger: invoke_context.get_logger(),
        }),
    );

    bind_feature_gated_syscall_context_object!(
        vm,
        invoke_context,
        &pubkey_log_syscall_enabled::id(),
        Box::new(SyscallLogPubkey {
            cost: bpf_compute_budget.log_pubkey_units,
            compute_meter: invoke_context.get_compute_meter(),
            logger: invoke_context.get_logger(),
            loader_id,
        }),
    );

    bind_feature_gated_syscall_context_object!(
        vm,
        invoke_context,
        &sha256_syscall_enabled::id(),
        Box::new(SyscallSha256 {
            sha256_base_cost: bpf_compute_budget.sha256_base_cost,
            sha256_byte_cost: bpf_compute_budget.sha256_byte_cost,
            compute_meter: invoke_context.get_compute_meter(),
            loader_id,
        }),
    );

    bind_feature_gated_syscall_context_object!(
        vm,
        invoke_context,
        &ristretto_mul_syscall_enabled::id(),
        Box::new(SyscallRistrettoMul {
            cost: 0,
            compute_meter: invoke_context.get_compute_meter(),
            loader_id,
        }),
    );

    vm.bind_syscall_context_object(
        Box::new(SyscallCreateProgramAddress {
            cost: bpf_compute_budget.create_program_address_units,
            compute_meter: invoke_context.get_compute_meter(),
            loader_id,
        }),
        None,
    )?;

    bind_feature_gated_syscall_context_object!(
        vm,
        invoke_context,
        &try_find_program_address_syscall_enabled::id(),
        Box::new(SyscallTryFindProgramAddress {
            cost: bpf_compute_budget.create_program_address_units,
            compute_meter: invoke_context.get_compute_meter(),
            loader_id,
        }),
    );

    // Cross-program invocation syscalls

    let invoke_context = Rc::new(RefCell::new(invoke_context));
    vm.bind_syscall_context_object(
        Box::new(SyscallInvokeSignedC {
            callers_keyed_accounts,
            invoke_context: invoke_context.clone(),
            loader_id,
        }),
        None,
    )?;
    vm.bind_syscall_context_object(
        Box::new(SyscallInvokeSignedRust {
            callers_keyed_accounts,
            invoke_context: invoke_context.clone(),
            loader_id,
        }),
        None,
    )?;

    // Memory allocator

    vm.bind_syscall_context_object(
        Box::new(SyscallAllocFree {
            aligned: *loader_id != bpf_loader_deprecated::id(),
            allocator: BPFAllocator::new(heap, MM_HEAP_START),
        }),
        None,
    )?;

    Ok(())
}

fn translate(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    len: u64,
) -> Result<u64, EbpfError<BPFError>> {
    memory_mapping.map::<BPFError>(access_type, vm_addr, len)
}

fn translate_type_inner<'a, T>(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    loader_id: &Pubkey,
) -> Result<&'a mut T, EbpfError<BPFError>> {
    if loader_id != &bpf_loader_deprecated::id()
        && (vm_addr as u64 as *mut T).align_offset(align_of::<T>()) != 0
    {
        Err(SyscallError::UnalignedPointer.into())
    } else {
        unsafe {
            match translate(memory_mapping, access_type, vm_addr, size_of::<T>() as u64) {
                Ok(value) => Ok(&mut *(value as *mut T)),
                Err(e) => Err(e),
            }
        }
    }
}
fn translate_type_mut<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    loader_id: &Pubkey,
) -> Result<&'a mut T, EbpfError<BPFError>> {
    translate_type_inner::<T>(memory_mapping, AccessType::Store, vm_addr, loader_id)
}
fn translate_type<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    loader_id: &Pubkey,
) -> Result<&'a T, EbpfError<BPFError>> {
    match translate_type_inner::<T>(memory_mapping, AccessType::Load, vm_addr, loader_id) {
        Ok(value) => Ok(&*value),
        Err(e) => Err(e),
    }
}

fn translate_slice_inner<'a, T>(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    len: u64,
    loader_id: &Pubkey,
) -> Result<&'a mut [T], EbpfError<BPFError>> {
    if loader_id != &bpf_loader_deprecated::id()
        && (vm_addr as u64 as *mut T).align_offset(align_of::<T>()) != 0
    {
        Err(SyscallError::UnalignedPointer.into())
    } else if len == 0 {
        Ok(unsafe { from_raw_parts_mut(0x1 as *mut T, len as usize) })
    } else {
        match translate(
            memory_mapping,
            access_type,
            vm_addr,
            len.saturating_mul(size_of::<T>() as u64),
        ) {
            Ok(value) => Ok(unsafe { from_raw_parts_mut(value as *mut T, len as usize) }),
            Err(e) => Err(e),
        }
    }
}
fn translate_slice_mut<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    len: u64,
    loader_id: &Pubkey,
) -> Result<&'a mut [T], EbpfError<BPFError>> {
    translate_slice_inner::<T>(memory_mapping, AccessType::Store, vm_addr, len, loader_id)
}
fn translate_slice<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    len: u64,
    loader_id: &Pubkey,
) -> Result<&'a [T], EbpfError<BPFError>> {
    match translate_slice_inner::<T>(memory_mapping, AccessType::Load, vm_addr, len, loader_id) {
        Ok(value) => Ok(&*value),
        Err(e) => Err(e),
    }
}

/// Take a virtual pointer to a string (points to BPF VM memory space), translate it
/// pass it to a user-defined work function
fn translate_string_and_do(
    memory_mapping: &MemoryMapping,
    addr: u64,
    len: u64,
    loader_id: &Pubkey,
    work: &mut dyn FnMut(&str) -> Result<u64, EbpfError<BPFError>>,
) -> Result<u64, EbpfError<BPFError>> {
    let buf = translate_slice::<u8>(memory_mapping, addr, len, loader_id)?;
    let i = match buf.iter().position(|byte| *byte == 0) {
        Some(i) => i,
        None => len as usize,
    };
    match from_utf8(&buf[..i]) {
        Ok(message) => work(message),
        Err(err) => Err(SyscallError::InvalidString(err, buf[..i].to_vec()).into()),
    }
}

/// Abort syscall functions, called when the BPF program calls `abort()`
/// LLVM will insert calls to `abort()` if it detects an untenable situation,
/// `abort()` is not intended to be called explicitly by the program.
/// Causes the BPF program to be halted immediately
pub struct SyscallAbort {}
impl SyscallObject<BPFError> for SyscallAbort {
    fn call(
        &mut self,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        *result = Err(SyscallError::Abort.into());
    }
}

/// Panic syscall function, called when the BPF program calls 'sol_panic_()`
/// Causes the BPF program to be halted immediately
/// Log a user's info message
pub struct SyscallPanic<'a> {
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallPanic<'a> {
    fn call(
        &mut self,
        file: u64,
        len: u64,
        line: u64,
        column: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        *result = translate_string_and_do(
            memory_mapping,
            file,
            len,
            &self.loader_id,
            &mut |string: &str| Err(SyscallError::Panic(string.to_string(), line, column).into()),
        );
    }
}

/// Log a user's info message
pub struct SyscallLog<'a> {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    logger: Rc<RefCell<dyn Logger>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallLog<'a> {
    fn call(
        &mut self,
        addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.cost), result);
        question_mark!(
            translate_string_and_do(
                memory_mapping,
                addr,
                len,
                &self.loader_id,
                &mut |string: &str| {
                    stable_log::program_log(&self.logger, string);
                    Ok(0)
                },
            ),
            result
        );
        *result = Ok(0);
    }
}

/// Log 5 64-bit values
pub struct SyscallLogU64 {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    logger: Rc<RefCell<dyn Logger>>,
}
impl SyscallObject<BPFError> for SyscallLogU64 {
    fn call(
        &mut self,
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        _memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.cost), result);
        stable_log::program_log(
            &self.logger,
            &format!(
                "{:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
                arg1, arg2, arg3, arg4, arg5
            ),
        );
        *result = Ok(0);
    }
}

/// Log current compute consumption
pub struct SyscallLogBpfComputeUnits {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    logger: Rc<RefCell<dyn Logger>>,
}
impl SyscallObject<BPFError> for SyscallLogBpfComputeUnits {
    fn call(
        &mut self,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.cost), result);
        let logger = question_mark!(
            self.logger
                .try_borrow_mut()
                .map_err(|_| SyscallError::InvokeContextBorrowFailed),
            result
        );
        if logger.log_enabled() {
            logger.log(&format!(
                "Program consumption: {} units remaining",
                self.compute_meter.borrow().get_remaining()
            ));
        }
        *result = Ok(0);
    }
}

/// Log 5 64-bit values
pub struct SyscallLogPubkey<'a> {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    logger: Rc<RefCell<dyn Logger>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallLogPubkey<'a> {
    fn call(
        &mut self,
        pubkey_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.cost), result);
        let pubkey = question_mark!(
            translate_type::<Pubkey>(memory_mapping, pubkey_addr, self.loader_id),
            result
        );
        stable_log::program_log(&self.logger, &pubkey.to_string());
        *result = Ok(0);
    }
}

/// Dynamic memory allocation syscall called when the BPF program calls
/// `sol_alloc_free_()`.  The allocator is expected to allocate/free
/// from/to a given chunk of memory and enforce size restrictions.  The
/// memory chunk is given to the allocator during allocator creation and
/// information about that memory (start address and size) is passed
/// to the VM to use for enforcement.
pub struct SyscallAllocFree {
    aligned: bool,
    allocator: BPFAllocator,
}
impl SyscallObject<BPFError> for SyscallAllocFree {
    fn call(
        &mut self,
        size: u64,
        free_addr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        let align = if self.aligned {
            align_of::<u128>()
        } else {
            align_of::<u8>()
        };
        let layout = match Layout::from_size_align(size as usize, align) {
            Ok(layout) => layout,
            Err(_) => {
                *result = Ok(0);
                return;
            }
        };
        *result = if free_addr == 0 {
            match self.allocator.alloc(layout) {
                Ok(addr) => Ok(addr as u64),
                Err(_) => Ok(0),
            }
        } else {
            self.allocator.dealloc(free_addr, layout);
            Ok(0)
        };
    }
}

fn translate_program_address_inputs<'a>(
    seeds_addr: u64,
    seeds_len: u64,
    program_id_addr: u64,
    memory_mapping: &MemoryMapping,
    loader_id: &Pubkey,
) -> Result<(Vec<&'a [u8]>, &'a Pubkey), EbpfError<BPFError>> {
    let untranslated_seeds =
        translate_slice::<&[&u8]>(memory_mapping, seeds_addr, seeds_len, loader_id)?;
    if untranslated_seeds.len() > MAX_SEEDS {
        return Err(SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded).into());
    }
    let seeds = untranslated_seeds
        .iter()
        .map(|untranslated_seed| {
            translate_slice::<u8>(
                memory_mapping,
                untranslated_seed.as_ptr() as *const _ as u64,
                untranslated_seed.len() as u64,
                loader_id,
            )
        })
        .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;
    let program_id = translate_type::<Pubkey>(memory_mapping, program_id_addr, loader_id)?;
    Ok((seeds, program_id))
}

/// Create a program address
struct SyscallCreateProgramAddress<'a> {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallCreateProgramAddress<'a> {
    fn call(
        &mut self,
        seeds_addr: u64,
        seeds_len: u64,
        program_id_addr: u64,
        address_addr: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        let (seeds, program_id) = question_mark!(
            translate_program_address_inputs(
                seeds_addr,
                seeds_len,
                program_id_addr,
                memory_mapping,
                self.loader_id,
            ),
            result
        );

        question_mark!(self.compute_meter.consume(self.cost), result);
        let new_address = match Pubkey::create_program_address(&seeds, program_id) {
            Ok(address) => address,
            Err(_) => {
                *result = Ok(1);
                return;
            }
        };
        let address = question_mark!(
            translate_slice_mut::<u8>(memory_mapping, address_addr, 32, self.loader_id),
            result
        );
        address.copy_from_slice(new_address.as_ref());
        *result = Ok(0);
    }
}

/// Create a program address
struct SyscallTryFindProgramAddress<'a> {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallTryFindProgramAddress<'a> {
    fn call(
        &mut self,
        seeds_addr: u64,
        seeds_len: u64,
        program_id_addr: u64,
        address_addr: u64,
        bump_seed_addr: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        let (seeds, program_id) = question_mark!(
            translate_program_address_inputs(
                seeds_addr,
                seeds_len,
                program_id_addr,
                memory_mapping,
                self.loader_id,
            ),
            result
        );

        let mut bump_seed = [std::u8::MAX];
        for _ in 0..std::u8::MAX {
            {
                let mut seeds_with_bump = seeds.to_vec();
                seeds_with_bump.push(&bump_seed);

                question_mark!(self.compute_meter.consume(self.cost), result);
                if let Ok(new_address) =
                    Pubkey::create_program_address(&seeds_with_bump, program_id)
                {
                    let bump_seed_ref = question_mark!(
                        translate_type_mut::<u8>(memory_mapping, bump_seed_addr, self.loader_id),
                        result
                    );
                    let address = question_mark!(
                        translate_slice_mut::<u8>(memory_mapping, address_addr, 32, self.loader_id),
                        result
                    );
                    *bump_seed_ref = bump_seed[0];
                    address.copy_from_slice(new_address.as_ref());
                    *result = Ok(0);
                    return;
                }
            }
            bump_seed[0] -= 1;
        }
        *result = Ok(1);
    }
}

/// SHA256
pub struct SyscallSha256<'a> {
    sha256_base_cost: u64,
    sha256_byte_cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallSha256<'a> {
    fn call(
        &mut self,
        vals_addr: u64,
        vals_len: u64,
        result_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.sha256_base_cost), result);
        let hash_result = question_mark!(
            translate_slice_mut::<u8>(
                memory_mapping,
                result_addr,
                HASH_BYTES as u64,
                self.loader_id
            ),
            result
        );
        let mut hasher = Hasher::default();
        if vals_len > 0 {
            let vals = question_mark!(
                translate_slice::<&[u8]>(memory_mapping, vals_addr, vals_len, self.loader_id),
                result
            );
            for val in vals.iter() {
                let bytes = question_mark!(
                    translate_slice::<u8>(
                        memory_mapping,
                        val.as_ptr() as u64,
                        val.len() as u64,
                        self.loader_id
                    ),
                    result
                );
                question_mark!(
                    self.compute_meter
                        .consume(self.sha256_byte_cost * (val.len() as u64 / 2)),
                    result
                );
                hasher.hash(bytes);
            }
        }
        hash_result.copy_from_slice(&hasher.result().to_bytes());
        *result = Ok(0);
    }
}

/// Ristretto point multiply
pub struct SyscallRistrettoMul<'a> {
    cost: u64,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallObject<BPFError> for SyscallRistrettoMul<'a> {
    fn call(
        &mut self,
        point_addr: u64,
        scalar_addr: u64,
        result_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        question_mark!(self.compute_meter.consume(self.cost), result);

        let point = question_mark!(
            translate_type::<RistrettoPoint>(memory_mapping, point_addr, self.loader_id),
            result
        );
        let scalar = question_mark!(
            translate_type::<Scalar>(memory_mapping, scalar_addr, self.loader_id),
            result
        );
        let output = question_mark!(
            translate_type_mut::<RistrettoPoint>(memory_mapping, result_addr, self.loader_id),
            result
        );
        *output = point * scalar;

        *result = Ok(0);
    }
}

// Cross-program invocation syscalls

struct AccountReferences<'a> {
    lamports: &'a mut u64,
    owner: &'a mut Pubkey,
    data: &'a mut [u8],
    vm_data_addr: u64,
    ref_to_len_in_vm: &'a mut u64,
    serialized_len_ptr: &'a mut u64,
}
type TranslatedAccount<'a> = (Rc<RefCell<Account>>, Option<AccountReferences<'a>>);
type TranslatedAccounts<'a> = (
    Vec<Rc<RefCell<Account>>>,
    Vec<Option<AccountReferences<'a>>>,
);

/// Implemented by language specific data structure translators
trait SyscallInvokeSigned<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>>;
    fn get_context(&self) -> Result<Ref<&'a mut dyn InvokeContext>, EbpfError<BPFError>>;
    fn get_callers_keyed_accounts(&self) -> &'a [KeyedAccount<'a>];
    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Instruction, EbpfError<BPFError>>;
    fn translate_accounts(
        &self,
        account_keys: &[Pubkey],
        program_account_index: usize,
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>>;
    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>>;
}

/// Cross-program invocation called from Rust
pub struct SyscallInvokeSignedRust<'a> {
    callers_keyed_accounts: &'a [KeyedAccount<'a>],
    invoke_context: Rc<RefCell<&'a mut dyn InvokeContext>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallInvokeSigned<'a> for SyscallInvokeSignedRust<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }
    fn get_context(&self) -> Result<Ref<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }
    fn get_callers_keyed_accounts(&self) -> &'a [KeyedAccount<'a>] {
        self.callers_keyed_accounts
    }
    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Instruction, EbpfError<BPFError>> {
        let ix = translate_type::<Instruction>(memory_mapping, addr, self.loader_id)?;

        check_instruction_size(
            ix.accounts.len(),
            ix.data.len(),
            &self.invoke_context.borrow(),
        )?;

        let accounts = translate_slice::<AccountMeta>(
            memory_mapping,
            ix.accounts.as_ptr() as u64,
            ix.accounts.len() as u64,
            self.loader_id,
        )?
        .to_vec();
        let data = translate_slice::<u8>(
            memory_mapping,
            ix.data.as_ptr() as u64,
            ix.data.len() as u64,
            self.loader_id,
        )?
        .to_vec();
        Ok(Instruction {
            program_id: ix.program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts(
        &self,
        account_keys: &[Pubkey],
        program_account_index: usize,
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>> {
        let invoke_context = self.invoke_context.borrow();

        let account_infos = translate_slice::<AccountInfo>(
            memory_mapping,
            account_infos_addr,
            account_infos_len,
            self.loader_id,
        )?;
        check_account_infos(account_infos.len(), &invoke_context)?;
        let account_info_keys = account_infos
            .iter()
            .map(|account_info| {
                translate_type::<Pubkey>(
                    memory_mapping,
                    account_info.key as *const _ as u64,
                    self.loader_id,
                )
            })
            .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;

        let translate = |account_info: &AccountInfo| {
            // Translate the account from user space

            let lamports = {
                // Double translate lamports out of RefCell
                let ptr = translate_type::<u64>(
                    memory_mapping,
                    account_info.lamports.as_ptr() as u64,
                    self.loader_id,
                )?;
                translate_type_mut::<u64>(memory_mapping, *ptr, self.loader_id)?
            };
            let owner = translate_type_mut::<Pubkey>(
                memory_mapping,
                account_info.owner as *const _ as u64,
                self.loader_id,
            )?;
            let (data, vm_data_addr, ref_to_len_in_vm, serialized_len_ptr) = {
                // Double translate data out of RefCell
                let data = *translate_type::<&[u8]>(
                    memory_mapping,
                    account_info.data.as_ptr() as *const _ as u64,
                    self.loader_id,
                )?;
                let translated = translate(
                    memory_mapping,
                    AccessType::Store,
                    unsafe { (account_info.data.as_ptr() as *const u64).offset(1) as u64 },
                    8,
                )? as *mut u64;
                let ref_to_len_in_vm = unsafe { &mut *translated };
                let ref_of_len_in_input_buffer = unsafe { data.as_ptr().offset(-8) };
                let serialized_len_ptr = translate_type_mut::<u64>(
                    memory_mapping,
                    ref_of_len_in_input_buffer as *const _ as u64,
                    self.loader_id,
                )?;
                let vm_data_addr = data.as_ptr() as u64;
                (
                    translate_slice_mut::<u8>(
                        memory_mapping,
                        vm_data_addr,
                        data.len() as u64,
                        self.loader_id,
                    )?,
                    vm_data_addr,
                    ref_to_len_in_vm,
                    serialized_len_ptr,
                )
            };

            Ok((
                Rc::new(RefCell::new(Account {
                    lamports: *lamports,
                    data: data.to_vec(),
                    executable: account_info.executable,
                    owner: *owner,
                    rent_epoch: account_info.rent_epoch,
                })),
                Some(AccountReferences {
                    lamports,
                    owner,
                    data,
                    vm_data_addr,
                    ref_to_len_in_vm,
                    serialized_len_ptr,
                }),
            ))
        };

        get_translated_accounts(
            account_keys,
            program_account_index,
            &account_info_keys,
            account_infos,
            &invoke_context,
            translate,
        )
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>> {
        let mut signers = Vec::new();
        if signers_seeds_len > 0 {
            let signers_seeds = translate_slice::<&[&[u8]]>(
                memory_mapping,
                signers_seeds_addr,
                signers_seeds_len,
                self.loader_id,
            )?;
            if signers_seeds.len() > MAX_SIGNERS {
                return Err(SyscallError::TooManySigners.into());
            }
            for signer_seeds in signers_seeds.iter() {
                let untranslated_seeds = translate_slice::<&[u8]>(
                    memory_mapping,
                    signer_seeds.as_ptr() as *const _ as u64,
                    signer_seeds.len() as u64,
                    self.loader_id,
                )?;
                if untranslated_seeds.len() > MAX_SEEDS {
                    return Err(SyscallError::InstructionError(
                        InstructionError::MaxSeedLengthExceeded,
                    )
                    .into());
                }
                let seeds = untranslated_seeds
                    .iter()
                    .map(|untranslated_seed| {
                        translate_slice::<u8>(
                            memory_mapping,
                            untranslated_seed.as_ptr() as *const _ as u64,
                            untranslated_seed.len() as u64,
                            self.loader_id,
                        )
                    })
                    .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;
                let signer = Pubkey::create_program_address(&seeds, program_id)
                    .map_err(SyscallError::BadSeeds)?;
                signers.push(signer);
            }
            Ok(signers)
        } else {
            Ok(vec![])
        }
    }
}
impl<'a> SyscallObject<BPFError> for SyscallInvokeSignedRust<'a> {
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        *result = call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        );
    }
}

/// Rust representation of C's SolInstruction
#[derive(Debug)]
struct SolInstruction {
    program_id_addr: u64,
    accounts_addr: u64,
    accounts_len: usize,
    data_addr: u64,
    data_len: usize,
}

/// Rust representation of C's SolAccountMeta
#[derive(Debug)]
struct SolAccountMeta {
    pubkey_addr: u64,
    is_writable: bool,
    is_signer: bool,
}

/// Rust representation of C's SolAccountInfo
#[derive(Debug)]
struct SolAccountInfo {
    key_addr: u64,
    lamports_addr: u64,
    data_len: u64,
    data_addr: u64,
    owner_addr: u64,
    rent_epoch: u64,
    is_signer: bool,
    is_writable: bool,
    executable: bool,
}

/// Rust representation of C's SolSignerSeed
#[derive(Debug)]
struct SolSignerSeedC {
    addr: u64,
    len: u64,
}

/// Rust representation of C's SolSignerSeeds
#[derive(Debug)]
struct SolSignerSeedsC {
    addr: u64,
    len: u64,
}

/// Cross-program invocation called from C
pub struct SyscallInvokeSignedC<'a> {
    callers_keyed_accounts: &'a [KeyedAccount<'a>],
    invoke_context: Rc<RefCell<&'a mut dyn InvokeContext>>,
    loader_id: &'a Pubkey,
}
impl<'a> SyscallInvokeSigned<'a> for SyscallInvokeSignedC<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }
    fn get_context(&self) -> Result<Ref<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }

    fn get_callers_keyed_accounts(&self) -> &'a [KeyedAccount<'a>] {
        self.callers_keyed_accounts
    }

    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Instruction, EbpfError<BPFError>> {
        let ix_c = translate_type::<SolInstruction>(memory_mapping, addr, self.loader_id)?;

        check_instruction_size(
            ix_c.accounts_len,
            ix_c.data_len,
            &self.invoke_context.borrow(),
        )?;
        let program_id =
            translate_type::<Pubkey>(memory_mapping, ix_c.program_id_addr, self.loader_id)?;
        let meta_cs = translate_slice::<SolAccountMeta>(
            memory_mapping,
            ix_c.accounts_addr,
            ix_c.accounts_len as u64,
            self.loader_id,
        )?;
        let data = translate_slice::<u8>(
            memory_mapping,
            ix_c.data_addr,
            ix_c.data_len as u64,
            self.loader_id,
        )?
        .to_vec();
        let accounts = meta_cs
            .iter()
            .map(|meta_c| {
                let pubkey =
                    translate_type::<Pubkey>(memory_mapping, meta_c.pubkey_addr, self.loader_id)?;
                Ok(AccountMeta {
                    pubkey: *pubkey,
                    is_signer: meta_c.is_signer,
                    is_writable: meta_c.is_writable,
                })
            })
            .collect::<Result<Vec<AccountMeta>, EbpfError<BPFError>>>()?;

        Ok(Instruction {
            program_id: *program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts(
        &self,
        account_keys: &[Pubkey],
        program_account_index: usize,
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>> {
        let invoke_context = self.invoke_context.borrow();

        let account_infos = translate_slice::<SolAccountInfo>(
            memory_mapping,
            account_infos_addr,
            account_infos_len,
            self.loader_id,
        )?;
        check_account_infos(account_infos.len(), &invoke_context)?;
        let account_info_keys = account_infos
            .iter()
            .map(|account_info| {
                translate_type::<Pubkey>(memory_mapping, account_info.key_addr, self.loader_id)
            })
            .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;

        let translate = |account_info: &SolAccountInfo| {
            // Translate the account from user space

            let lamports = translate_type_mut::<u64>(
                memory_mapping,
                account_info.lamports_addr,
                self.loader_id,
            )?;
            let owner = translate_type_mut::<Pubkey>(
                memory_mapping,
                account_info.owner_addr,
                self.loader_id,
            )?;
            let vm_data_addr = account_info.data_addr;
            let data = translate_slice_mut::<u8>(
                memory_mapping,
                vm_data_addr,
                account_info.data_len,
                self.loader_id,
            )?;

            let first_info_addr = &account_infos[0] as *const _ as u64;
            let addr = &account_info.data_len as *const u64 as u64;
            let vm_addr = account_infos_addr + (addr - first_info_addr);
            let _ = translate(
                memory_mapping,
                AccessType::Store,
                vm_addr,
                size_of::<u64>() as u64,
            )?;
            let ref_to_len_in_vm = unsafe { &mut *(addr as *mut u64) };

            let ref_of_len_in_input_buffer =
                unsafe { (account_info.data_addr as *mut u8).offset(-8) };
            let serialized_len_ptr = translate_type_mut::<u64>(
                memory_mapping,
                ref_of_len_in_input_buffer as *const _ as u64,
                self.loader_id,
            )?;

            Ok((
                Rc::new(RefCell::new(Account {
                    lamports: *lamports,
                    data: data.to_vec(),
                    executable: account_info.executable,
                    owner: *owner,
                    rent_epoch: account_info.rent_epoch,
                })),
                Some(AccountReferences {
                    lamports,
                    owner,
                    data,
                    vm_data_addr,
                    ref_to_len_in_vm,
                    serialized_len_ptr,
                }),
            ))
        };

        get_translated_accounts(
            account_keys,
            program_account_index,
            &account_info_keys,
            account_infos,
            &invoke_context,
            translate,
        )
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &MemoryMapping,
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>> {
        if signers_seeds_len > 0 {
            let signers_seeds = translate_slice::<SolSignerSeedC>(
                memory_mapping,
                signers_seeds_addr,
                signers_seeds_len,
                self.loader_id,
            )?;
            if signers_seeds.len() > MAX_SIGNERS {
                return Err(SyscallError::TooManySigners.into());
            }
            Ok(signers_seeds
                .iter()
                .map(|signer_seeds| {
                    let seeds = translate_slice::<SolSignerSeedC>(
                        memory_mapping,
                        signer_seeds.addr,
                        signer_seeds.len,
                        self.loader_id,
                    )?;
                    if seeds.len() > MAX_SEEDS {
                        return Err(SyscallError::InstructionError(
                            InstructionError::MaxSeedLengthExceeded,
                        )
                        .into());
                    }
                    let seeds_bytes = seeds
                        .iter()
                        .map(|seed| {
                            translate_slice::<u8>(
                                memory_mapping,
                                seed.addr,
                                seed.len,
                                self.loader_id,
                            )
                        })
                        .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;
                    Pubkey::create_program_address(&seeds_bytes, program_id)
                        .map_err(|err| SyscallError::BadSeeds(err).into())
                })
                .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?)
        } else {
            Ok(vec![])
        }
    }
}
impl<'a> SyscallObject<BPFError> for SyscallInvokeSignedC<'a> {
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &MemoryMapping,
        result: &mut Result<u64, EbpfError<BPFError>>,
    ) {
        *result = call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        );
    }
}

fn get_translated_accounts<'a, T, F>(
    account_keys: &[Pubkey],
    program_account_index: usize,
    account_info_keys: &[&Pubkey],
    account_infos: &[T],
    invoke_context: &Ref<&mut dyn InvokeContext>,
    do_translate: F,
) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>>
where
    F: Fn(&T) -> Result<TranslatedAccount<'a>, EbpfError<BPFError>>,
{
    let mut accounts = Vec::with_capacity(account_keys.len());
    let mut refs = Vec::with_capacity(account_keys.len());
    for (i, ref account_key) in account_keys.iter().enumerate() {
        let account = invoke_context.get_account(&account_key).ok_or_else(|| {
            ic_msg!(
                invoke_context,
                "Instruction references an unknown account {}",
                account_key
            );
            SyscallError::InstructionError(InstructionError::MissingAccount)
        })?;

        if (invoke_context.is_feature_active(&use_loaded_program_accounts::id())
            && i == program_account_index)
            || (invoke_context.is_feature_active(&use_loaded_executables::id())
                && account.borrow().executable)
        {
            // Use the known executable
            accounts.push(Rc::new(account));
            refs.push(None);
        } else if let Some(account_info) =
            account_info_keys
                .iter()
                .zip(account_infos)
                .find_map(|(key, account_info)| {
                    if key == account_key {
                        Some(account_info)
                    } else {
                        None
                    }
                })
        {
            let (account, account_ref) = do_translate(account_info)?;
            accounts.push(account);
            refs.push(account_ref);
        } else {
            ic_msg!(
                invoke_context,
                "Instruction references an unknown account {}",
                account_key
            );
            return Err(SyscallError::InstructionError(InstructionError::MissingAccount).into());
        }
    }

    Ok((accounts, refs))
}

fn check_instruction_size(
    num_accounts: usize,
    data_len: usize,
    invoke_context: &Ref<&mut dyn InvokeContext>,
) -> Result<(), EbpfError<BPFError>> {
    let size = num_accounts * size_of::<AccountMeta>() + data_len;
    let max_size = invoke_context
        .get_bpf_compute_budget()
        .max_cpi_instruction_size;
    if size > max_size {
        return Err(SyscallError::InstructionTooLarge(size, max_size).into());
    }
    Ok(())
}

fn check_account_infos(
    len: usize,
    invoke_context: &Ref<&mut dyn InvokeContext>,
) -> Result<(), EbpfError<BPFError>> {
    if len * size_of::<Pubkey>()
        > invoke_context
            .get_bpf_compute_budget()
            .max_cpi_instruction_size
        && invoke_context.is_feature_active(&use_loaded_program_accounts::id())
    {
        // Cap the number of account_infos a caller can pass to approximate
        // maximum that accounts that could be passed in an instruction
        return Err(SyscallError::TooManyAccounts.into());
    };
    Ok(())
}

fn check_authorized_program(
    program_id: &Pubkey,
    instruction_data: &[u8],
) -> Result<(), EbpfError<BPFError>> {
    if native_loader::check_id(program_id)
        || bpf_loader::check_id(program_id)
        || bpf_loader_deprecated::check_id(program_id)
        || (bpf_loader_upgradeable::check_id(program_id)
            && !bpf_loader_upgradeable::is_upgrade_instruction(instruction_data))
    {
        return Err(SyscallError::ProgramNotSupported(*program_id).into());
    }
    Ok(())
}

fn get_upgradeable_executable(
    callee_program_id: &Pubkey,
    program_account: &RefCell<Account>,
    invoke_context: &Ref<&mut dyn InvokeContext>,
) -> Result<Option<(Pubkey, RefCell<Account>)>, EbpfError<BPFError>> {
    if program_account.borrow().owner == bpf_loader_upgradeable::id() {
        match program_account.borrow().state() {
            Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) => {
                if let Some(account) = invoke_context.get_account(&programdata_address) {
                    Ok(Some((programdata_address, account)))
                } else {
                    ic_msg!(
                        invoke_context,
                        "Unknown upgradeable programdata account {}",
                        programdata_address,
                    );
                    Err(SyscallError::InstructionError(InstructionError::MissingAccount).into())
                }
            }
            _ => {
                ic_msg!(
                    invoke_context,
                    "Invalid upgradeable program account {}",
                    callee_program_id,
                );
                Err(SyscallError::InstructionError(InstructionError::InvalidAccountData).into())
            }
        }
    } else {
        Ok(None)
    }
}

/// Call process instruction, common to both Rust and C
fn call<'a>(
    syscall: &mut dyn SyscallInvokeSigned<'a>,
    instruction_addr: u64,
    account_infos_addr: u64,
    account_infos_len: u64,
    signers_seeds_addr: u64,
    signers_seeds_len: u64,
    memory_mapping: &MemoryMapping,
) -> Result<u64, EbpfError<BPFError>> {
    let (
        message,
        executables,
        accounts,
        account_refs,
        caller_privileges,
        abort_on_all_cpi_failures,
    ) = {
        let invoke_context = syscall.get_context()?;

        invoke_context
            .get_compute_meter()
            .consume(invoke_context.get_bpf_compute_budget().invoke_units)?;

        let caller_program_id = invoke_context
            .get_caller()
            .map_err(SyscallError::InstructionError)?;

        // Translate and verify caller's data

        let instruction = syscall.translate_instruction(instruction_addr, &memory_mapping)?;
        let signers = syscall.translate_signers(
            caller_program_id,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        )?;
        let keyed_account_refs = syscall
            .get_callers_keyed_accounts()
            .iter()
            .collect::<Vec<&KeyedAccount>>();
        let (message, callee_program_id, callee_program_id_index) =
            MessageProcessor::create_message(
                &instruction,
                &keyed_account_refs,
                &signers,
                &invoke_context,
            )
            .map_err(SyscallError::InstructionError)?;
        let caller_privileges = message
            .account_keys
            .iter()
            .map(|key| {
                if let Some(keyed_account) = keyed_account_refs
                    .iter()
                    .find(|keyed_account| key == keyed_account.unsigned_key())
                {
                    keyed_account.is_writable()
                } else {
                    false
                }
            })
            .collect::<Vec<bool>>();
        if invoke_context.is_feature_active(&limit_cpi_loader_invoke::id()) {
            check_authorized_program(&callee_program_id, &instruction.data)?;
        }
        let (accounts, account_refs) = syscall.translate_accounts(
            &message.account_keys,
            callee_program_id_index,
            account_infos_addr,
            account_infos_len,
            memory_mapping,
        )?;

        // Construct executables

        let program_account = (**accounts.get(callee_program_id_index).ok_or_else(|| {
            ic_msg!(invoke_context, "Unknown program {}", callee_program_id,);
            SyscallError::InstructionError(InstructionError::MissingAccount)
        })?)
        .clone();
        let programdata_executable =
            get_upgradeable_executable(&callee_program_id, &program_account, &invoke_context)?;
        let mut executables = vec![(callee_program_id, program_account)];
        if let Some(executable) = programdata_executable {
            executables.push(executable);
        }

        // Record the instruction

        invoke_context.record_instruction(&instruction);

        (
            message,
            executables,
            accounts,
            account_refs,
            caller_privileges,
            invoke_context.is_feature_active(&abort_on_all_cpi_failures::id()),
        )
    };

    // Process instruction

    #[allow(clippy::deref_addrof)]
    match MessageProcessor::process_cross_program_instruction(
        &message,
        &executables,
        &accounts,
        &caller_privileges,
        *(&mut *(syscall.get_context_mut()?)),
    ) {
        Ok(()) => (),
        Err(err) => {
            if abort_on_all_cpi_failures {
                return Err(SyscallError::InstructionError(err).into());
            } else {
                match ProgramError::try_from(err) {
                    Ok(err) => return Ok(err.into()),
                    Err(err) => return Err(SyscallError::InstructionError(err).into()),
                }
            }
        }
    }

    // Copy results back to caller
    {
        let invoke_context = syscall.get_context()?;
        for (i, (account, account_ref)) in accounts.iter().zip(account_refs).enumerate() {
            let account = account.borrow();
            if let Some(account_ref) = account_ref {
                if message.is_writable(i) && !account.executable {
                    *account_ref.lamports = account.lamports;
                    *account_ref.owner = account.owner;
                    if account_ref.data.len() != account.data.len() {
                        if !account_ref.data.is_empty() {
                            // Only support for `CreateAccount` at this time.
                            // Need a way to limit total realloc size across multiple CPI calls
                            ic_msg!(
                                invoke_context,
                                "Inner instructions do not support realloc, only SystemProgram::CreateAccount",
                            );
                            return Err(SyscallError::InstructionError(
                                InstructionError::InvalidRealloc,
                            )
                            .into());
                        }
                        if account.data.len() > account_ref.data.len() + MAX_PERMITTED_DATA_INCREASE
                        {
                            ic_msg!(
                                invoke_context,
                                "SystemProgram::CreateAccount data size limited to {} in inner instructions",
                                MAX_PERMITTED_DATA_INCREASE
                            );
                            return Err(SyscallError::InstructionError(
                                InstructionError::InvalidRealloc,
                            )
                            .into());
                        }
                        let _ = translate(
                            memory_mapping,
                            AccessType::Store,
                            account_ref.vm_data_addr,
                            account.data.len() as u64,
                        )?;
                        *account_ref.ref_to_len_in_vm = account.data.len() as u64;
                        *account_ref.serialized_len_ptr = account.data.len() as u64;
                    }
                    account_ref
                        .data
                        .clone_from_slice(&account.data[0..account_ref.data.len()]);
                }
            }
        }
    }

    Ok(SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_rbpf::{memory_region::MemoryRegion, vm::Config};
    use solana_sdk::{
        bpf_loader,
        hash::hashv,
        process_instruction::{MockComputeMeter, MockLogger},
    };
    use std::str::FromStr;

    const DEFAULT_CONFIG: Config = Config {
        max_call_depth: 20,
        stack_frame_size: 4_096,
        enable_instruction_meter: true,
        enable_instruction_tracing: false,
    };

    macro_rules! assert_access_violation {
        ($result:expr, $va:expr, $len:expr) => {
            match $result {
                Err(EbpfError::AccessViolation(_, _, va, len, _)) if $va == va && len == len => (),
                _ => panic!(),
            }
        };
    }

    #[test]
    fn test_translate() {
        const START: u64 = 100;
        const LENGTH: u64 = 1000;
        let data = vec![0u8; LENGTH as usize];
        let addr = data.as_ptr() as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion::new_from_slice(&data, START, 0, false)],
            &DEFAULT_CONFIG,
        );

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
            if ok {
                assert_eq!(
                    translate(&memory_mapping, AccessType::Load, start, length,).unwrap(),
                    value
                )
            } else {
                assert!(translate(&memory_mapping, AccessType::Load, start, length,).is_err())
            }
        }
    }

    #[test]
    fn test_translate_type() {
        // Pubkey
        let pubkey = solana_sdk::pubkey::new_rand();
        let addr = &pubkey as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: std::mem::size_of::<Pubkey>() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_pubkey =
            translate_type::<Pubkey>(&memory_mapping, 100, &bpf_loader::id()).unwrap();
        assert_eq!(pubkey, *translated_pubkey);

        // Instruction
        let instruction = Instruction::new(
            solana_sdk::pubkey::new_rand(),
            &"foobar",
            vec![AccountMeta::new(solana_sdk::pubkey::new_rand(), false)],
        );
        let addr = &instruction as *const _ as u64;
        let mut memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 96,
                len: std::mem::size_of::<Instruction>() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_instruction =
            translate_type::<Instruction>(&memory_mapping, 96, &bpf_loader::id()).unwrap();
        assert_eq!(instruction, *translated_instruction);
        memory_mapping.resize_region::<BPFError>(0, 1).unwrap();
        assert!(translate_type::<Instruction>(&memory_mapping, 100, &bpf_loader::id()).is_err());
    }

    #[test]
    fn test_translate_slice() {
        // zero len
        let good_data = vec![1u8, 2, 3, 4, 5];
        let data: Vec<u8> = vec![];
        assert_eq!(0x1 as *const u8, data.as_ptr());
        let addr = good_data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: good_data.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_data =
            translate_slice::<u8>(&memory_mapping, data.as_ptr() as u64, 0, &bpf_loader::id())
                .unwrap();
        assert_eq!(data, translated_data);
        assert_eq!(0, translated_data.len());

        // u8
        let mut data = vec![1u8, 2, 3, 4, 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: data.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_data =
            translate_slice::<u8>(&memory_mapping, 100, data.len() as u64, &bpf_loader::id())
                .unwrap();
        assert_eq!(data, translated_data);
        data[0] = 10;
        assert_eq!(data, translated_data);
        assert!(translate_slice::<u8>(
            &memory_mapping,
            data.as_ptr() as u64,
            u64::MAX,
            &bpf_loader::id()
        )
        .is_err());

        assert!(translate_slice::<u8>(
            &memory_mapping,
            100 - 1,
            data.len() as u64,
            &bpf_loader::id()
        )
        .is_err());

        // u64
        let mut data = vec![1u64, 2, 3, 4, 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 96,
                len: (data.len() * size_of::<u64>()) as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_data =
            translate_slice::<u64>(&memory_mapping, 96, data.len() as u64, &bpf_loader::id())
                .unwrap();
        assert_eq!(data, translated_data);
        data[0] = 10;
        assert_eq!(data, translated_data);
        assert!(translate_slice::<u64>(&memory_mapping, 96, u64::MAX, &bpf_loader::id(),).is_err());

        // Pubkeys
        let mut data = vec![solana_sdk::pubkey::new_rand(); 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: (data.len() * std::mem::size_of::<Pubkey>()) as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let translated_data =
            translate_slice::<Pubkey>(&memory_mapping, 100, data.len() as u64, &bpf_loader::id())
                .unwrap();
        assert_eq!(data, translated_data);
        data[0] = solana_sdk::pubkey::new_rand(); // Both should point to same place
        assert_eq!(data, translated_data);
    }

    #[test]
    fn test_translate_string_and_do() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        assert_eq!(
            42,
            translate_string_and_do(
                &memory_mapping,
                100,
                string.len() as u64,
                &bpf_loader::id(),
                &mut |string: &str| {
                    assert_eq!(string, "Gaggablaghblagh!");
                    Ok(42)
                }
            )
            .unwrap()
        );
    }

    #[test]
    #[should_panic(expected = "UserError(SyscallError(Abort))")]
    fn test_syscall_abort() {
        let memory_mapping = MemoryMapping::new(vec![MemoryRegion::default()], &DEFAULT_CONFIG);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        SyscallAbort::call(
            &mut SyscallAbort {},
            0,
            0,
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        result.unwrap();
    }

    #[test]
    #[should_panic(expected = "UserError(SyscallError(Panic(\"Gaggablaghblagh!\", 42, 84)))")]
    fn test_syscall_sol_panic() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );
        let mut syscall_panic = SyscallPanic {
            loader_id: &bpf_loader::id(),
        };
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_panic.call(
            100,
            string.len() as u64,
            42,
            84,
            0,
            &memory_mapping,
            &mut result,
        );
        result.unwrap();
    }

    #[test]
    fn test_syscall_sol_log() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;

        let compute_meter: Rc<RefCell<dyn ComputeMeter>> =
            Rc::new(RefCell::new(MockComputeMeter { remaining: 3 }));
        let log = Rc::new(RefCell::new(vec![]));
        let logger: Rc<RefCell<dyn Logger>> =
            Rc::new(RefCell::new(MockLogger { log: log.clone() }));
        let mut syscall_sol_log = SyscallLog {
            cost: 1,
            compute_meter,
            logger,
            loader_id: &bpf_loader::id(),
        };
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );

        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_log.call(
            100,
            string.len() as u64,
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        result.unwrap();
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0], "Program log: Gaggablaghblagh!");

        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_log.call(
            101, // AccessViolation
            string.len() as u64,
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 101, string.len() as u64);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_log.call(
            100,
            string.len() as u64 * 2, // AccessViolation
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 100, string.len() as u64 * 2);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_log.call(
            100,
            string.len() as u64,
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_eq!(
            Err(EbpfError::UserError(BPFError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ))),
            result
        );
    }

    #[test]
    fn test_syscall_sol_log_u64() {
        let compute_meter: Rc<RefCell<dyn ComputeMeter>> =
            Rc::new(RefCell::new(MockComputeMeter {
                remaining: std::u64::MAX,
            }));
        let log = Rc::new(RefCell::new(vec![]));
        let logger: Rc<RefCell<dyn Logger>> =
            Rc::new(RefCell::new(MockLogger { log: log.clone() }));
        let mut syscall_sol_log_u64 = SyscallLogU64 {
            cost: 0,
            compute_meter,
            logger,
        };
        let memory_mapping = MemoryMapping::new(vec![], &DEFAULT_CONFIG);

        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_log_u64.call(1, 2, 3, 4, 5, &memory_mapping, &mut result);
        result.unwrap();

        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0], "Program log: 0x1, 0x2, 0x3, 0x4, 0x5");
    }

    #[test]
    fn test_syscall_sol_pubkey() {
        let pubkey = Pubkey::from_str("MoqiU1vryuCGQSxFKA1SZ316JdLEFFhoAu6cKUNk7dN").unwrap();
        let addr = &pubkey.as_ref()[0] as *const _ as u64;

        let compute_meter: Rc<RefCell<dyn ComputeMeter>> =
            Rc::new(RefCell::new(MockComputeMeter { remaining: 2 }));
        let log = Rc::new(RefCell::new(vec![]));
        let logger: Rc<RefCell<dyn Logger>> =
            Rc::new(RefCell::new(MockLogger { log: log.clone() }));
        let mut syscall_sol_pubkey = SyscallLogPubkey {
            cost: 1,
            compute_meter,
            logger,
            loader_id: &bpf_loader::id(),
        };
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 100,
                len: 32,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &DEFAULT_CONFIG,
        );

        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_pubkey.call(100, 0, 0, 0, 0, &memory_mapping, &mut result);
        result.unwrap();
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(
            log.borrow()[0],
            "Program log: MoqiU1vryuCGQSxFKA1SZ316JdLEFFhoAu6cKUNk7dN"
        );
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_pubkey.call(
            101, // AccessViolation
            32,
            0,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 101, 32);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall_sol_pubkey.call(100, 32, 0, 0, 0, &memory_mapping, &mut result);
        assert_eq!(
            Err(EbpfError::UserError(BPFError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ))),
            result
        );
    }

    #[test]
    fn test_syscall_sol_alloc_free() {
        // large alloc
        {
            let heap = vec![0_u8; 100];
            let memory_mapping = MemoryMapping::new(
                vec![MemoryRegion::new_from_slice(&heap, MM_HEAP_START, 0, true)],
                &DEFAULT_CONFIG,
            );
            let mut syscall = SyscallAllocFree {
                aligned: true,
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(100, 0, 0, 0, 0, &memory_mapping, &mut result);
            assert_ne!(result.unwrap(), 0);
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(100, 0, 0, 0, 0, &memory_mapping, &mut result);
            assert_eq!(result.unwrap(), 0);
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(u64::MAX, 0, 0, 0, 0, &memory_mapping, &mut result);
            assert_eq!(result.unwrap(), 0);
        }
        // many small unaligned allocs
        {
            let heap = vec![0_u8; 100];
            let memory_mapping = MemoryMapping::new(
                vec![MemoryRegion::new_from_slice(&heap, MM_HEAP_START, 0, true)],
                &DEFAULT_CONFIG,
            );
            let mut syscall = SyscallAllocFree {
                aligned: false,
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            for _ in 0..100 {
                let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
                syscall.call(1, 0, 0, 0, 0, &memory_mapping, &mut result);
                assert_ne!(result.unwrap(), 0);
            }
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(100, 0, 0, 0, 0, &memory_mapping, &mut result);
            assert_eq!(result.unwrap(), 0);
        }
        // many small aligned allocs
        {
            let heap = vec![0_u8; 100];
            let memory_mapping = MemoryMapping::new(
                vec![MemoryRegion::new_from_slice(&heap, MM_HEAP_START, 0, true)],
                &DEFAULT_CONFIG,
            );
            let mut syscall = SyscallAllocFree {
                aligned: true,
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            for _ in 0..12 {
                let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
                syscall.call(1, 0, 0, 0, 0, &memory_mapping, &mut result);
                assert_ne!(result.unwrap(), 0);
            }
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(100, 0, 0, 0, 0, &memory_mapping, &mut result);
            assert_eq!(result.unwrap(), 0);
        }
        // aligned allocs

        fn check_alignment<T>() {
            let heap = vec![0_u8; 100];
            let memory_mapping = MemoryMapping::new(
                vec![MemoryRegion::new_from_slice(&heap, MM_HEAP_START, 0, true)],
                &DEFAULT_CONFIG,
            );
            let mut syscall = SyscallAllocFree {
                aligned: true,
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
            syscall.call(
                size_of::<u8>() as u64,
                0,
                0,
                0,
                0,
                &memory_mapping,
                &mut result,
            );
            let address = result.unwrap();
            assert_ne!(address, 0);
            assert_eq!((address as *const u8).align_offset(align_of::<u8>()), 0);
        }
        check_alignment::<u8>();
        check_alignment::<u16>();
        check_alignment::<u32>();
        check_alignment::<u64>();
        check_alignment::<u128>();
    }

    #[test]
    fn test_syscall_sha256() {
        let bytes1 = "Gaggablaghblagh!";
        let bytes2 = "flurbos";

        struct MockSlice {
            pub addr: u64,
            pub len: usize,
        }
        let mock_slice1 = MockSlice {
            addr: 4096,
            len: bytes1.len(),
        };
        let mock_slice2 = MockSlice {
            addr: 8192,
            len: bytes2.len(),
        };
        let bytes_to_hash = [mock_slice1, mock_slice2];
        let hash_result = [0; HASH_BYTES];
        let ro_len = bytes_to_hash.len() as u64;
        let ro_va = 96;
        let rw_va = 192;
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: bytes1.as_ptr() as *const _ as u64,
                    vm_addr: 4096,
                    len: bytes1.len() as u64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: bytes2.as_ptr() as *const _ as u64,
                    vm_addr: 8192,
                    len: bytes2.len() as u64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: bytes_to_hash.as_ptr() as *const _ as u64,
                    vm_addr: 96,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: hash_result.as_ptr() as *const _ as u64,
                    vm_addr: rw_va,
                    len: HASH_BYTES as u64,
                    vm_gap_shift: 63,
                    is_writable: true,
                },
            ],
            &DEFAULT_CONFIG,
        );
        let compute_meter: Rc<RefCell<dyn ComputeMeter>> =
            Rc::new(RefCell::new(MockComputeMeter {
                remaining: (bytes1.len() + bytes2.len()) as u64,
            }));
        let mut syscall = SyscallSha256 {
            sha256_base_cost: 0,
            sha256_byte_cost: 2,
            compute_meter,
            loader_id: &bpf_loader_deprecated::id(),
        };

        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall.call(ro_va, ro_len, rw_va, 0, 0, &memory_mapping, &mut result);
        result.unwrap();

        let hash_local = hashv(&[bytes1.as_ref(), bytes2.as_ref()]).to_bytes();
        assert_eq!(hash_result, hash_local);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall.call(
            ro_va - 1, // AccessViolation
            ro_len,
            rw_va,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, ro_va - 1, ro_len);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall.call(
            ro_va,
            ro_len + 1, // AccessViolation
            rw_va,
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, ro_va, ro_len + 1);
        let mut result: Result<u64, EbpfError<BPFError>> = Ok(0);
        syscall.call(
            ro_va,
            ro_len,
            rw_va - 1, // AccessViolation
            0,
            0,
            &memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, rw_va - 1, HASH_BYTES as u64);

        syscall.call(ro_va, ro_len, rw_va, 0, 0, &memory_mapping, &mut result);
        assert_eq!(
            Err(EbpfError::UserError(BPFError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ))),
            result
        );
    }
}
