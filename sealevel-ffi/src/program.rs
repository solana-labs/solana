use {
    crate::{
        context::{sealevel_invoke_context, sealevel_syscall_registry},
        error::set_result,
        machine::sealevel_machine,
    },
    solana_bpf_loader_program::{BpfError, ThisInstructionMeter},
    solana_rbpf::elf::Executable,
    std::{
        os::raw::{c_char, c_int},
        pin::Pin,
        ptr::null_mut,
    },
};

/// A virtual machine program ready to be executed.
pub struct sealevel_program {
    program: Pin<Box<Executable<BpfError, ThisInstructionMeter>>>,
}

/// Access parameters of an account usage in an instruction.
#[repr(C)]
pub struct sealevel_instruction_account {
    pub index_in_transaction: usize,
    pub index_in_caller: usize,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Loads a Sealevel program from an ELF buffer and verifies its SBF bytecode.
///
/// Consumes the given syscall registry.
#[no_mangle]
pub unsafe extern "C" fn sealevel_program_create(
    machine: *const sealevel_machine,
    syscalls: sealevel_syscall_registry,
    data: *const c_char,
    data_len: usize,
) -> *mut sealevel_program {
    let data_slice = std::slice::from_raw_parts(data as *const u8, data_len);
    // TODO syscall registry
    let load_result = Executable::<BpfError, ThisInstructionMeter>::from_elf(
        data_slice,
        None,
        (*machine).config,
        *syscalls,
    );
    match set_result(load_result) {
        None => null_mut(),
        Some(program) => {
            let wrapped_program = sealevel_program { program };
            Box::into_raw(Box::new(wrapped_program))
        }
    }
}

/// Compiles a program to native executable code.
///
/// Sets `sealevel_errno`.
#[no_mangle]
pub unsafe extern "C" fn sealevel_program_jit_compile(program: *mut sealevel_program) {
    let result = Executable::jit_compile(&mut (*program).program);
    set_result(result);
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
    unimplemented!()
}
