#![allow(non_camel_case_types)]

use std::os::raw::c_int;
use std::pin::Pin;
use {
    solana_bpf_loader_program::{BpfError, ThisInstructionMeter},
    solana_rbpf::{
        elf::Executable,
        vm::{Config, SyscallRegistry},
    },
    std::{os::raw::c_char, ptr::null_mut},
};

/// A virtual machine capable of executing Solana Sealevel programs.
#[derive(Default)]
pub struct sealevel_machine {
    config: Config,
}

impl sealevel_machine {
    fn new() -> Self {
        Self::default()
    }
}

/// Creates a new Sealevel machine environment.
#[no_mangle]
pub unsafe extern "C" fn sealevel_machine_new() -> *mut sealevel_machine {
    let machine = sealevel_machine::new();
    Box::into_raw(Box::new(machine))
}

/// Releases resources associated with a Sealevel machine.
#[no_mangle]
pub unsafe extern "C" fn sealevel_machine_free(machine: *mut sealevel_machine) {
    drop(Box::from_raw(machine))
}

/// The invoke context holds the state of a single transaction execution.
/// It tracks the execution progress (instruction being executed),
/// interfaces with account data,
/// and specifies the on-chain execution rules (precompiles, syscalls, sysvars).
pub struct sealevel_invoke_context {}

/// Access parameters of an account usage in an instruction.
#[repr(C)]
pub struct sealevel_instruction_account {
    pub index_in_transaction: usize,
    pub index_in_caller: usize,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Drops an invoke context and all programs created with it.
#[no_mangle]
pub unsafe extern "C" fn sealevel_invoke_context_free(this: *mut sealevel_invoke_context) {
    drop(Box::from_raw(this))
}

/// Processes a transaction instruction.
#[no_mangle]
pub unsafe extern "C" fn sealevel_process_instruction(
    invoke_context: *mut sealevel_invoke_context,
    data: *const c_char,
    data_len: usize,
    accounts: *const sealevel_instruction_account,
    accounts_len: usize,
    compute_units_consumed: *mut u64,
) {
}

/// A virtual machine program ready to be executed.
pub struct sealevel_program {
    program: Pin<Box<Executable<BpfError, ThisInstructionMeter>>>,
}

/// Loads a Sealevel program from an ELF buffer and verifies its SBF bytecode.
#[no_mangle]
pub unsafe extern "C" fn sealevel_program_create(
    machine: *const sealevel_machine,
    invoke_context: *const sealevel_invoke_context,
    data: *const c_char,
    data_len: usize,
) -> *mut sealevel_program {
    let data_slice = std::slice::from_raw_parts(data as *const u8, data_len);
    // TODO syscall registry
    let load_result = Executable::<BpfError, ThisInstructionMeter>::from_elf(
        data_slice,
        None,
        (*machine).config,
        SyscallRegistry::default(),
    );
    let program = match load_result {
        Err(_) => return null_mut(), // TODO yield error
        Ok(program) => program,
    };
    let wrapped_program = sealevel_program { program };
    Box::into_raw(Box::new(wrapped_program))
}

/// Compiles a program to native executable code.
#[no_mangle]
pub unsafe extern "C" fn sealevel_program_jit_compile(program: *mut sealevel_program) -> c_int {
    0
}

/// Executes a Sealevel program with the given instruction data and accounts.
///
/// Unlike `sealevel_process_instruction`, does not progress the transaction context state machine.
#[no_mangle]
pub unsafe extern "C" fn sealevel_program_execute(
    program: *const sealevel_program,
    invoke_context: *const sealevel_invoke_context,
    data: *const c_char,
    data_len: usize,
    accounts: *const sealevel_instruction_account,
    accounts_len: usize,
) -> u64 {
    0
}
