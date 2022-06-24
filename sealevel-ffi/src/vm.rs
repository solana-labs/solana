use {
    crate::{config::sealevel_config, error::hoist_error, program::sealevel_executable},
    solana_bpf_loader_program::{BpfError, ThisInstructionMeter},
    solana_program_runtime::invoke_context::ComputeMeter,
    solana_rbpf::{verifier::RequisiteVerifier, vm::EbpfVm},
    std::{ffi::c_void, ptr::null_mut, slice},
};

pub struct sealevel_vm {
    pub(crate) vm: EbpfVm<'static, RequisiteVerifier, BpfError, ThisInstructionMeter>, // hack: lifetime is not static
    pub(crate) program: *mut sealevel_executable,
}

/// Creates a Sealevel virtual machine and loads the given program into it.
///
/// Sets `sealevel_errno` and returns a null pointer if loading failed.
// TODO: Support `additional_regions` parameter.
#[no_mangle]
pub unsafe extern "C" fn sealevel_vm_create(
    program: *mut sealevel_executable,
    heap: *mut c_void,
    heap_len: usize,
) -> *mut sealevel_vm {
    let heap_ptr = heap as *mut u8;
    let heap_slice = slice::from_raw_parts_mut(heap_ptr, heap_len);
    let result = EbpfVm::new(&((*program).program), heap_slice, vec![]);
    match hoist_error(result) {
        None => null_mut(),
        Some(vm) => {
            let wrapper = sealevel_vm { vm, program };
            Box::into_raw(Box::new(wrapper))
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn sealevel_vm_execute(vm: *mut sealevel_vm) -> u64 {
    // TODO Configurable instruction meter
    let mut instruction_meter = ThisInstructionMeter::new(ComputeMeter::new_ref(100000));
    let result = if (*(*vm).program).is_jit_compiled {
        (*vm).vm.execute_program_jit(&mut instruction_meter)
    } else {
        (*vm).vm.execute_program_interpreted(&mut instruction_meter)
    };
    let ret_opt = hoist_error(result);
    ret_opt.unwrap_or(0u64)
}
