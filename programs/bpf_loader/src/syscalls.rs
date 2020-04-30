use crate::{alloc, BPFError};
use alloc::Alloc;
use log::*;
use solana_rbpf::{
    ebpf::{EbpfError, SyscallObject, ELF_INSN_DUMP_OFFSET, MM_HEAP_START},
    memory_region::{translate_addr, MemoryRegion},
    EbpfVm,
};
use solana_runtime::message_processor::MessageProcessor;
use solana_sdk::{
    account::Account,
    account_info::AccountInfo,
    bpf_loader,
    entrypoint::SUCCESS,
    entrypoint_native::InvokeContext,
    hash::Hash,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    program_error::ProgramError,
    pubkey::{Pubkey, PubkeyError},
};
use std::{
    alloc::Layout,
    cell::{RefCell, RefMut},
    convert::TryFrom,
    mem::{align_of, size_of},
    rc::Rc,
    slice::from_raw_parts_mut,
    str::{from_utf8, Utf8Error},
};
use thiserror::Error as ThisError;

/// Error definitions
#[derive(Debug, ThisError)]
pub enum SyscallError {
    #[error("{0}: {1:?}")]
    InvalidString(Utf8Error, Vec<u8>),
    #[error("BPF program called abort()!")]
    Abort,
    #[error("BPF program Panicked at {0}, {1}:{2}")]
    Panic(String, u64, u64),
    #[error("cannot borrow invoke context")]
    InvokeContextBorrowFailed,
    #[error("malformed signer seed: {0}: {1:?}")]
    MalformedSignerSeed(Utf8Error, Vec<u8>),
    #[error("Could not create program address with signer seeds: {0}")]
    BadSeeds(PubkeyError),
    #[error("Program id is not supported by cross-program invocations")]
    ProgramNotSupported,
    #[error("{0}")]
    InstructionError(InstructionError),
}
impl From<SyscallError> for EbpfError<BPFError> {
    fn from(error: SyscallError) -> Self {
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

pub fn register_syscalls<'a>(
    vm: &mut EbpfVm<'a, BPFError>,
    invoke_context: &'a mut dyn InvokeContext,
) -> Result<MemoryRegion, EbpfError<BPFError>> {
    // Syscall function common across languages
    vm.register_syscall_ex("abort", syscall_abort)?;
    vm.register_syscall_ex("sol_panic_", syscall_sol_panic)?;
    vm.register_syscall_ex("sol_log_", syscall_sol_log)?;
    vm.register_syscall_ex("sol_log_64_", syscall_sol_log_u64)?;

    // Cross-program invocation syscalls
    {
        let invoke_context = Rc::new(RefCell::new(invoke_context));
        vm.register_syscall_with_context_ex(
            "sol_invoke_signed_c",
            Box::new(SyscallProcessSolInstructionC {
                invoke_context: invoke_context.clone(),
            }),
        )?;
        vm.register_syscall_with_context_ex(
            "sol_invoke_signed_rust",
            Box::new(SyscallProcessInstructionRust {
                invoke_context: invoke_context.clone(),
            }),
        )?;
    }

    // Memory allocator
    let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
    let heap_region = MemoryRegion::new_from_slice(&heap, MM_HEAP_START);
    vm.register_syscall_with_context_ex(
        "sol_alloc_free_",
        Box::new(SyscallSolAllocFree {
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
        Err(err) => Err(SyscallError::InvalidString(err, buf[..i].to_vec()).into()),
    }
}

/// Abort syscall functions, called when the BPF program calls `abort()`
/// LLVM will insert calls to `abort()` if it detects an untenable situation,
/// `abort()` is not intended to be called explicitly by the program.
/// Causes the BPF program to be halted immediately
pub fn syscall_abort(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    _ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    Err(SyscallError::Abort.into())
}

/// Panic syscall function, called when the BPF program calls 'sol_panic_()`
/// Causes the BPF program to be halted immediately
pub fn syscall_sol_panic(
    file: u64,
    len: u64,
    line: u64,
    column: u64,
    _arg5: u64,
    ro_regions: &[MemoryRegion],
    _rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    translate_string_and_do(file, len, ro_regions, &|string: &str| {
        Err(SyscallError::Panic(string.to_string(), line, column).into())
    })
}

/// Log a user's info message
pub fn syscall_sol_log(
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

/// Log 5 64-bit values
pub fn syscall_sol_log_u64(
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

/// Dynamic memory allocation syscall called when the BPF program calls
/// `sol_alloc_free_()`.  The allocator is expected to allocate/free
/// from/to a given chunk of memory and enforce size restrictions.  The
/// memory chunk is given to the allocator during allocator creation and
/// information about that memory (start address and size) is passed
/// to the VM to use for enforcement.
pub struct SyscallSolAllocFree {
    allocator: BPFAllocator,
}
impl SyscallObject<BPFError> for SyscallSolAllocFree {
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

// Cross-program invocation syscalls

pub type TranslatedAccounts<'a> = (Vec<Rc<RefCell<Account>>>, Vec<(&'a mut u64, &'a mut [u8])>);

/// Implemented by language specific data structure translators
trait SyscallProcessInstruction<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>>;
    fn translate_instruction(
        &self,
        addr: u64,
        ro_regions: &[MemoryRegion],
    ) -> Result<Instruction, EbpfError<BPFError>>;
    fn translate_accounts(
        &self,
        message: &Message,
        account_infos_addr: u64,
        account_infos_len: usize,
        ro_regions: &[MemoryRegion],
        rw_regions: &[MemoryRegion],
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>>;
    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: usize,
        ro_regions: &[MemoryRegion],
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>>;
}

/// Cross-program invocation called from Rust
pub struct SyscallProcessInstructionRust<'a> {
    invoke_context: Rc<RefCell<&'a mut dyn InvokeContext>>,
}
impl<'a> SyscallProcessInstruction<'a> for SyscallProcessInstructionRust<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }
    fn translate_instruction(
        &self,
        addr: u64,
        ro_regions: &[MemoryRegion],
    ) -> Result<Instruction, EbpfError<BPFError>> {
        let ix = translate_type!(Instruction, addr, ro_regions)?;
        let accounts = translate_slice!(
            AccountMeta,
            ix.accounts.as_ptr(),
            ix.accounts.len(),
            ro_regions
        )?
        .to_vec();
        let data = translate_slice!(u8, ix.data.as_ptr(), ix.data.len(), ro_regions)?.to_vec();
        Ok(Instruction {
            program_id: ix.program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts(
        &self,
        message: &Message,
        account_infos_addr: u64,
        account_infos_len: usize,
        ro_regions: &[MemoryRegion],
        rw_regions: &[MemoryRegion],
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>> {
        let account_infos = if account_infos_len > 0 {
            translate_slice!(
                AccountInfo,
                account_infos_addr,
                account_infos_len,
                ro_regions
            )?
        } else {
            &[]
        };

        let mut accounts = Vec::with_capacity(message.account_keys.len());
        let mut refs = Vec::with_capacity(message.account_keys.len());
        'root: for account_key in message.account_keys.iter() {
            for account_info in account_infos.iter() {
                let key = translate_type!(Pubkey, account_info.key as *const _, ro_regions)?;
                if account_key == key {
                    let lamports_ref = {
                        // Double translate lamports out of RefCell
                        let ptr = translate_type!(u64, account_info.lamports.as_ptr(), ro_regions)?;
                        translate_type_mut!(u64, *(ptr as *const u64), rw_regions)?
                    };
                    let data = {
                        // Double translate data out of RefCell
                        let data = *translate_type!(&[u8], account_info.data.as_ptr(), ro_regions)?;
                        translate_slice_mut!(u8, data.as_ptr(), data.len(), rw_regions)?
                    };
                    let owner =
                        translate_type!(Pubkey, account_info.owner as *const _, ro_regions)?;

                    accounts.push(Rc::new(RefCell::new(Account {
                        lamports: *lamports_ref,
                        data: data.to_vec(),
                        executable: account_info.executable,
                        owner: *owner,
                        rent_epoch: account_info.rent_epoch,
                        hash: Hash::default(),
                    })));
                    refs.push((lamports_ref, data));
                    continue 'root;
                }
            }
            return Err(SyscallError::InstructionError(InstructionError::MissingAccount).into());
        }

        Ok((accounts, refs))
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: usize,
        ro_regions: &[MemoryRegion],
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>> {
        let mut signers = Vec::new();
        if signers_seeds_len > 0 {
            let signers_seeds =
                translate_slice!(&[&str], signers_seeds_addr, signers_seeds_len, ro_regions)?;
            for signer_seeds in signers_seeds.iter() {
                let untranslated_seeds =
                    translate_slice!(&str, signer_seeds.as_ptr(), signer_seeds.len(), ro_regions)?;
                let seeds = untranslated_seeds
                    .iter()
                    .map(|untranslated_seed| {
                        let seed_bytes = translate_slice!(
                            u8,
                            untranslated_seed.as_ptr(),
                            untranslated_seed.len(),
                            ro_regions
                        )?;
                        from_utf8(seed_bytes).map_err(|err| {
                            SyscallError::MalformedSignerSeed(err, seed_bytes.to_vec()).into()
                        })
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
impl<'a> SyscallObject<BPFError> for SyscallProcessInstructionRust<'a> {
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        ro_regions: &[MemoryRegion],
        rw_regions: &[MemoryRegion],
    ) -> Result<u64, EbpfError<BPFError>> {
        call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            ro_regions,
            rw_regions,
        )
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
    data_len: usize,
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
pub struct SyscallProcessSolInstructionC<'a> {
    invoke_context: Rc<RefCell<&'a mut dyn InvokeContext>>,
}
impl<'a> SyscallProcessInstruction<'a> for SyscallProcessSolInstructionC<'a> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut dyn InvokeContext>, EbpfError<BPFError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }

    fn translate_instruction(
        &self,
        addr: u64,
        ro_regions: &[MemoryRegion],
    ) -> Result<Instruction, EbpfError<BPFError>> {
        let ix_c = translate_type!(SolInstruction, addr, ro_regions)?;
        let program_id = translate_type!(Pubkey, ix_c.program_id_addr, ro_regions)?;
        let meta_cs = translate_slice!(
            SolAccountMeta,
            ix_c.accounts_addr,
            ix_c.accounts_len,
            ro_regions
        )?;
        let data = translate_slice!(u8, ix_c.data_addr, ix_c.data_len, ro_regions)?.to_vec();
        let accounts = meta_cs
            .iter()
            .map(|meta_c| {
                let pubkey = translate_type!(Pubkey, meta_c.pubkey_addr, ro_regions)?;
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
        message: &Message,
        account_infos_addr: u64,
        account_infos_len: usize,
        ro_regions: &[MemoryRegion],
        rw_regions: &[MemoryRegion],
    ) -> Result<TranslatedAccounts<'a>, EbpfError<BPFError>> {
        let account_infos = translate_slice!(
            SolAccountInfo,
            account_infos_addr,
            account_infos_len,
            ro_regions
        )?;
        let mut accounts = Vec::with_capacity(message.account_keys.len());
        let mut refs = Vec::with_capacity(message.account_keys.len());
        'root: for account_key in message.account_keys.iter() {
            for account_info in account_infos.iter() {
                let key = translate_type!(Pubkey, account_info.key_addr, ro_regions)?;
                if account_key == key {
                    let lamports_ref =
                        translate_type_mut!(u64, account_info.lamports_addr, rw_regions)?;
                    let data = translate_slice_mut!(
                        u8,
                        account_info.data_addr,
                        account_info.data_len,
                        rw_regions
                    )?;
                    let owner = translate_type!(Pubkey, account_info.owner_addr, ro_regions)?;

                    accounts.push(Rc::new(RefCell::new(Account {
                        lamports: *lamports_ref,
                        data: data.to_vec(),
                        executable: account_info.executable,
                        owner: *owner,
                        rent_epoch: account_info.rent_epoch,
                        hash: Hash::default(),
                    })));
                    refs.push((lamports_ref, data));
                    continue 'root;
                }
            }
            return Err(SyscallError::InstructionError(InstructionError::MissingAccount).into());
        }

        Ok((accounts, refs))
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: usize,
        ro_regions: &[MemoryRegion],
    ) -> Result<Vec<Pubkey>, EbpfError<BPFError>> {
        if signers_seeds_len > 0 {
            let signers_seeds = translate_slice!(
                SolSignerSeedC,
                signers_seeds_addr,
                signers_seeds_len,
                ro_regions
            )?;
            Ok(signers_seeds
                .iter()
                .map(|signer_seeds| {
                    let seeds = translate_slice!(
                        SolSignerSeedC,
                        signer_seeds.addr,
                        signer_seeds.len,
                        ro_regions
                    )?;
                    let seed_strs = seeds
                        .iter()
                        .map(|seed| {
                            let seed_bytes = translate_slice!(u8, seed.addr, seed.len, ro_regions)?;
                            std::str::from_utf8(seed_bytes).map_err(|err| {
                                SyscallError::MalformedSignerSeed(err, seed_bytes.to_vec()).into()
                            })
                        })
                        .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?;
                    Pubkey::create_program_address(&seed_strs, program_id)
                        .map_err(|err| SyscallError::BadSeeds(err).into())
                })
                .collect::<Result<Vec<_>, EbpfError<BPFError>>>()?)
        } else {
            Ok(vec![])
        }
    }
}
impl<'a> SyscallObject<BPFError> for SyscallProcessSolInstructionC<'a> {
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        ro_regions: &[MemoryRegion],
        rw_regions: &[MemoryRegion],
    ) -> Result<u64, EbpfError<BPFError>> {
        call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            ro_regions,
            rw_regions,
        )
    }
}

/// Call process instruction, common to both Rust and C
fn call<'a>(
    syscall: &mut dyn SyscallProcessInstruction<'a>,
    instruction_addr: u64,
    account_infos_addr: u64,
    account_infos_len: u64,
    signers_seeds_addr: u64,
    signers_seeds_len: u64,
    ro_regions: &[MemoryRegion],
    rw_regions: &[MemoryRegion],
) -> Result<u64, EbpfError<BPFError>> {
    let mut invoke_context = syscall.get_context_mut()?;

    // Translate data passed from the VM

    let instruction = syscall.translate_instruction(instruction_addr, ro_regions)?;
    let message = Message::new(&[instruction]);
    let program_id_index = message.instructions[0].program_id_index as usize;
    let program_id = message.account_keys[program_id_index];
    let (accounts, refs) = syscall.translate_accounts(
        &message,
        account_infos_addr,
        account_infos_len as usize,
        ro_regions,
        rw_regions,
    )?;
    let signers = syscall.translate_signers(
        &program_id,
        signers_seeds_addr,
        signers_seeds_len as usize,
        ro_regions,
    )?;

    // Process instruction

    let program_account = (*accounts[program_id_index]).clone();
    if program_account.borrow().owner != bpf_loader::id() {
        // Only BPF programs supported for now
        return Err(SyscallError::ProgramNotSupported.into());
    }
    let executable_accounts = vec![(program_id, program_account)];

    #[allow(clippy::deref_addrof)]
    match MessageProcessor::process_cross_program_instruction(
        &message,
        &executable_accounts,
        &accounts,
        &signers,
        crate::process_instruction,
        *(&mut *invoke_context),
    ) {
        Ok(()) => (),
        Err(err) => match ProgramError::try_from(err) {
            Ok(err) => return Ok(err.into()),
            Err(err) => return Err(SyscallError::InstructionError(err).into()),
        },
    }

    // Copy results back into caller's AccountInfos
    for (i, (account, (lamport_ref, data))) in accounts.iter().zip(refs).enumerate() {
        let account = account.borrow();
        if message.is_writable(i) && !account.executable {
            *lamport_ref = account.lamports;
            data.clone_from_slice(&account.data);
        }
    }

    Ok(SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[should_panic(expected = "UserError(SyscallError(Abort))")]
    fn test_syscall_abort() {
        let ro_region = MemoryRegion::default();
        let rw_region = MemoryRegion::default();
        syscall_abort(0, 0, 0, 0, 0, &[ro_region], &[rw_region]).unwrap();
    }

    #[test]
    #[should_panic(expected = "UserError(SyscallError(Panic(\"Gaggablaghblagh!\", 42, 84)))")]
    fn test_syscall_sol_panic() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let ro_region = MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: string.len() as u64,
        };
        let rw_region = MemoryRegion::default();
        syscall_sol_panic(
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
    fn test_syscall_sol_log() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let ro_regions = &[MemoryRegion {
            addr_host: addr,
            addr_vm: 100,
            len: string.len() as u64,
        }];
        let rw_regions = &[MemoryRegion::default()];
        solana_logger::setup_with_default("solana=info");
        syscall_sol_log(100, string.len() as u64, 0, 0, 0, ro_regions, rw_regions).unwrap();
        solana_logger::setup_with_default("solana=info");
        syscall_sol_log(
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
    fn test_syscall_sol_log_u64() {
        solana_logger::setup_with_default("solana=info");

        let ro_regions = &[MemoryRegion::default()];
        let rw_regions = &[MemoryRegion::default()];
        syscall_sol_log_u64(1, 2, 3, 4, 5, ro_regions, rw_regions).unwrap();
    }

    #[test]
    fn test_syscall_sol_alloc_free() {
        // large alloc
        {
            let heap = vec![0_u8; 100];
            let ro_regions = &[MemoryRegion::default()];
            let rw_regions = &[MemoryRegion::new_from_slice(&heap, MM_HEAP_START)];
            let mut syscall = SyscallSolAllocFree {
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            assert_ne!(
                syscall
                    .call(100, 0, 0, 0, 0, ro_regions, rw_regions)
                    .unwrap(),
                0
            );
            assert_eq!(
                syscall
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
            let mut syscall = SyscallSolAllocFree {
                allocator: BPFAllocator::new(heap, MM_HEAP_START),
            };
            for _ in 0..100 {
                assert_ne!(
                    syscall.call(1, 0, 0, 0, 0, ro_regions, rw_regions).unwrap(),
                    0
                );
            }
            assert_eq!(
                syscall
                    .call(100, 0, 0, 0, 0, ro_regions, rw_regions)
                    .unwrap(),
                0
            );
        }
    }
}
