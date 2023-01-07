pub use self::{
    cpi::{SyscallInvokeSignedC, SyscallInvokeSignedRust},
    logging::{
        SyscallLog, SyscallLogBpfComputeUnits, SyscallLogData, SyscallLogPubkey, SyscallLogU64,
    },
    mem_ops::{SyscallMemcmp, SyscallMemcpy, SyscallMemmove, SyscallMemset},
    sysvar::{
        SyscallGetClockSysvar, SyscallGetEpochScheduleSysvar, SyscallGetFeesSysvar,
        SyscallGetRentSysvar,
    },
};
#[allow(deprecated)]
use {
    crate::BpfError,
    solana_program_runtime::{
        compute_budget::ComputeBudget, ic_logger_msg, ic_msg, invoke_context::InvokeContext,
        stable_log, timings::ExecuteTimings,
    },
    solana_rbpf::{
        error::EbpfError,
        memory_region::{AccessType, MemoryMapping},
        vm::{BuiltInProgram, Config, ProgramResult, PROGRAM_ENVIRONMENT_KEY_SHIFT},
    },
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        account_info::AccountInfo,
        alt_bn128::prelude::{
            alt_bn128_addition, alt_bn128_multiplication, alt_bn128_pairing, AltBn128Error,
            ALT_BN128_ADDITION_OUTPUT_LEN, ALT_BN128_MULTIPLICATION_OUTPUT_LEN,
            ALT_BN128_PAIRING_ELEMENT_LEN, ALT_BN128_PAIRING_OUTPUT_LEN,
        },
        blake3, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        entrypoint::{BPF_ALIGN_OF_U128, MAX_PERMITTED_DATA_INCREASE, SUCCESS},
        feature_set::FeatureSet,
        feature_set::{
            self, blake3_syscall_enabled, check_syscall_outputs_do_not_overlap,
            curve25519_syscall_enabled, disable_cpi_setting_executable_and_rent_epoch,
            disable_fees_sysvar, enable_alt_bn128_syscall,
            enable_early_verification_of_account_modifications,
            error_on_syscall_bpf_function_hash_collisions, libsecp256k1_0_5_upgrade_enabled,
            limit_secp256k1_recovery_id, reject_callx_r10,
            stop_sibling_instruction_search_at_parent,
        },
        hash::{Hasher, HASH_BYTES},
        instruction::{
            AccountMeta, Instruction, InstructionError, ProcessedSiblingInstruction,
            TRANSACTION_LEVEL_STACK_HEIGHT,
        },
        keccak, native_loader,
        precompiles::is_precompile,
        program::MAX_RETURN_DATA,
        program_stubs::is_nonoverlapping,
        pubkey::{Pubkey, PubkeyError, MAX_SEEDS, MAX_SEED_LEN},
        secp256k1_recover::{
            Secp256k1RecoverError, SECP256K1_PUBLIC_KEY_LENGTH, SECP256K1_SIGNATURE_LENGTH,
        },
        sysvar::{Sysvar, SysvarId},
        transaction_context::{IndexOfAccount, InstructionAccount},
    },
    std::{
        alloc::Layout,
        mem::{align_of, size_of},
        slice::from_raw_parts_mut,
        str::{from_utf8, Utf8Error},
        sync::Arc,
    },
    thiserror::Error as ThisError,
};

mod cpi;
mod logging;
mod mem_ops;
mod sysvar;

/// Maximum signers
pub const MAX_SIGNERS: usize = 16;

/// Error definitions
#[derive(Debug, ThisError, PartialEq, Eq)]
pub enum SyscallError {
    #[error("{0}: {1:?}")]
    InvalidString(Utf8Error, Vec<u8>),
    #[error("SBF program panicked")]
    Abort,
    #[error("SBF program Panicked in {0} at {1}:{2}")]
    Panic(String, u64, u64),
    #[error("Cannot borrow invoke context")]
    InvokeContextBorrowFailed,
    #[error("Malformed signer seed: {0}: {1:?}")]
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
    #[error("Overlapping copy")]
    CopyOverlapping,
    #[error("Return data too large ({0} > {1})")]
    ReturnDataTooLarge(u64, u64),
    #[error("Hashing too many sequences")]
    TooManySlices,
    #[error("InvalidLength")]
    InvalidLength,
    #[error("Invoked an instruction with data that is too large ({data_len} > {max_data_len})")]
    MaxInstructionDataLenExceeded { data_len: u64, max_data_len: u64 },
    #[error("Invoked an instruction with too many accounts ({num_accounts} > {max_accounts})")]
    MaxInstructionAccountsExceeded {
        num_accounts: u64,
        max_accounts: u64,
    },
    #[error("Invoked an instruction with too many account info's ({num_account_infos} > {max_account_infos})")]
    MaxInstructionAccountInfosExceeded {
        num_account_infos: u64,
        max_account_infos: u64,
    },
    #[error("InvalidAttribute")]
    InvalidAttribute,
}
impl From<SyscallError> for EbpfError {
    fn from(error: SyscallError) -> Self {
        EbpfError::UserError(Box::<BpfError>::new(error.into()))
    }
}

fn consume_compute_meter(invoke_context: &InvokeContext, amount: u64) -> Result<(), EbpfError> {
    invoke_context
        .consume_checked(amount)
        .map_err(SyscallError::InstructionError)?;
    Ok(())
}

macro_rules! register_feature_gated_function {
    ($result:expr, $is_feature_active:expr, $name:expr, $call:expr $(,)?) => {
        if $is_feature_active {
            $result.register_function_by_name($name, $call)
        } else {
            Ok(())
        }
    };
}

pub fn create_loader<'a>(
    feature_set: &FeatureSet,
    compute_budget: &ComputeBudget,
    reject_deployment_of_broken_elfs: bool,
    disable_deploy_of_alloc_free_syscall: bool,
    debugging_features: bool,
) -> Result<Arc<BuiltInProgram<InvokeContext<'a>>>, EbpfError> {
    use rand::Rng;
    let config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_stack_frame_gaps: true,
        instruction_meter_checkpoint_distance: 10000,
        enable_instruction_meter: true,
        enable_instruction_tracing: debugging_features,
        enable_symbol_and_section_labels: debugging_features,
        reject_broken_elfs: reject_deployment_of_broken_elfs,
        noop_instruction_rate: 256,
        sanitize_user_provided_values: true,
        runtime_environment_key: rand::thread_rng()
            .gen::<i32>()
            .checked_shr(PROGRAM_ENVIRONMENT_KEY_SHIFT)
            .unwrap_or(0),
        external_internal_function_hash_collision: feature_set
            .is_active(&error_on_syscall_bpf_function_hash_collisions::id()),
        reject_callx_r10: feature_set.is_active(&reject_callx_r10::id()),
        dynamic_stack_frames: false,
        enable_sdiv: false,
        optimize_rodata: false,
        static_syscalls: false,
        enable_elf_vaddr: false,
        reject_rodata_stack_overlap: false,
        new_elf_parser: false,
        aligned_memory_mapping: true,
        // Warning, do not use `Config::default()` so that configuration here is explicit.
    };

    let enable_alt_bn128_syscall = feature_set.is_active(&enable_alt_bn128_syscall::id());
    let blake3_syscall_enabled = feature_set.is_active(&blake3_syscall_enabled::id());
    let curve25519_syscall_enabled = feature_set.is_active(&curve25519_syscall_enabled::id());
    let disable_fees_sysvar = feature_set.is_active(&disable_fees_sysvar::id());
    let is_abi_v2 = false;

    let mut result = BuiltInProgram::new_loader(config);

    // Abort
    result.register_function_by_name("abort", SyscallAbort::call)?;

    // Panic
    result.register_function_by_name("sol_panic_", SyscallPanic::call)?;

    // Logging
    result.register_function_by_name("sol_log_", SyscallLog::call)?;
    result.register_function_by_name("sol_log_64_", SyscallLogU64::call)?;
    result.register_function_by_name("sol_log_compute_units_", SyscallLogBpfComputeUnits::call)?;
    result.register_function_by_name("sol_log_pubkey", SyscallLogPubkey::call)?;

    // Program defined addresses (PDA)
    result.register_function_by_name(
        "sol_create_program_address",
        SyscallCreateProgramAddress::call,
    )?;
    result.register_function_by_name(
        "sol_try_find_program_address",
        SyscallTryFindProgramAddress::call,
    )?;

    // Sha256
    result.register_function_by_name("sol_sha256", SyscallSha256::call)?;

    // Keccak256
    result.register_function_by_name("sol_keccak256", SyscallKeccak256::call)?;

    // Secp256k1 Recover
    result.register_function_by_name("sol_secp256k1_recover", SyscallSecp256k1Recover::call)?;

    // Blake3
    register_feature_gated_function!(
        result,
        blake3_syscall_enabled,
        "sol_blake3",
        SyscallBlake3::call,
    )?;

    // Elliptic Curve Operations
    register_feature_gated_function!(
        result,
        curve25519_syscall_enabled,
        "sol_curve_validate_point",
        SyscallCurvePointValidation::call,
    )?;
    register_feature_gated_function!(
        result,
        curve25519_syscall_enabled,
        "sol_curve_group_op",
        SyscallCurveGroupOps::call,
    )?;
    register_feature_gated_function!(
        result,
        curve25519_syscall_enabled,
        "sol_curve_multiscalar_mul",
        SyscallCurveMultiscalarMultiplication::call,
    )?;

    // Sysvars
    result.register_function_by_name("sol_get_clock_sysvar", SyscallGetClockSysvar::call)?;
    result.register_function_by_name(
        "sol_get_epoch_schedule_sysvar",
        SyscallGetEpochScheduleSysvar::call,
    )?;
    register_feature_gated_function!(
        result,
        !disable_fees_sysvar,
        "sol_get_fees_sysvar",
        SyscallGetFeesSysvar::call,
    )?;
    result.register_function_by_name("sol_get_rent_sysvar", SyscallGetRentSysvar::call)?;

    // Memory ops
    result.register_function_by_name("sol_memcpy_", SyscallMemcpy::call)?;
    result.register_function_by_name("sol_memmove_", SyscallMemmove::call)?;
    result.register_function_by_name("sol_memcmp_", SyscallMemcmp::call)?;
    result.register_function_by_name("sol_memset_", SyscallMemset::call)?;

    if !is_abi_v2 {
        // Processed sibling instructions
        result.register_function_by_name(
            "sol_get_processed_sibling_instruction",
            SyscallGetProcessedSiblingInstruction::call,
        )?;

        // Stack height
        result.register_function_by_name("sol_get_stack_height", SyscallGetStackHeight::call)?;

        // Return data
        result.register_function_by_name("sol_set_return_data", SyscallSetReturnData::call)?;
        result.register_function_by_name("sol_get_return_data", SyscallGetReturnData::call)?;

        // Cross-program invocation
        result.register_function_by_name("sol_invoke_signed_c", SyscallInvokeSignedC::call)?;
        result
            .register_function_by_name("sol_invoke_signed_rust", SyscallInvokeSignedRust::call)?;

        // Memory allocator
        register_feature_gated_function!(
            result,
            !disable_deploy_of_alloc_free_syscall,
            "sol_alloc_free_",
            SyscallAllocFree::call,
        )?;

        // Alt_bn128
        register_feature_gated_function!(
            result,
            enable_alt_bn128_syscall,
            "sol_alt_bn128_group_op",
            SyscallAltBn128::call,
        )?;
    }

    // Log data
    result.register_function_by_name("sol_log_data", SyscallLogData::call)?;

    Ok(Arc::new(result))
}

fn translate(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    len: u64,
) -> Result<u64, EbpfError> {
    memory_mapping.map(access_type, vm_addr, len, 0).into()
}

fn translate_type_inner<'a, T>(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    check_aligned: bool,
) -> Result<&'a mut T, EbpfError> {
    let host_addr = translate(memory_mapping, access_type, vm_addr, size_of::<T>() as u64)?;

    if check_aligned && (host_addr as *mut T as usize).wrapping_rem(align_of::<T>()) != 0 {
        return Err(SyscallError::UnalignedPointer.into());
    }
    Ok(unsafe { &mut *(host_addr as *mut T) })
}
fn translate_type_mut<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    check_aligned: bool,
) -> Result<&'a mut T, EbpfError> {
    translate_type_inner::<T>(memory_mapping, AccessType::Store, vm_addr, check_aligned)
}
fn translate_type<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    check_aligned: bool,
) -> Result<&'a T, EbpfError> {
    translate_type_inner::<T>(memory_mapping, AccessType::Load, vm_addr, check_aligned)
        .map(|value| &*value)
}

fn translate_slice_inner<'a, T>(
    memory_mapping: &MemoryMapping,
    access_type: AccessType,
    vm_addr: u64,
    len: u64,
    check_aligned: bool,
    check_size: bool,
) -> Result<&'a mut [T], EbpfError> {
    if len == 0 {
        return Ok(&mut []);
    }

    let total_size = len.saturating_mul(size_of::<T>() as u64);
    if check_size && isize::try_from(total_size).is_err() {
        return Err(SyscallError::InvalidLength.into());
    }

    let host_addr = translate(memory_mapping, access_type, vm_addr, total_size)?;

    if check_aligned && (host_addr as *mut T as usize).wrapping_rem(align_of::<T>()) != 0 {
        return Err(SyscallError::UnalignedPointer.into());
    }
    Ok(unsafe { from_raw_parts_mut(host_addr as *mut T, len as usize) })
}
fn translate_slice_mut<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    len: u64,
    check_aligned: bool,
    check_size: bool,
) -> Result<&'a mut [T], EbpfError> {
    translate_slice_inner::<T>(
        memory_mapping,
        AccessType::Store,
        vm_addr,
        len,
        check_aligned,
        check_size,
    )
}
fn translate_slice<'a, T>(
    memory_mapping: &MemoryMapping,
    vm_addr: u64,
    len: u64,
    check_aligned: bool,
    check_size: bool,
) -> Result<&'a [T], EbpfError> {
    translate_slice_inner::<T>(
        memory_mapping,
        AccessType::Load,
        vm_addr,
        len,
        check_aligned,
        check_size,
    )
    .map(|value| &*value)
}

/// Take a virtual pointer to a string (points to SBF VM memory space), translate it
/// pass it to a user-defined work function
fn translate_string_and_do(
    memory_mapping: &MemoryMapping,
    addr: u64,
    len: u64,
    check_aligned: bool,
    check_size: bool,
    work: &mut dyn FnMut(&str) -> Result<u64, EbpfError>,
) -> Result<u64, EbpfError> {
    let buf = translate_slice::<u8>(memory_mapping, addr, len, check_aligned, check_size)?;
    let i = match buf.iter().position(|byte| *byte == 0) {
        Some(i) => i,
        None => len as usize,
    };
    let msg = buf.get(..i).ok_or(SyscallError::InvalidLength)?;
    match from_utf8(msg) {
        Ok(message) => work(message),
        Err(err) => Err(SyscallError::InvalidString(err, msg.to_vec()).into()),
    }
}

#[macro_export]
macro_rules! declare_syscall {
    ($(#[$attr:meta])* $name:ident, $inner_call:item) => {
        $(#[$attr])*
        pub struct $name {}
        impl $name {
            $inner_call
            pub fn call(
                invoke_context: &mut InvokeContext,
                arg_a: u64,
                arg_b: u64,
                arg_c: u64,
                arg_d: u64,
                arg_e: u64,
                memory_mapping: &mut MemoryMapping,
                result: &mut ProgramResult,
            ) {
                let converted_result: ProgramResult = Self::inner_call(
                    invoke_context, arg_a, arg_b, arg_c, arg_d, arg_e, memory_mapping,
                ).into();
                *result = converted_result;
            }
        }
    };
}

declare_syscall!(
    /// Abort syscall functions, called when the SBF program calls `abort()`
    /// LLVM will insert calls to `abort()` if it detects an untenable situation,
    /// `abort()` is not intended to be called explicitly by the program.
    /// Causes the SBF program to be halted immediately
    SyscallAbort,
    fn inner_call(
        _invoke_context: &mut InvokeContext,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        Err(SyscallError::Abort.into())
    }
);

declare_syscall!(
    /// Panic syscall function, called when the SBF program calls 'sol_panic_()`
    /// Causes the SBF program to be halted immediately
    SyscallPanic,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        file: u64,
        len: u64,
        line: u64,
        column: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        consume_compute_meter(invoke_context, len)?;

        translate_string_and_do(
            memory_mapping,
            file,
            len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
            &mut |string: &str| Err(SyscallError::Panic(string.to_string(), line, column).into()),
        )
    }
);

declare_syscall!(
    /// Dynamic memory allocation syscall called when the SBF program calls
    /// `sol_alloc_free_()`.  The allocator is expected to allocate/free
    /// from/to a given chunk of memory and enforce size restrictions.  The
    /// memory chunk is given to the allocator during allocator creation and
    /// information about that memory (start address and size) is passed
    /// to the VM to use for enforcement.
    SyscallAllocFree,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        size: u64,
        free_addr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let allocator = invoke_context
            .get_allocator()
            .map_err(SyscallError::InstructionError)?;
        let mut allocator = allocator
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed)?;

        let align = if invoke_context.get_check_aligned() {
            BPF_ALIGN_OF_U128
        } else {
            align_of::<u8>()
        };
        let layout = match Layout::from_size_align(size as usize, align) {
            Ok(layout) => layout,
            Err(_) => {
                return Ok(0);
            }
        };
        if free_addr == 0 {
            match allocator.alloc(layout) {
                Ok(addr) => Ok(addr),
                Err(_) => Ok(0),
            }
        } else {
            allocator.dealloc(free_addr, layout);
            Ok(0)
        }
    }
);

fn translate_and_check_program_address_inputs<'a>(
    seeds_addr: u64,
    seeds_len: u64,
    program_id_addr: u64,
    memory_mapping: &mut MemoryMapping,
    check_aligned: bool,
    check_size: bool,
) -> Result<(Vec<&'a [u8]>, &'a Pubkey), EbpfError> {
    let untranslated_seeds = translate_slice::<&[&u8]>(
        memory_mapping,
        seeds_addr,
        seeds_len,
        check_aligned,
        check_size,
    )?;
    if untranslated_seeds.len() > MAX_SEEDS {
        return Err(SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded).into());
    }
    let seeds = untranslated_seeds
        .iter()
        .map(|untranslated_seed| {
            if untranslated_seed.len() > MAX_SEED_LEN {
                return Err(SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded).into());
            }
            translate_slice::<u8>(
                memory_mapping,
                untranslated_seed.as_ptr() as *const _ as u64,
                untranslated_seed.len() as u64,
                check_aligned,
                check_size,
            )
        })
        .collect::<Result<Vec<_>, EbpfError>>()?;
    let program_id = translate_type::<Pubkey>(memory_mapping, program_id_addr, check_aligned)?;
    Ok((seeds, program_id))
}

declare_syscall!(
    /// Create a program address
    SyscallCreateProgramAddress,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        seeds_addr: u64,
        seeds_len: u64,
        program_id_addr: u64,
        address_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let cost = invoke_context
            .get_compute_budget()
            .create_program_address_units;
        consume_compute_meter(invoke_context, cost)?;

        let (seeds, program_id) = translate_and_check_program_address_inputs(
            seeds_addr,
            seeds_len,
            program_id_addr,
            memory_mapping,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let new_address = match Pubkey::create_program_address(&seeds, program_id) {
            Ok(address) => address,
            Err(_) => {
                return Ok(1);
            }
        };
        let address = translate_slice_mut::<u8>(
            memory_mapping,
            address_addr,
            32,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        address.copy_from_slice(new_address.as_ref());
        Ok(0)
    }
);

declare_syscall!(
    /// Create a program address
    SyscallTryFindProgramAddress,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        seeds_addr: u64,
        seeds_len: u64,
        program_id_addr: u64,
        address_addr: u64,
        bump_seed_addr: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let cost = invoke_context
            .get_compute_budget()
            .create_program_address_units;
        consume_compute_meter(invoke_context, cost)?;

        let (seeds, program_id) = translate_and_check_program_address_inputs(
            seeds_addr,
            seeds_len,
            program_id_addr,
            memory_mapping,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let mut bump_seed = [std::u8::MAX];
        for _ in 0..std::u8::MAX {
            {
                let mut seeds_with_bump = seeds.to_vec();
                seeds_with_bump.push(&bump_seed);

                if let Ok(new_address) =
                    Pubkey::create_program_address(&seeds_with_bump, program_id)
                {
                    let bump_seed_ref = translate_type_mut::<u8>(
                        memory_mapping,
                        bump_seed_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let address = translate_slice_mut::<u8>(
                        memory_mapping,
                        address_addr,
                        std::mem::size_of::<Pubkey>() as u64,
                        invoke_context.get_check_aligned(),
                        invoke_context.get_check_size(),
                    )?;
                    if !is_nonoverlapping(
                        bump_seed_ref as *const _ as usize,
                        std::mem::size_of_val(bump_seed_ref),
                        address.as_ptr() as usize,
                        std::mem::size_of::<Pubkey>(),
                    ) && invoke_context
                        .feature_set
                        .is_active(&check_syscall_outputs_do_not_overlap::id())
                    {
                        return Err(SyscallError::CopyOverlapping.into());
                    }
                    *bump_seed_ref = bump_seed[0];
                    address.copy_from_slice(new_address.as_ref());
                    return Ok(0);
                }
            }
            bump_seed[0] = bump_seed[0].saturating_sub(1);
            consume_compute_meter(invoke_context, cost)?;
        }
        Ok(1)
    }
);

declare_syscall!(
    /// SHA256
    SyscallSha256,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        vals_addr: u64,
        vals_len: u64,
        result_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let compute_budget = invoke_context.get_compute_budget();
        if compute_budget.sha256_max_slices < vals_len {
            ic_msg!(
                invoke_context,
                "Sha256 hashing {} sequences in one syscall is over the limit {}",
                vals_len,
                compute_budget.sha256_max_slices,
            );
            return Err(SyscallError::TooManySlices.into());
        }

        consume_compute_meter(invoke_context, compute_budget.sha256_base_cost)?;

        let hash_result = translate_slice_mut::<u8>(
            memory_mapping,
            result_addr,
            HASH_BYTES as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let mut hasher = Hasher::default();
        if vals_len > 0 {
            let vals = translate_slice::<&[u8]>(
                memory_mapping,
                vals_addr,
                vals_len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            for val in vals.iter() {
                let bytes = translate_slice::<u8>(
                    memory_mapping,
                    val.as_ptr() as u64,
                    val.len() as u64,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;
                let cost = compute_budget.mem_op_base_cost.max(
                    compute_budget
                        .sha256_byte_cost
                        .saturating_mul((val.len() as u64).saturating_div(2)),
                );
                consume_compute_meter(invoke_context, cost)?;
                hasher.hash(bytes);
            }
        }
        hash_result.copy_from_slice(&hasher.result().to_bytes());
        Ok(0)
    }
);

declare_syscall!(
    // Keccak256
    SyscallKeccak256,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        vals_addr: u64,
        vals_len: u64,
        result_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let compute_budget = invoke_context.get_compute_budget();
        if compute_budget.sha256_max_slices < vals_len {
            ic_msg!(
                invoke_context,
                "Keccak256 hashing {} sequences in one syscall is over the limit {}",
                vals_len,
                compute_budget.sha256_max_slices,
            );
            return Err(SyscallError::TooManySlices.into());
        }

        consume_compute_meter(invoke_context, compute_budget.sha256_base_cost)?;

        let hash_result = translate_slice_mut::<u8>(
            memory_mapping,
            result_addr,
            keccak::HASH_BYTES as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let mut hasher = keccak::Hasher::default();
        if vals_len > 0 {
            let vals = translate_slice::<&[u8]>(
                memory_mapping,
                vals_addr,
                vals_len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            for val in vals.iter() {
                let bytes = translate_slice::<u8>(
                    memory_mapping,
                    val.as_ptr() as u64,
                    val.len() as u64,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;
                let cost = compute_budget.mem_op_base_cost.max(
                    compute_budget
                        .sha256_byte_cost
                        .saturating_mul((val.len() as u64).saturating_div(2)),
                );
                consume_compute_meter(invoke_context, cost)?;
                hasher.hash(bytes);
            }
        }
        hash_result.copy_from_slice(&hasher.result().to_bytes());
        Ok(0)
    }
);

declare_syscall!(
    /// secp256k1_recover
    SyscallSecp256k1Recover,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        hash_addr: u64,
        recovery_id_val: u64,
        signature_addr: u64,
        result_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let cost = invoke_context.get_compute_budget().secp256k1_recover_cost;
        consume_compute_meter(invoke_context, cost)?;

        let hash = translate_slice::<u8>(
            memory_mapping,
            hash_addr,
            keccak::HASH_BYTES as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let signature = translate_slice::<u8>(
            memory_mapping,
            signature_addr,
            SECP256K1_SIGNATURE_LENGTH as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let secp256k1_recover_result = translate_slice_mut::<u8>(
            memory_mapping,
            result_addr,
            SECP256K1_PUBLIC_KEY_LENGTH as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let message = match libsecp256k1::Message::parse_slice(hash) {
            Ok(msg) => msg,
            Err(_) => {
                return Ok(Secp256k1RecoverError::InvalidHash.into());
            }
        };
        let adjusted_recover_id_val = if invoke_context
            .feature_set
            .is_active(&limit_secp256k1_recovery_id::id())
        {
            match recovery_id_val.try_into() {
                Ok(adjusted_recover_id_val) => adjusted_recover_id_val,
                Err(_) => {
                    return Ok(Secp256k1RecoverError::InvalidRecoveryId.into());
                }
            }
        } else {
            recovery_id_val as u8
        };
        let recovery_id = match libsecp256k1::RecoveryId::parse(adjusted_recover_id_val) {
            Ok(id) => id,
            Err(_) => {
                return Ok(Secp256k1RecoverError::InvalidRecoveryId.into());
            }
        };
        let sig_parse_result = if invoke_context
            .feature_set
            .is_active(&libsecp256k1_0_5_upgrade_enabled::id())
        {
            libsecp256k1::Signature::parse_standard_slice(signature)
        } else {
            libsecp256k1::Signature::parse_overflowing_slice(signature)
        };

        let signature = match sig_parse_result {
            Ok(sig) => sig,
            Err(_) => {
                return Ok(Secp256k1RecoverError::InvalidSignature.into());
            }
        };

        let public_key = match libsecp256k1::recover(&message, &signature, &recovery_id) {
            Ok(key) => key.serialize(),
            Err(_) => {
                return Ok(Secp256k1RecoverError::InvalidSignature.into());
            }
        };

        secp256k1_recover_result.copy_from_slice(&public_key[1..65]);
        Ok(SUCCESS)
    }
);

declare_syscall!(
    // Elliptic Curve Point Validation
    //
    // Currently, only curve25519 Edwards and Ristretto representations are supported
    SyscallCurvePointValidation,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        curve_id: u64,
        point_addr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        use solana_zk_token_sdk::curve25519::{curve_syscall_traits::*, edwards, ristretto};
        match curve_id {
            CURVE25519_EDWARDS => {
                let cost = invoke_context
                    .get_compute_budget()
                    .curve25519_edwards_validate_point_cost;
                consume_compute_meter(invoke_context, cost)?;

                let point = translate_type::<edwards::PodEdwardsPoint>(
                    memory_mapping,
                    point_addr,
                    invoke_context.get_check_aligned(),
                )?;

                if edwards::validate_edwards(point) {
                    Ok(0)
                } else {
                    Ok(1)
                }
            }
            CURVE25519_RISTRETTO => {
                let cost = invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_validate_point_cost;
                consume_compute_meter(invoke_context, cost)?;

                let point = translate_type::<ristretto::PodRistrettoPoint>(
                    memory_mapping,
                    point_addr,
                    invoke_context.get_check_aligned(),
                )?;

                if ristretto::validate_ristretto(point) {
                    Ok(0)
                } else {
                    Ok(1)
                }
            }
            _ => Ok(1),
        }
    }
);

declare_syscall!(
    // Elliptic Curve Group Operations
    //
    // Currently, only curve25519 Edwards and Ristretto representations are supported
    SyscallCurveGroupOps,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        curve_id: u64,
        group_op: u64,
        left_input_addr: u64,
        right_input_addr: u64,
        result_point_addr: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        use solana_zk_token_sdk::curve25519::{
            curve_syscall_traits::*, edwards, ristretto, scalar,
        };
        match curve_id {
            CURVE25519_EDWARDS => match group_op {
                ADD => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_edwards_add_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let left_point = translate_type::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let right_point = translate_type::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) = edwards::add_edwards(left_point, right_point) {
                        *translate_type_mut::<edwards::PodEdwardsPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                SUB => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_edwards_subtract_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let left_point = translate_type::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let right_point = translate_type::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) = edwards::subtract_edwards(left_point, right_point) {
                        *translate_type_mut::<edwards::PodEdwardsPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                MUL => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_edwards_multiply_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let scalar = translate_type::<scalar::PodScalar>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let input_point = translate_type::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) = edwards::multiply_edwards(scalar, input_point) {
                        *translate_type_mut::<edwards::PodEdwardsPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                _ => Ok(1),
            },

            CURVE25519_RISTRETTO => match group_op {
                ADD => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_ristretto_add_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let left_point = translate_type::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let right_point = translate_type::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) = ristretto::add_ristretto(left_point, right_point) {
                        *translate_type_mut::<ristretto::PodRistrettoPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                SUB => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_ristretto_subtract_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let left_point = translate_type::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let right_point = translate_type::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) =
                        ristretto::subtract_ristretto(left_point, right_point)
                    {
                        *translate_type_mut::<ristretto::PodRistrettoPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                MUL => {
                    let cost = invoke_context
                        .get_compute_budget()
                        .curve25519_ristretto_multiply_cost;
                    consume_compute_meter(invoke_context, cost)?;

                    let scalar = translate_type::<scalar::PodScalar>(
                        memory_mapping,
                        left_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;
                    let input_point = translate_type::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        right_input_addr,
                        invoke_context.get_check_aligned(),
                    )?;

                    if let Some(result_point) = ristretto::multiply_ristretto(scalar, input_point) {
                        *translate_type_mut::<ristretto::PodRistrettoPoint>(
                            memory_mapping,
                            result_point_addr,
                            invoke_context.get_check_aligned(),
                        )? = result_point;
                        Ok(0)
                    } else {
                        Ok(1)
                    }
                }
                _ => Ok(1),
            },

            _ => Ok(1),
        }
    }
);

declare_syscall!(
    // Elliptic Curve Multiscalar Multiplication
    //
    // Currently, only curve25519 Edwards and Ristretto representations are supported
    SyscallCurveMultiscalarMultiplication,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        curve_id: u64,
        scalars_addr: u64,
        points_addr: u64,
        points_len: u64,
        result_point_addr: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        use solana_zk_token_sdk::curve25519::{
            curve_syscall_traits::*, edwards, ristretto, scalar,
        };
        match curve_id {
            CURVE25519_EDWARDS => {
                let cost = invoke_context
                    .get_compute_budget()
                    .curve25519_edwards_msm_base_cost
                    .saturating_add(
                        invoke_context
                            .get_compute_budget()
                            .curve25519_edwards_msm_incremental_cost
                            .saturating_mul(points_len.saturating_sub(1)),
                    );
                consume_compute_meter(invoke_context, cost)?;

                let scalars = translate_slice::<scalar::PodScalar>(
                    memory_mapping,
                    scalars_addr,
                    points_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;

                let points = translate_slice::<edwards::PodEdwardsPoint>(
                    memory_mapping,
                    points_addr,
                    points_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;

                if let Some(result_point) = edwards::multiscalar_multiply_edwards(scalars, points) {
                    *translate_type_mut::<edwards::PodEdwardsPoint>(
                        memory_mapping,
                        result_point_addr,
                        invoke_context.get_check_aligned(),
                    )? = result_point;
                    Ok(0)
                } else {
                    Ok(1)
                }
            }

            CURVE25519_RISTRETTO => {
                let cost = invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_msm_base_cost
                    .saturating_add(
                        invoke_context
                            .get_compute_budget()
                            .curve25519_ristretto_msm_incremental_cost
                            .saturating_mul(points_len.saturating_sub(1)),
                    );
                consume_compute_meter(invoke_context, cost)?;

                let scalars = translate_slice::<scalar::PodScalar>(
                    memory_mapping,
                    scalars_addr,
                    points_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;

                let points = translate_slice::<ristretto::PodRistrettoPoint>(
                    memory_mapping,
                    points_addr,
                    points_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;

                if let Some(result_point) =
                    ristretto::multiscalar_multiply_ristretto(scalars, points)
                {
                    *translate_type_mut::<ristretto::PodRistrettoPoint>(
                        memory_mapping,
                        result_point_addr,
                        invoke_context.get_check_aligned(),
                    )? = result_point;
                    Ok(0)
                } else {
                    Ok(1)
                }
            }

            _ => Ok(1),
        }
    }
);

declare_syscall!(
    // Blake3
    SyscallBlake3,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        vals_addr: u64,
        vals_len: u64,
        result_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let compute_budget = invoke_context.get_compute_budget();
        if compute_budget.sha256_max_slices < vals_len {
            ic_msg!(
                invoke_context,
                "Blake3 hashing {} sequences in one syscall is over the limit {}",
                vals_len,
                compute_budget.sha256_max_slices,
            );
            return Err(SyscallError::TooManySlices.into());
        }

        consume_compute_meter(invoke_context, compute_budget.sha256_base_cost)?;

        let hash_result = translate_slice_mut::<u8>(
            memory_mapping,
            result_addr,
            blake3::HASH_BYTES as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let mut hasher = blake3::Hasher::default();
        if vals_len > 0 {
            let vals = translate_slice::<&[u8]>(
                memory_mapping,
                vals_addr,
                vals_len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            for val in vals.iter() {
                let bytes = translate_slice::<u8>(
                    memory_mapping,
                    val.as_ptr() as u64,
                    val.len() as u64,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;
                let cost = compute_budget.mem_op_base_cost.max(
                    compute_budget
                        .sha256_byte_cost
                        .saturating_mul((val.len() as u64).saturating_div(2)),
                );
                consume_compute_meter(invoke_context, cost)?;
                hasher.hash(bytes);
            }
        }
        hash_result.copy_from_slice(&hasher.result().to_bytes());
        Ok(0)
    }
);

declare_syscall!(
    /// Set return data
    SyscallSetReturnData,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let budget = invoke_context.get_compute_budget();

        let cost = len
            .saturating_div(budget.cpi_bytes_per_unit)
            .saturating_add(budget.syscall_base_cost);
        consume_compute_meter(invoke_context, cost)?;

        if len > MAX_RETURN_DATA as u64 {
            return Err(SyscallError::ReturnDataTooLarge(len, MAX_RETURN_DATA as u64).into());
        }

        let return_data = if len == 0 {
            Vec::new()
        } else {
            translate_slice::<u8>(
                memory_mapping,
                addr,
                len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?
            .to_vec()
        };
        let transaction_context = &mut invoke_context.transaction_context;
        let program_id = *transaction_context
            .get_current_instruction_context()
            .and_then(|instruction_context| {
                instruction_context.get_last_program_key(transaction_context)
            })
            .map_err(SyscallError::InstructionError)?;

        transaction_context
            .set_return_data(program_id, return_data)
            .map_err(SyscallError::InstructionError)?;

        Ok(0)
    }
);

declare_syscall!(
    /// Get return data
    SyscallGetReturnData,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        return_data_addr: u64,
        mut length: u64,
        program_id_addr: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let budget = invoke_context.get_compute_budget();

        consume_compute_meter(invoke_context, budget.syscall_base_cost)?;

        let (program_id, return_data) = invoke_context.transaction_context.get_return_data();
        length = length.min(return_data.len() as u64);
        if length != 0 {
            let cost = length
                .saturating_add(size_of::<Pubkey>() as u64)
                .saturating_div(budget.cpi_bytes_per_unit);
            consume_compute_meter(invoke_context, cost)?;

            let return_data_result = translate_slice_mut::<u8>(
                memory_mapping,
                return_data_addr,
                length,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;

            let to_slice = return_data_result;
            let from_slice = return_data
                .get(..length as usize)
                .ok_or(SyscallError::InvokeContextBorrowFailed)?;
            if to_slice.len() != from_slice.len() {
                return Err(SyscallError::InvalidLength.into());
            }
            to_slice.copy_from_slice(from_slice);

            let program_id_result = translate_type_mut::<Pubkey>(
                memory_mapping,
                program_id_addr,
                invoke_context.get_check_aligned(),
            )?;

            if !is_nonoverlapping(
                to_slice.as_ptr() as usize,
                length as usize,
                program_id_result as *const _ as usize,
                std::mem::size_of::<Pubkey>(),
            ) && invoke_context
                .feature_set
                .is_active(&check_syscall_outputs_do_not_overlap::id())
            {
                return Err(SyscallError::CopyOverlapping.into());
            }

            *program_id_result = *program_id;
        }

        // Return the actual length, rather the length returned
        Ok(return_data.len() as u64)
    }
);

declare_syscall!(
    /// Get a processed sigling instruction
    SyscallGetProcessedSiblingInstruction,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        index: u64,
        meta_addr: u64,
        program_id_addr: u64,
        data_addr: u64,
        accounts_addr: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let budget = invoke_context.get_compute_budget();

        consume_compute_meter(invoke_context, budget.syscall_base_cost)?;
        let stop_sibling_instruction_search_at_parent = invoke_context
            .feature_set
            .is_active(&stop_sibling_instruction_search_at_parent::id());

        // Reverse iterate through the instruction trace,
        // ignoring anything except instructions on the same level
        let stack_height = invoke_context.get_stack_height();
        let instruction_trace_length = invoke_context
            .transaction_context
            .get_instruction_trace_length();
        let mut reverse_index_at_stack_height = 0;
        let mut found_instruction_context = None;
        for index_in_trace in (0..instruction_trace_length).rev() {
            let instruction_context = invoke_context
                .transaction_context
                .get_instruction_context_at_index_in_trace(index_in_trace)
                .map_err(SyscallError::InstructionError)?;
            if (stop_sibling_instruction_search_at_parent
                || instruction_context.get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT)
                && instruction_context.get_stack_height() < stack_height
            {
                break;
            }
            if instruction_context.get_stack_height() == stack_height {
                if index.saturating_add(1) == reverse_index_at_stack_height {
                    found_instruction_context = Some(instruction_context);
                    break;
                }
                reverse_index_at_stack_height = reverse_index_at_stack_height.saturating_add(1);
            }
        }

        if let Some(instruction_context) = found_instruction_context {
            let result_header = translate_type_mut::<ProcessedSiblingInstruction>(
                memory_mapping,
                meta_addr,
                invoke_context.get_check_aligned(),
            )?;

            if result_header.data_len == (instruction_context.get_instruction_data().len() as u64)
                && result_header.accounts_len
                    == (instruction_context.get_number_of_instruction_accounts() as u64)
            {
                let program_id = translate_type_mut::<Pubkey>(
                    memory_mapping,
                    program_id_addr,
                    invoke_context.get_check_aligned(),
                )?;
                let data = translate_slice_mut::<u8>(
                    memory_mapping,
                    data_addr,
                    result_header.data_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;
                let accounts = translate_slice_mut::<AccountMeta>(
                    memory_mapping,
                    accounts_addr,
                    result_header.accounts_len,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;

                if (!is_nonoverlapping(
                    result_header as *const _ as usize,
                    std::mem::size_of::<ProcessedSiblingInstruction>(),
                    program_id as *const _ as usize,
                    std::mem::size_of::<Pubkey>(),
                ) || !is_nonoverlapping(
                    result_header as *const _ as usize,
                    std::mem::size_of::<ProcessedSiblingInstruction>(),
                    accounts.as_ptr() as usize,
                    std::mem::size_of::<AccountMeta>()
                        .saturating_mul(result_header.accounts_len as usize),
                ) || !is_nonoverlapping(
                    result_header as *const _ as usize,
                    std::mem::size_of::<ProcessedSiblingInstruction>(),
                    data.as_ptr() as usize,
                    result_header.data_len as usize,
                ) || !is_nonoverlapping(
                    program_id as *const _ as usize,
                    std::mem::size_of::<Pubkey>(),
                    data.as_ptr() as usize,
                    result_header.data_len as usize,
                ) || !is_nonoverlapping(
                    program_id as *const _ as usize,
                    std::mem::size_of::<Pubkey>(),
                    accounts.as_ptr() as usize,
                    std::mem::size_of::<AccountMeta>()
                        .saturating_mul(result_header.accounts_len as usize),
                ) || !is_nonoverlapping(
                    data.as_ptr() as usize,
                    result_header.data_len as usize,
                    accounts.as_ptr() as usize,
                    std::mem::size_of::<AccountMeta>()
                        .saturating_mul(result_header.accounts_len as usize),
                )) && invoke_context
                    .feature_set
                    .is_active(&check_syscall_outputs_do_not_overlap::id())
                {
                    return Err(SyscallError::CopyOverlapping.into());
                }

                *program_id = *instruction_context
                    .get_last_program_key(invoke_context.transaction_context)
                    .map_err(SyscallError::InstructionError)?;
                data.clone_from_slice(instruction_context.get_instruction_data());
                let account_metas = (0..instruction_context.get_number_of_instruction_accounts())
                    .map(|instruction_account_index| {
                        Ok(AccountMeta {
                            pubkey: *invoke_context
                                .transaction_context
                                .get_key_of_account_at_index(
                                    instruction_context
                                        .get_index_of_instruction_account_in_transaction(
                                            instruction_account_index,
                                        )?,
                                )?,
                            is_signer: instruction_context
                                .is_instruction_account_signer(instruction_account_index)?,
                            is_writable: instruction_context
                                .is_instruction_account_writable(instruction_account_index)?,
                        })
                    })
                    .collect::<Result<Vec<_>, InstructionError>>()
                    .map_err(SyscallError::InstructionError)?;
                accounts.clone_from_slice(account_metas.as_slice());
            }
            result_header.data_len = instruction_context.get_instruction_data().len() as u64;
            result_header.accounts_len =
                instruction_context.get_number_of_instruction_accounts() as u64;
            return Ok(true as u64);
        }
        Ok(false as u64)
    }
);

declare_syscall!(
    /// Get current call stack height
    SyscallGetStackHeight,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        let budget = invoke_context.get_compute_budget();

        consume_compute_meter(invoke_context, budget.syscall_base_cost)?;

        Ok(invoke_context.get_stack_height() as u64)
    }
);

declare_syscall!(
    /// alt_bn128 group operations
    SyscallAltBn128,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        group_op: u64,
        input_addr: u64,
        input_size: u64,
        result_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        use solana_sdk::alt_bn128::prelude::{ALT_BN128_ADD, ALT_BN128_MUL, ALT_BN128_PAIRING};
        let budget = invoke_context.get_compute_budget();
        let (cost, output): (u64, usize) = match group_op {
            ALT_BN128_ADD => (
                budget.alt_bn128_addition_cost,
                ALT_BN128_ADDITION_OUTPUT_LEN,
            ),
            ALT_BN128_MUL => (
                budget.alt_bn128_multiplication_cost,
                ALT_BN128_MULTIPLICATION_OUTPUT_LEN,
            ),
            ALT_BN128_PAIRING => {
                let ele_len = input_size.saturating_div(ALT_BN128_PAIRING_ELEMENT_LEN as u64);
                let cost = budget
                    .alt_bn128_pairing_one_pair_cost_first
                    .saturating_add(
                        budget
                            .alt_bn128_pairing_one_pair_cost_other
                            .saturating_mul(ele_len.saturating_sub(1)),
                    )
                    .saturating_add(budget.sha256_base_cost)
                    .saturating_add(input_size)
                    .saturating_add(ALT_BN128_PAIRING_OUTPUT_LEN as u64);
                (cost, ALT_BN128_PAIRING_OUTPUT_LEN)
            }
            _ => {
                return Err(SyscallError::InvalidAttribute.into());
            }
        };

        consume_compute_meter(invoke_context, cost)?;

        let input = translate_slice::<u8>(
            memory_mapping,
            input_addr,
            input_size,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let call_result = translate_slice_mut::<u8>(
            memory_mapping,
            result_addr,
            output as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let calculation = match group_op {
            ALT_BN128_ADD => alt_bn128_addition,
            ALT_BN128_MUL => alt_bn128_multiplication,
            ALT_BN128_PAIRING => alt_bn128_pairing,
            _ => {
                return Err(SyscallError::InvalidAttribute.into());
            }
        };

        let result_point = match calculation(input) {
            Ok(result_point) => result_point,
            Err(e) => {
                return Ok(e.into());
            }
        };

        if result_point.len() != output {
            return Ok(AltBn128Error::SliceOutOfBounds.into());
        }

        call_result.copy_from_slice(&result_point);
        Ok(SUCCESS)
    }
);

#[cfg(test)]
mod tests {
    #[allow(deprecated)]
    use solana_sdk::sysvar::fees::Fees;
    use {
        super::*,
        crate::BpfAllocator,
        solana_program_runtime::{invoke_context::InvokeContext, sysvar_cache::SysvarCache},
        solana_rbpf::{
            aligned_memory::AlignedMemory,
            ebpf::{self, HOST_ALIGN},
            memory_region::MemoryRegion,
            vm::{BuiltInFunction, Config},
        },
        solana_sdk::{
            account::AccountSharedData,
            bpf_loader,
            fee_calculator::FeeCalculator,
            hash::hashv,
            program::check_type_assumptions,
            sysvar::{clock::Clock, epoch_schedule::EpochSchedule, rent::Rent},
            transaction_context::TransactionContext,
        },
        std::{borrow::Cow, cell::RefCell, rc::Rc, str::FromStr},
    };

    macro_rules! assert_access_violation {
        ($result:expr, $va:expr, $len:expr) => {
            match $result {
                ProgramResult::Err(EbpfError::AccessViolation(_, _, va, len, _))
                    if $va == va && $len == len => {}
                ProgramResult::Err(EbpfError::StackAccessViolation(_, _, va, len, _))
                    if $va == va && $len == len => {}
                _ => panic!(),
            }
        };
    }

    macro_rules! prepare_mockup {
        ($invoke_context:ident,
         $transaction_context:ident,
         $program_key:ident,
         $loader_key:expr $(,)?) => {
            let $program_key = Pubkey::new_unique();
            let transaction_accounts = vec![
                (
                    $loader_key,
                    AccountSharedData::new(0, 0, &native_loader::id()),
                ),
                ($program_key, AccountSharedData::new(0, 0, &$loader_key)),
            ];
            let mut $transaction_context =
                TransactionContext::new(transaction_accounts, Some(Rent::default()), 1, 1);
            let mut $invoke_context = InvokeContext::new_mock(&mut $transaction_context, &[]);
            $invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(&[0, 1], &[], &[]);
            $invoke_context.push().unwrap();
        };
    }

    #[allow(dead_code)]
    struct MockSlice {
        pub vm_addr: u64,
        pub len: usize,
    }

    #[test]
    fn test_translate() {
        const START: u64 = 0x100000000;
        const LENGTH: u64 = 1000;
        let data = vec![0u8; LENGTH as usize];
        let addr = data.as_ptr() as u64;
        let config = Config::default();
        let memory_mapping =
            MemoryMapping::new(vec![MemoryRegion::new_readonly(&data, START)], &config).unwrap();

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
                    translate(&memory_mapping, AccessType::Load, start, length).unwrap(),
                    value
                )
            } else {
                assert!(translate(&memory_mapping, AccessType::Load, start, length).is_err())
            }
        }
    }

    #[test]
    fn test_translate_type() {
        // Pubkey
        let pubkey = solana_sdk::pubkey::new_rand();
        let addr = &pubkey as *const _ as u64;
        let config = Config::default();
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: std::mem::size_of::<Pubkey>() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        let translated_pubkey =
            translate_type::<Pubkey>(&memory_mapping, 0x100000000, true).unwrap();
        assert_eq!(pubkey, *translated_pubkey);

        // Instruction
        let instruction = Instruction::new_with_bincode(
            solana_sdk::pubkey::new_rand(),
            &"foobar",
            vec![AccountMeta::new(solana_sdk::pubkey::new_rand(), false)],
        );
        let addr = &instruction as *const _ as u64;
        let mut memory_region = MemoryRegion {
            host_addr: addr,
            vm_addr: 0x100000000,
            len: std::mem::size_of::<Instruction>() as u64,
            vm_gap_shift: 63,
            is_writable: false,
        };
        let mut memory_mapping = MemoryMapping::new(vec![memory_region.clone()], &config).unwrap();
        let translated_instruction =
            translate_type::<Instruction>(&memory_mapping, 0x100000000, true).unwrap();
        assert_eq!(instruction, *translated_instruction);
        memory_region.len = 1;
        let memory_region_index = memory_mapping
            .get_regions()
            .iter()
            .position(|memory_region| memory_region.vm_addr == 0x100000000)
            .unwrap();
        memory_mapping
            .replace_region(memory_region_index, memory_region)
            .unwrap();
        assert!(translate_type::<Instruction>(&memory_mapping, 0x100000000, true).is_err());
    }

    #[test]
    fn test_translate_slice() {
        // zero len
        let good_data = vec![1u8, 2, 3, 4, 5];
        let data: Vec<u8> = vec![];
        assert_eq!(0x1 as *const u8, data.as_ptr());
        let addr = good_data.as_ptr() as *const _ as u64;
        let config = Config::default();
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: good_data.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        let translated_data =
            translate_slice::<u8>(&memory_mapping, data.as_ptr() as u64, 0, true, true).unwrap();
        assert_eq!(data, translated_data);
        assert_eq!(0, translated_data.len());

        // u8
        let mut data = vec![1u8, 2, 3, 4, 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: data.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        let translated_data =
            translate_slice::<u8>(&memory_mapping, 0x100000000, data.len() as u64, true, true)
                .unwrap();
        assert_eq!(data, translated_data);
        *data.first_mut().unwrap() = 10;
        assert_eq!(data, translated_data);
        assert!(
            translate_slice::<u8>(&memory_mapping, data.as_ptr() as u64, u64::MAX, true, true)
                .is_err()
        );

        assert!(translate_slice::<u8>(
            &memory_mapping,
            0x100000000 - 1,
            data.len() as u64,
            true,
            true
        )
        .is_err());

        // u64
        let mut data = vec![1u64, 2, 3, 4, 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: (data.len() * size_of::<u64>()) as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        let translated_data =
            translate_slice::<u64>(&memory_mapping, 0x100000000, data.len() as u64, true, true)
                .unwrap();
        assert_eq!(data, translated_data);
        *data.first_mut().unwrap() = 10;
        assert_eq!(data, translated_data);
        assert!(
            translate_slice::<u64>(&memory_mapping, 0x100000000, u64::MAX, true, true).is_err()
        );

        // Pubkeys
        let mut data = vec![solana_sdk::pubkey::new_rand(); 5];
        let addr = data.as_ptr() as *const _ as u64;
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: (data.len() * std::mem::size_of::<Pubkey>()) as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        let translated_data =
            translate_slice::<Pubkey>(&memory_mapping, 0x100000000, data.len() as u64, true, true)
                .unwrap();
        assert_eq!(data, translated_data);
        *data.first_mut().unwrap() = solana_sdk::pubkey::new_rand(); // Both should point to same place
        assert_eq!(data, translated_data);
    }

    #[test]
    fn test_translate_string_and_do() {
        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let config = Config::default();
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();
        assert_eq!(
            42,
            translate_string_and_do(
                &memory_mapping,
                0x100000000,
                string.len() as u64,
                true,
                true,
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
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(vec![], &config).unwrap();
        let mut result = ProgramResult::Ok(0);
        SyscallAbort::call(
            &mut invoke_context,
            0,
            0,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();
    }

    #[test]
    #[should_panic(expected = "UserError(SyscallError(Panic(\"Gaggablaghblagh!\", 42, 84)))")]
    fn test_syscall_sol_panic() {
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(string.len() as u64 - 1);
        let mut result = ProgramResult::Ok(0);
        SyscallPanic::call(
            &mut invoke_context,
            0x100000000,
            string.len() as u64,
            42,
            84,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));

        invoke_context.mock_set_remaining(string.len() as u64);
        let mut result = ProgramResult::Ok(0);
        SyscallPanic::call(
            &mut invoke_context,
            0x100000000,
            string.len() as u64,
            42,
            84,
            0,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();
    }

    #[test]
    fn test_syscall_sol_log() {
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let string = "Gaggablaghblagh!";
        let addr = string.as_ptr() as *const _ as u64;
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: string.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(400 - 1);
        let mut result = ProgramResult::Ok(0);
        SyscallLog::call(
            &mut invoke_context,
            0x100000001, // AccessViolation
            string.len() as u64,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 0x100000001, string.len() as u64);
        let mut result = ProgramResult::Ok(0);
        SyscallLog::call(
            &mut invoke_context,
            0x100000000,
            string.len() as u64 * 2, // AccessViolation
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 0x100000000, string.len() as u64 * 2);

        let mut result = ProgramResult::Ok(0);
        SyscallLog::call(
            &mut invoke_context,
            0x100000000,
            string.len() as u64,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();
        let mut result = ProgramResult::Ok(0);
        SyscallLog::call(
            &mut invoke_context,
            0x100000000,
            string.len() as u64,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));

        assert_eq!(
            invoke_context
                .get_log_collector()
                .unwrap()
                .borrow()
                .get_recorded_content(),
            &["Program log: Gaggablaghblagh!".to_string()]
        );
    }

    #[test]
    fn test_syscall_sol_log_u64() {
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        let cost = invoke_context.get_compute_budget().log_64_units;

        invoke_context.mock_set_remaining(cost);
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(vec![], &config).unwrap();
        let mut result = ProgramResult::Ok(0);
        SyscallLogU64::call(
            &mut invoke_context,
            1,
            2,
            3,
            4,
            5,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();

        assert_eq!(
            invoke_context
                .get_log_collector()
                .unwrap()
                .borrow()
                .get_recorded_content(),
            &["Program log: 0x1, 0x2, 0x3, 0x4, 0x5".to_string()]
        );
    }

    #[test]
    fn test_syscall_sol_pubkey() {
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        let cost = invoke_context.get_compute_budget().log_pubkey_units;

        let pubkey = Pubkey::from_str("MoqiU1vryuCGQSxFKA1SZ316JdLEFFhoAu6cKUNk7dN").unwrap();
        let addr = pubkey.as_ref().first().unwrap() as *const _ as u64;
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: addr,
                vm_addr: 0x100000000,
                len: 32,
                vm_gap_shift: 63,
                is_writable: false,
            }],
            &config,
        )
        .unwrap();

        let mut result = ProgramResult::Ok(0);
        SyscallLogPubkey::call(
            &mut invoke_context,
            0x100000001, // AccessViolation
            32,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, 0x100000001, 32);

        invoke_context.mock_set_remaining(1);
        let mut result = ProgramResult::Ok(0);
        SyscallLogPubkey::call(
            &mut invoke_context,
            100,
            32,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));

        invoke_context.mock_set_remaining(cost);
        let mut result = ProgramResult::Ok(0);
        SyscallLogPubkey::call(
            &mut invoke_context,
            0x100000000,
            0,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();

        assert_eq!(
            invoke_context
                .get_log_collector()
                .unwrap()
                .borrow()
                .get_recorded_content(),
            &["Program log: MoqiU1vryuCGQSxFKA1SZ316JdLEFFhoAu6cKUNk7dN".to_string()]
        );
    }

    #[test]
    fn test_syscall_sol_alloc_free() {
        let config = Config::default();

        // large alloc
        {
            prepare_mockup!(
                invoke_context,
                transaction_context,
                program_id,
                bpf_loader::id(),
            );
            let mut heap = AlignedMemory::<HOST_ALIGN>::zero_filled(100);
            let mut memory_mapping = MemoryMapping::new(
                vec![
                    MemoryRegion::new_readonly(&[], ebpf::MM_PROGRAM_START),
                    MemoryRegion::new_writable_gapped(&mut [], ebpf::MM_STACK_START, 4096),
                    MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
                    MemoryRegion::new_writable(&mut [], ebpf::MM_INPUT_START),
                ],
                &config,
            )
            .unwrap();
            invoke_context
                .set_syscall_context(
                    true,
                    true,
                    vec![],
                    Rc::new(RefCell::new(BpfAllocator::new(heap, ebpf::MM_HEAP_START))),
                )
                .unwrap();
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                100,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            assert_ne!(result.unwrap(), 0);
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                100,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            assert_eq!(result.unwrap(), 0);
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                u64::MAX,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            assert_eq!(result.unwrap(), 0);
        }

        // many small unaligned allocs
        {
            prepare_mockup!(
                invoke_context,
                transaction_context,
                program_id,
                bpf_loader::id(),
            );
            let mut heap = AlignedMemory::<HOST_ALIGN>::zero_filled(100);
            let mut memory_mapping = MemoryMapping::new(
                vec![
                    MemoryRegion::new_readonly(&[], ebpf::MM_PROGRAM_START),
                    MemoryRegion::new_writable_gapped(&mut [], ebpf::MM_STACK_START, 4096),
                    MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
                    MemoryRegion::new_writable(&mut [], ebpf::MM_INPUT_START),
                ],
                &config,
            )
            .unwrap();
            invoke_context
                .set_syscall_context(
                    false,
                    true,
                    vec![],
                    Rc::new(RefCell::new(BpfAllocator::new(heap, ebpf::MM_HEAP_START))),
                )
                .unwrap();
            for _ in 0..100 {
                let mut result = ProgramResult::Ok(0);
                SyscallAllocFree::call(
                    &mut invoke_context,
                    1,
                    0,
                    0,
                    0,
                    0,
                    &mut memory_mapping,
                    &mut result,
                );
                assert_ne!(result.unwrap(), 0);
            }
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                100,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            assert_eq!(result.unwrap(), 0);
        }

        // many small aligned allocs
        {
            prepare_mockup!(
                invoke_context,
                transaction_context,
                program_id,
                bpf_loader::id(),
            );
            let mut heap = AlignedMemory::<HOST_ALIGN>::zero_filled(100);
            let mut memory_mapping = MemoryMapping::new(
                vec![
                    MemoryRegion::new_readonly(&[], ebpf::MM_PROGRAM_START),
                    MemoryRegion::new_writable_gapped(&mut [], ebpf::MM_STACK_START, 4096),
                    MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
                    MemoryRegion::new_writable(&mut [], ebpf::MM_INPUT_START),
                ],
                &config,
            )
            .unwrap();
            invoke_context
                .set_syscall_context(
                    true,
                    true,
                    vec![],
                    Rc::new(RefCell::new(BpfAllocator::new(heap, ebpf::MM_HEAP_START))),
                )
                .unwrap();
            for _ in 0..12 {
                let mut result = ProgramResult::Ok(0);
                SyscallAllocFree::call(
                    &mut invoke_context,
                    1,
                    0,
                    0,
                    0,
                    0,
                    &mut memory_mapping,
                    &mut result,
                );
                assert_ne!(result.unwrap(), 0);
            }
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                100,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            assert_eq!(result.unwrap(), 0);
        }

        // aligned allocs

        fn aligned<T>() {
            prepare_mockup!(
                invoke_context,
                transaction_context,
                program_id,
                bpf_loader::id(),
            );
            let mut heap = AlignedMemory::<HOST_ALIGN>::zero_filled(100);
            let config = Config::default();
            let mut memory_mapping = MemoryMapping::new(
                vec![
                    MemoryRegion::new_readonly(&[], ebpf::MM_PROGRAM_START),
                    MemoryRegion::new_writable_gapped(&mut [], ebpf::MM_STACK_START, 4096),
                    MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
                    MemoryRegion::new_writable(&mut [], ebpf::MM_INPUT_START),
                ],
                &config,
            )
            .unwrap();
            invoke_context
                .set_syscall_context(
                    true,
                    true,
                    vec![],
                    Rc::new(RefCell::new(BpfAllocator::new(heap, ebpf::MM_HEAP_START))),
                )
                .unwrap();
            let mut result = ProgramResult::Ok(0);
            SyscallAllocFree::call(
                &mut invoke_context,
                size_of::<T>() as u64,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            let address = result.unwrap();
            assert_ne!(address, 0);
            assert_eq!(
                (address as *const u8 as usize).wrapping_rem(align_of::<T>()),
                0
            );
        }
        aligned::<u8>();
        aligned::<u16>();
        aligned::<u32>();
        aligned::<u64>();
        aligned::<u128>();
    }

    #[test]
    fn test_syscall_sha256() {
        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader_deprecated::id(),
        );

        let bytes1 = "Gaggablaghblagh!";
        let bytes2 = "flurbos";

        let mock_slice1 = MockSlice {
            vm_addr: 0x300000000,
            len: bytes1.len(),
        };
        let mock_slice2 = MockSlice {
            vm_addr: 0x400000000,
            len: bytes2.len(),
        };
        let bytes_to_hash = [mock_slice1, mock_slice2];
        let hash_result = [0; HASH_BYTES];
        let ro_len = bytes_to_hash.len() as u64;
        let ro_va = 0x100000000;
        let rw_va = 0x200000000;
        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: bytes_to_hash.as_ptr() as *const _ as u64,
                    vm_addr: ro_va,
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
                MemoryRegion {
                    host_addr: bytes1.as_ptr() as *const _ as u64,
                    vm_addr: bytes_to_hash[0].vm_addr,
                    len: bytes1.len() as u64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: bytes2.as_ptr() as *const _ as u64,
                    vm_addr: bytes_to_hash[1].vm_addr,
                    len: bytes2.len() as u64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            (invoke_context.get_compute_budget().sha256_base_cost
                + invoke_context.get_compute_budget().mem_op_base_cost.max(
                    invoke_context
                        .get_compute_budget()
                        .sha256_byte_cost
                        .saturating_mul((bytes1.len() + bytes2.len()) as u64 / 2),
                ))
                * 4,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallSha256::call(
            &mut invoke_context,
            ro_va,
            ro_len,
            rw_va,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        result.unwrap();

        let hash_local = hashv(&[bytes1.as_ref(), bytes2.as_ref()]).to_bytes();
        assert_eq!(hash_result, hash_local);
        let mut result = ProgramResult::Ok(0);
        SyscallSha256::call(
            &mut invoke_context,
            ro_va - 1, // AccessViolation
            ro_len,
            rw_va,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, ro_va - 1, 32);
        let mut result = ProgramResult::Ok(0);
        SyscallSha256::call(
            &mut invoke_context,
            ro_va,
            ro_len + 1, // AccessViolation
            rw_va,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, ro_va, 48);
        let mut result = ProgramResult::Ok(0);
        SyscallSha256::call(
            &mut invoke_context,
            ro_va,
            ro_len,
            rw_va - 1, // AccessViolation
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_access_violation!(result, rw_va - 1, HASH_BYTES as u64);

        SyscallSha256::call(
            &mut invoke_context,
            ro_va,
            ro_len,
            rw_va,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_syscall_edwards_curve_point_validation() {
        use solana_zk_token_sdk::curve25519::curve_syscall_traits::CURVE25519_EDWARDS;

        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let valid_bytes: [u8; 32] = [
            201, 179, 241, 122, 180, 185, 239, 50, 183, 52, 221, 0, 153, 195, 43, 18, 22, 38, 187,
            206, 179, 192, 210, 58, 53, 45, 150, 98, 89, 17, 158, 11,
        ];
        let valid_bytes_va = 0x100000000;

        let invalid_bytes: [u8; 32] = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];
        let invalid_bytes_va = 0x200000000;

        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: valid_bytes.as_ptr() as *const _ as u64,
                    vm_addr: valid_bytes_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: invalid_bytes.as_ptr() as *const _ as u64,
                    vm_addr: invalid_bytes_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            (invoke_context
                .get_compute_budget()
                .curve25519_edwards_validate_point_cost)
                * 2,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            valid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(0, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            invalid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            valid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_syscall_ristretto_curve_point_validation() {
        use solana_zk_token_sdk::curve25519::curve_syscall_traits::CURVE25519_RISTRETTO;

        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let valid_bytes: [u8; 32] = [
            226, 242, 174, 10, 106, 188, 78, 113, 168, 132, 169, 97, 197, 0, 81, 95, 88, 227, 11,
            106, 165, 130, 221, 141, 182, 166, 89, 69, 224, 141, 45, 118,
        ];
        let valid_bytes_va = 0x100000000;

        let invalid_bytes: [u8; 32] = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];
        let invalid_bytes_va = 0x200000000;

        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: valid_bytes.as_ptr() as *const _ as u64,
                    vm_addr: valid_bytes_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: invalid_bytes.as_ptr() as *const _ as u64,
                    vm_addr: invalid_bytes_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            (invoke_context
                .get_compute_budget()
                .curve25519_ristretto_validate_point_cost)
                * 2,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            valid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(0, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            invalid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurvePointValidation::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            valid_bytes_va,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_syscall_edwards_curve_group_ops() {
        use solana_zk_token_sdk::curve25519::curve_syscall_traits::{
            ADD, CURVE25519_EDWARDS, MUL, SUB,
        };

        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let left_point: [u8; 32] = [
            33, 124, 71, 170, 117, 69, 151, 247, 59, 12, 95, 125, 133, 166, 64, 5, 2, 27, 90, 27,
            200, 167, 59, 164, 52, 54, 52, 200, 29, 13, 34, 213,
        ];
        let left_point_va = 0x100000000;
        let right_point: [u8; 32] = [
            70, 222, 137, 221, 253, 204, 71, 51, 78, 8, 124, 1, 67, 200, 102, 225, 122, 228, 111,
            183, 129, 14, 131, 210, 212, 95, 109, 246, 55, 10, 159, 91,
        ];
        let right_point_va = 0x200000000;
        let scalar: [u8; 32] = [
            254, 198, 23, 138, 67, 243, 184, 110, 236, 115, 236, 205, 205, 215, 79, 114, 45, 250,
            78, 137, 3, 107, 136, 237, 49, 126, 117, 223, 37, 191, 88, 6,
        ];
        let scalar_va = 0x300000000;
        let invalid_point: [u8; 32] = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];
        let invalid_point_va = 0x400000000;
        let result_point: [u8; 32] = [0; 32];
        let result_point_va = 0x500000000;

        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: left_point.as_ptr() as *const _ as u64,
                    vm_addr: left_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: right_point.as_ptr() as *const _ as u64,
                    vm_addr: right_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: scalar.as_ptr() as *const _ as u64,
                    vm_addr: scalar_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: invalid_point.as_ptr() as *const _ as u64,
                    vm_addr: invalid_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: result_point.as_ptr() as *const _ as u64,
                    vm_addr: result_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: true,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            (invoke_context
                .get_compute_budget()
                .curve25519_edwards_add_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_edwards_subtract_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_edwards_multiply_cost)
                * 2,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            ADD,
            left_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_sum = [
            7, 251, 187, 86, 186, 232, 57, 242, 193, 236, 49, 200, 90, 29, 254, 82, 46, 80, 83, 70,
            244, 153, 23, 156, 2, 138, 207, 51, 165, 38, 200, 85,
        ];
        assert_eq!(expected_sum, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            ADD,
            invalid_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            SUB,
            left_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_difference = [
            60, 87, 90, 68, 232, 25, 7, 172, 247, 120, 158, 104, 52, 127, 94, 244, 5, 79, 253, 15,
            48, 69, 82, 134, 155, 70, 188, 81, 108, 95, 212, 9,
        ];
        assert_eq!(expected_difference, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            SUB,
            invalid_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            MUL,
            scalar_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        result.unwrap();
        let expected_product = [
            64, 150, 40, 55, 80, 49, 217, 209, 105, 229, 181, 65, 241, 68, 2, 106, 220, 234, 211,
            71, 159, 76, 156, 114, 242, 68, 147, 31, 243, 211, 191, 124,
        ];
        assert_eq!(expected_product, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            MUL,
            scalar_va,
            invalid_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            MUL,
            scalar_va,
            invalid_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_syscall_ristretto_curve_group_ops() {
        use solana_zk_token_sdk::curve25519::curve_syscall_traits::{
            ADD, CURVE25519_RISTRETTO, MUL, SUB,
        };

        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let left_point: [u8; 32] = [
            208, 165, 125, 204, 2, 100, 218, 17, 170, 194, 23, 9, 102, 156, 134, 136, 217, 190, 98,
            34, 183, 194, 228, 153, 92, 11, 108, 103, 28, 57, 88, 15,
        ];
        let left_point_va = 0x100000000;
        let right_point: [u8; 32] = [
            208, 241, 72, 163, 73, 53, 32, 174, 54, 194, 71, 8, 70, 181, 244, 199, 93, 147, 99,
            231, 162, 127, 25, 40, 39, 19, 140, 132, 112, 212, 145, 108,
        ];
        let right_point_va = 0x200000000;
        let scalar: [u8; 32] = [
            254, 198, 23, 138, 67, 243, 184, 110, 236, 115, 236, 205, 205, 215, 79, 114, 45, 250,
            78, 137, 3, 107, 136, 237, 49, 126, 117, 223, 37, 191, 88, 6,
        ];
        let scalar_va = 0x300000000;
        let invalid_point: [u8; 32] = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];
        let invalid_point_va = 0x400000000;
        let result_point: [u8; 32] = [0; 32];
        let result_point_va = 0x500000000;

        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: left_point.as_ptr() as *const _ as u64,
                    vm_addr: left_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: right_point.as_ptr() as *const _ as u64,
                    vm_addr: right_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: scalar.as_ptr() as *const _ as u64,
                    vm_addr: scalar_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: invalid_point.as_ptr() as *const _ as u64,
                    vm_addr: invalid_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: result_point.as_ptr() as *const _ as u64,
                    vm_addr: result_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: true,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            (invoke_context
                .get_compute_budget()
                .curve25519_ristretto_add_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_subtract_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_multiply_cost)
                * 2,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            ADD,
            left_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_sum = [
            78, 173, 9, 241, 180, 224, 31, 107, 176, 210, 144, 240, 118, 73, 70, 191, 128, 119,
            141, 113, 125, 215, 161, 71, 49, 176, 87, 38, 180, 177, 39, 78,
        ];
        assert_eq!(expected_sum, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            ADD,
            invalid_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            SUB,
            left_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_difference = [
            150, 72, 222, 61, 148, 79, 96, 130, 151, 176, 29, 217, 231, 211, 0, 215, 76, 86, 212,
            146, 110, 128, 24, 151, 187, 144, 108, 233, 221, 208, 157, 52,
        ];
        assert_eq!(expected_difference, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            SUB,
            invalid_point_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            MUL,
            scalar_va,
            right_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        result.unwrap();
        let expected_product = [
            4, 16, 46, 2, 53, 151, 201, 133, 117, 149, 232, 164, 119, 109, 136, 20, 153, 24, 124,
            21, 101, 124, 80, 19, 119, 100, 77, 108, 65, 187, 228, 5,
        ];
        assert_eq!(expected_product, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            MUL,
            scalar_va,
            invalid_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(1, result.unwrap());

        let mut result = ProgramResult::Ok(0);
        SyscallCurveGroupOps::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            MUL,
            scalar_va,
            invalid_point_va,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_syscall_multiscalar_multiplication() {
        use solana_zk_token_sdk::curve25519::curve_syscall_traits::{
            CURVE25519_EDWARDS, CURVE25519_RISTRETTO,
        };

        let config = Config::default();
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let scalar_a: [u8; 32] = [
            254, 198, 23, 138, 67, 243, 184, 110, 236, 115, 236, 205, 205, 215, 79, 114, 45, 250,
            78, 137, 3, 107, 136, 237, 49, 126, 117, 223, 37, 191, 88, 6,
        ];
        let scalar_b: [u8; 32] = [
            254, 198, 23, 138, 67, 243, 184, 110, 236, 115, 236, 205, 205, 215, 79, 114, 45, 250,
            78, 137, 3, 107, 136, 237, 49, 126, 117, 223, 37, 191, 88, 6,
        ];

        let scalars = [scalar_a, scalar_b];
        let scalars_va = 0x100000000;

        let edwards_point_x: [u8; 32] = [
            252, 31, 230, 46, 173, 95, 144, 148, 158, 157, 63, 10, 8, 68, 58, 176, 142, 192, 168,
            53, 61, 105, 194, 166, 43, 56, 246, 236, 28, 146, 114, 133,
        ];
        let edwards_point_y: [u8; 32] = [
            10, 111, 8, 236, 97, 189, 124, 69, 89, 176, 222, 39, 199, 253, 111, 11, 248, 186, 128,
            90, 120, 128, 248, 210, 232, 183, 93, 104, 111, 150, 7, 241,
        ];
        let edwards_points = [edwards_point_x, edwards_point_y];
        let edwards_points_va = 0x200000000;

        let ristretto_point_x: [u8; 32] = [
            130, 35, 97, 25, 18, 199, 33, 239, 85, 143, 119, 111, 49, 51, 224, 40, 167, 185, 240,
            179, 25, 194, 213, 41, 14, 155, 104, 18, 181, 197, 15, 112,
        ];
        let ristretto_point_y: [u8; 32] = [
            152, 156, 155, 197, 152, 232, 92, 206, 219, 159, 193, 134, 121, 128, 139, 36, 56, 191,
            51, 143, 72, 204, 87, 76, 110, 124, 101, 96, 238, 158, 42, 108,
        ];
        let ristretto_points = [ristretto_point_x, ristretto_point_y];
        let ristretto_points_va = 0x300000000;

        let result_point: [u8; 32] = [0; 32];
        let result_point_va = 0x400000000;

        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion {
                    host_addr: scalars.as_ptr() as *const _ as u64,
                    vm_addr: scalars_va,
                    len: 64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: edwards_points.as_ptr() as *const _ as u64,
                    vm_addr: edwards_points_va,
                    len: 64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: ristretto_points.as_ptr() as *const _ as u64,
                    vm_addr: ristretto_points_va,
                    len: 64,
                    vm_gap_shift: 63,
                    is_writable: false,
                },
                MemoryRegion {
                    host_addr: result_point.as_ptr() as *const _ as u64,
                    vm_addr: result_point_va,
                    len: 32,
                    vm_gap_shift: 63,
                    is_writable: true,
                },
            ],
            &config,
        )
        .unwrap();

        invoke_context.mock_set_remaining(
            invoke_context
                .get_compute_budget()
                .curve25519_edwards_msm_base_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_edwards_msm_incremental_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_msm_base_cost
                + invoke_context
                    .get_compute_budget()
                    .curve25519_ristretto_msm_incremental_cost,
        );

        let mut result = ProgramResult::Ok(0);
        SyscallCurveMultiscalarMultiplication::call(
            &mut invoke_context,
            CURVE25519_EDWARDS,
            scalars_va,
            edwards_points_va,
            2,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_product = [
            30, 174, 168, 34, 160, 70, 63, 166, 236, 18, 74, 144, 185, 222, 208, 243, 5, 54, 223,
            172, 185, 75, 244, 26, 70, 18, 248, 46, 207, 184, 235, 60,
        ];
        assert_eq!(expected_product, result_point);

        let mut result = ProgramResult::Ok(0);
        SyscallCurveMultiscalarMultiplication::call(
            &mut invoke_context,
            CURVE25519_RISTRETTO,
            scalars_va,
            ristretto_points_va,
            2,
            result_point_va,
            &mut memory_mapping,
            &mut result,
        );

        assert_eq!(0, result.unwrap());
        let expected_product = [
            78, 120, 86, 111, 152, 64, 146, 84, 14, 236, 77, 147, 237, 190, 251, 241, 136, 167, 21,
            94, 84, 118, 92, 140, 120, 81, 30, 246, 173, 140, 195, 86,
        ];
        assert_eq!(expected_product, result_point);
    }

    fn create_filled_type<T: Default>(zero_init: bool) -> T {
        let mut val = T::default();
        let p = &mut val as *mut _ as *mut u8;
        for i in 0..(size_of::<T>() as isize) {
            unsafe {
                *p.offset(i) = if zero_init { 0 } else { i as u8 };
            }
        }
        val
    }

    fn are_bytes_equal<T>(first: &T, second: &T) -> bool {
        let p_first = first as *const _ as *const u8;
        let p_second = second as *const _ as *const u8;
        for i in 0..(size_of::<T>() as isize) {
            unsafe {
                if *p_first.offset(i) != *p_second.offset(i) {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    #[allow(deprecated)]
    fn test_syscall_get_sysvar() {
        let config = Config::default();

        let mut src_clock = create_filled_type::<Clock>(false);
        src_clock.slot = 1;
        src_clock.epoch_start_timestamp = 2;
        src_clock.epoch = 3;
        src_clock.leader_schedule_epoch = 4;
        src_clock.unix_timestamp = 5;
        let mut src_epochschedule = create_filled_type::<EpochSchedule>(false);
        src_epochschedule.slots_per_epoch = 1;
        src_epochschedule.leader_schedule_slot_offset = 2;
        src_epochschedule.warmup = false;
        src_epochschedule.first_normal_epoch = 3;
        src_epochschedule.first_normal_slot = 4;
        let mut src_fees = create_filled_type::<Fees>(false);
        src_fees.fee_calculator = FeeCalculator {
            lamports_per_signature: 1,
        };
        let mut src_rent = create_filled_type::<Rent>(false);
        src_rent.lamports_per_byte_year = 1;
        src_rent.exemption_threshold = 2.0;
        src_rent.burn_percent = 3;

        let mut sysvar_cache = SysvarCache::default();
        sysvar_cache.set_clock(src_clock.clone());
        sysvar_cache.set_epoch_schedule(src_epochschedule);
        sysvar_cache.set_fees(src_fees.clone());
        sysvar_cache.set_rent(src_rent);

        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        invoke_context.sysvar_cache = Cow::Owned(sysvar_cache);

        // Test clock sysvar
        {
            let got_clock = Clock::default();
            let got_clock_va = 0x100000000;

            let mut memory_mapping = MemoryMapping::new(
                vec![MemoryRegion {
                    host_addr: &got_clock as *const _ as u64,
                    vm_addr: got_clock_va,
                    len: size_of::<Clock>() as u64,
                    vm_gap_shift: 63,
                    is_writable: true,
                }],
                &config,
            )
            .unwrap();

            let mut result = ProgramResult::Ok(0);
            SyscallGetClockSysvar::call(
                &mut invoke_context,
                got_clock_va,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            result.unwrap();
            assert_eq!(got_clock, src_clock);

            let mut clean_clock = create_filled_type::<Clock>(true);
            clean_clock.slot = src_clock.slot;
            clean_clock.epoch_start_timestamp = src_clock.epoch_start_timestamp;
            clean_clock.epoch = src_clock.epoch;
            clean_clock.leader_schedule_epoch = src_clock.leader_schedule_epoch;
            clean_clock.unix_timestamp = src_clock.unix_timestamp;
            assert!(are_bytes_equal(&got_clock, &clean_clock));
        }

        // Test epoch_schedule sysvar
        {
            let got_epochschedule = EpochSchedule::default();
            let got_epochschedule_va = 0x100000000;

            let mut memory_mapping = MemoryMapping::new(
                vec![MemoryRegion {
                    host_addr: &got_epochschedule as *const _ as u64,
                    vm_addr: got_epochschedule_va,
                    len: size_of::<EpochSchedule>() as u64,
                    vm_gap_shift: 63,
                    is_writable: true,
                }],
                &config,
            )
            .unwrap();

            let mut result = ProgramResult::Ok(0);
            SyscallGetEpochScheduleSysvar::call(
                &mut invoke_context,
                got_epochschedule_va,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            result.unwrap();
            assert_eq!(got_epochschedule, src_epochschedule);

            let mut clean_epochschedule = create_filled_type::<EpochSchedule>(true);
            clean_epochschedule.slots_per_epoch = src_epochschedule.slots_per_epoch;
            clean_epochschedule.leader_schedule_slot_offset =
                src_epochschedule.leader_schedule_slot_offset;
            clean_epochschedule.warmup = src_epochschedule.warmup;
            clean_epochschedule.first_normal_epoch = src_epochschedule.first_normal_epoch;
            clean_epochschedule.first_normal_slot = src_epochschedule.first_normal_slot;
            assert!(are_bytes_equal(&got_epochschedule, &clean_epochschedule));
        }

        // Test fees sysvar
        {
            let got_fees = Fees::default();
            let got_fees_va = 0x100000000;

            let mut memory_mapping = MemoryMapping::new(
                vec![MemoryRegion {
                    host_addr: &got_fees as *const _ as u64,
                    vm_addr: got_fees_va,
                    len: size_of::<Fees>() as u64,
                    vm_gap_shift: 63,
                    is_writable: true,
                }],
                &config,
            )
            .unwrap();

            let mut result = ProgramResult::Ok(0);
            SyscallGetFeesSysvar::call(
                &mut invoke_context,
                got_fees_va,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            result.unwrap();
            assert_eq!(got_fees, src_fees);

            let mut clean_fees = create_filled_type::<Fees>(true);
            clean_fees.fee_calculator = src_fees.fee_calculator;
            assert!(are_bytes_equal(&got_fees, &clean_fees));
        }

        // Test rent sysvar
        {
            let got_rent = create_filled_type::<Rent>(true);
            let got_rent_va = 0x100000000;

            let mut memory_mapping = MemoryMapping::new(
                vec![MemoryRegion {
                    host_addr: &got_rent as *const _ as u64,
                    vm_addr: got_rent_va,
                    len: size_of::<Rent>() as u64,
                    vm_gap_shift: 63,
                    is_writable: true,
                }],
                &config,
            )
            .unwrap();

            let mut result = ProgramResult::Ok(0);
            SyscallGetRentSysvar::call(
                &mut invoke_context,
                got_rent_va,
                0,
                0,
                0,
                0,
                &mut memory_mapping,
                &mut result,
            );
            result.unwrap();
            assert_eq!(got_rent, src_rent);

            let mut clean_rent = create_filled_type::<Rent>(true);
            clean_rent.lamports_per_byte_year = src_rent.lamports_per_byte_year;
            clean_rent.exemption_threshold = src_rent.exemption_threshold;
            clean_rent.burn_percent = src_rent.burn_percent;
            assert!(are_bytes_equal(&got_rent, &clean_rent));
        }
    }

    fn call_program_address_common<'a, 'b: 'a>(
        invoke_context: &'a mut InvokeContext<'b>,
        seeds: &[&[u8]],
        program_id: &Pubkey,
        overlap_outputs: bool,
        syscall: BuiltInFunction<InvokeContext<'b>>,
    ) -> Result<(Pubkey, u8), EbpfError> {
        const SEEDS_VA: u64 = 0x100000000;
        const PROGRAM_ID_VA: u64 = 0x200000000;
        const ADDRESS_VA: u64 = 0x300000000;
        const BUMP_SEED_VA: u64 = 0x400000000;
        const SEED_VA: u64 = 0x500000000;

        let config = Config::default();
        let address = Pubkey::default();
        let bump_seed = 0;
        let mut mock_slices = Vec::with_capacity(seeds.len());
        let mut regions = vec![
            MemoryRegion {
                host_addr: mock_slices.as_ptr() as u64,
                vm_addr: SEEDS_VA,
                len: (seeds.len().saturating_mul(size_of::<MockSlice>()) as u64),
                vm_gap_shift: 63,
                is_writable: false,
            },
            MemoryRegion {
                host_addr: program_id.as_ref().as_ptr() as u64,
                vm_addr: PROGRAM_ID_VA,
                len: 32,
                vm_gap_shift: 63,
                is_writable: false,
            },
            MemoryRegion {
                host_addr: address.as_ref().as_ptr() as u64,
                vm_addr: ADDRESS_VA,
                len: 32,
                vm_gap_shift: 63,
                is_writable: true,
            },
            MemoryRegion {
                host_addr: &bump_seed as *const u8 as u64,
                vm_addr: BUMP_SEED_VA,
                len: 32,
                vm_gap_shift: 63,
                is_writable: true,
            },
        ];

        for (i, seed) in seeds.iter().enumerate() {
            let vm_addr = SEED_VA.saturating_add((i as u64).saturating_mul(0x100000000));
            let mock_slice = MockSlice {
                vm_addr,
                len: seed.len(),
            };
            mock_slices.push(mock_slice);
            regions.push(MemoryRegion {
                host_addr: seed.as_ptr() as u64,
                vm_addr,
                len: seed.len() as u64,
                vm_gap_shift: 63,
                is_writable: false,
            });
        }
        let mut memory_mapping = MemoryMapping::new(regions, &config).unwrap();

        let mut result = ProgramResult::Ok(0);
        syscall(
            invoke_context,
            SEEDS_VA,
            seeds.len() as u64,
            PROGRAM_ID_VA,
            ADDRESS_VA,
            if overlap_outputs {
                ADDRESS_VA
            } else {
                BUMP_SEED_VA
            },
            &mut memory_mapping,
            &mut result,
        );
        Result::<u64, EbpfError>::from(result).map(|_| (address, bump_seed))
    }

    fn create_program_address(
        invoke_context: &mut InvokeContext,
        seeds: &[&[u8]],
        address: &Pubkey,
    ) -> Result<Pubkey, EbpfError> {
        let (address, _) = call_program_address_common(
            invoke_context,
            seeds,
            address,
            false,
            SyscallCreateProgramAddress::call,
        )?;
        Ok(address)
    }

    fn try_find_program_address(
        invoke_context: &mut InvokeContext,
        seeds: &[&[u8]],
        address: &Pubkey,
    ) -> Result<(Pubkey, u8), EbpfError> {
        call_program_address_common(
            invoke_context,
            seeds,
            address,
            false,
            SyscallTryFindProgramAddress::call,
        )
    }

    #[test]
    fn test_set_and_get_return_data() {
        const SRC_VA: u64 = 0x100000000;
        const DST_VA: u64 = 0x200000000;
        const PROGRAM_ID_VA: u64 = 0x300000000;
        let data = vec![42; 24];
        let mut data_buffer = vec![0; 16];
        let mut id_buffer = vec![0; 32];

        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&data, SRC_VA),
                MemoryRegion::new_writable(&mut data_buffer, DST_VA),
                MemoryRegion::new_writable(&mut id_buffer, PROGRAM_ID_VA),
            ],
            &config,
        )
        .unwrap();

        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );

        let mut result = ProgramResult::Ok(0);
        SyscallSetReturnData::call(
            &mut invoke_context,
            SRC_VA,
            data.len() as u64,
            0,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(result.unwrap(), 0);

        let mut result = ProgramResult::Ok(0);
        SyscallGetReturnData::call(
            &mut invoke_context,
            DST_VA,
            data_buffer.len() as u64,
            PROGRAM_ID_VA,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(result.unwrap() as usize, data.len());
        assert_eq!(data.get(0..data_buffer.len()).unwrap(), data_buffer);
        assert_eq!(id_buffer, program_id.to_bytes());

        let mut result = ProgramResult::Ok(0);
        SyscallGetReturnData::call(
            &mut invoke_context,
            PROGRAM_ID_VA,
            data_buffer.len() as u64,
            PROGRAM_ID_VA,
            0,
            0,
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::CopyOverlapping
            ),
        ));
    }

    #[test]
    fn test_syscall_sol_get_processed_sibling_instruction() {
        let transaction_accounts = (0..9)
            .map(|_| {
                (
                    Pubkey::new_unique(),
                    AccountSharedData::new(0, 0, &bpf_loader::id()),
                )
            })
            .collect::<Vec<_>>();
        let instruction_trace = [1, 2, 3, 2, 2, 3, 4, 3];
        let mut transaction_context =
            TransactionContext::new(transaction_accounts, None, 4, instruction_trace.len());
        for (index_in_trace, stack_height) in instruction_trace.into_iter().enumerate() {
            while stack_height <= transaction_context.get_instruction_context_stack_height() {
                transaction_context.pop().unwrap();
            }
            if stack_height > transaction_context.get_instruction_context_stack_height() {
                let instruction_accounts = [InstructionAccount {
                    index_in_transaction: index_in_trace.saturating_add(1) as IndexOfAccount,
                    index_in_caller: 0, // This is incorrect / inconsistent but not required
                    index_in_callee: 0,
                    is_signer: false,
                    is_writable: false,
                }];
                transaction_context
                    .get_next_instruction_context()
                    .unwrap()
                    .configure(&[0], &instruction_accounts, &[index_in_trace as u8]);
                transaction_context.push().unwrap();
            }
        }
        let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);

        let syscall_base_cost = invoke_context.get_compute_budget().syscall_base_cost;

        const VM_BASE_ADDRESS: u64 = 0x100000000;
        const META_OFFSET: usize = 0;
        const PROGRAM_ID_OFFSET: usize =
            META_OFFSET + std::mem::size_of::<ProcessedSiblingInstruction>();
        const DATA_OFFSET: usize = PROGRAM_ID_OFFSET + std::mem::size_of::<Pubkey>();
        const ACCOUNTS_OFFSET: usize = DATA_OFFSET + 0x100;
        const END_OFFSET: usize = ACCOUNTS_OFFSET + std::mem::size_of::<AccountInfo>() * 4;
        let mut memory = [0u8; END_OFFSET];
        let config = Config::default();
        let mut memory_mapping = MemoryMapping::new(
            vec![MemoryRegion {
                host_addr: memory.as_mut_ptr() as u64,
                vm_addr: VM_BASE_ADDRESS,
                len: END_OFFSET as u64,
                vm_gap_shift: 63,
                is_writable: true,
            }],
            &config,
        )
        .unwrap();
        let processed_sibling_instruction = translate_type_mut::<ProcessedSiblingInstruction>(
            &memory_mapping,
            VM_BASE_ADDRESS,
            true,
        )
        .unwrap();
        processed_sibling_instruction.data_len = 1;
        processed_sibling_instruction.accounts_len = 1;
        let program_id = translate_type_mut::<Pubkey>(
            &memory_mapping,
            VM_BASE_ADDRESS.saturating_add(PROGRAM_ID_OFFSET as u64),
            true,
        )
        .unwrap();
        let data = translate_slice_mut::<u8>(
            &memory_mapping,
            VM_BASE_ADDRESS.saturating_add(DATA_OFFSET as u64),
            processed_sibling_instruction.data_len,
            true,
            true,
        )
        .unwrap();
        let accounts = translate_slice_mut::<AccountMeta>(
            &memory_mapping,
            VM_BASE_ADDRESS.saturating_add(ACCOUNTS_OFFSET as u64),
            processed_sibling_instruction.accounts_len,
            true,
            true,
        )
        .unwrap();

        invoke_context.mock_set_remaining(syscall_base_cost);
        let mut result = ProgramResult::Ok(0);
        SyscallGetProcessedSiblingInstruction::call(
            &mut invoke_context,
            0,
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(PROGRAM_ID_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(DATA_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(ACCOUNTS_OFFSET as u64),
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(result.unwrap(), 1);
        {
            let transaction_context = &invoke_context.transaction_context;
            assert_eq!(processed_sibling_instruction.data_len, 1);
            assert_eq!(processed_sibling_instruction.accounts_len, 1);
            assert_eq!(
                program_id,
                transaction_context.get_key_of_account_at_index(0).unwrap(),
            );
            assert_eq!(data, &[5]);
            assert_eq!(
                accounts,
                &[AccountMeta {
                    pubkey: *transaction_context.get_key_of_account_at_index(6).unwrap(),
                    is_signer: false,
                    is_writable: false
                }]
            );
        }

        invoke_context.mock_set_remaining(syscall_base_cost);
        let mut result = ProgramResult::Ok(0);
        SyscallGetProcessedSiblingInstruction::call(
            &mut invoke_context,
            1,
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(PROGRAM_ID_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(DATA_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(ACCOUNTS_OFFSET as u64),
            &mut memory_mapping,
            &mut result,
        );
        assert_eq!(result.unwrap(), 0);

        invoke_context.mock_set_remaining(syscall_base_cost);
        let mut result = ProgramResult::Ok(0);
        SyscallGetProcessedSiblingInstruction::call(
            &mut invoke_context,
            0,
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            VM_BASE_ADDRESS.saturating_add(META_OFFSET as u64),
            &mut memory_mapping,
            &mut result,
        );
        assert!(matches!(
            result,
            ProgramResult::Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::CopyOverlapping
            ),
        ));
    }

    #[test]
    fn test_create_program_address() {
        // These tests duplicate the direct tests in solana_program::pubkey

        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        let address = bpf_loader_upgradeable::id();

        let exceeded_seed = &[127; MAX_SEED_LEN + 1];
        assert!(matches!(
            create_program_address(&mut invoke_context, &[exceeded_seed], &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded)
            ),
        ));
        assert!(matches!(
            create_program_address(
                &mut invoke_context,
                &[b"short_seed", exceeded_seed],
                &address,
            ),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded)
            ),
        ));
        let max_seed = &[0; MAX_SEED_LEN];
        assert!(create_program_address(&mut invoke_context, &[max_seed], &address).is_ok());
        let exceeded_seeds: &[&[u8]] = &[
            &[1],
            &[2],
            &[3],
            &[4],
            &[5],
            &[6],
            &[7],
            &[8],
            &[9],
            &[10],
            &[11],
            &[12],
            &[13],
            &[14],
            &[15],
            &[16],
        ];
        assert!(create_program_address(&mut invoke_context, exceeded_seeds, &address).is_ok());
        let max_seeds: &[&[u8]] = &[
            &[1],
            &[2],
            &[3],
            &[4],
            &[5],
            &[6],
            &[7],
            &[8],
            &[9],
            &[10],
            &[11],
            &[12],
            &[13],
            &[14],
            &[15],
            &[16],
            &[17],
        ];
        assert!(matches!(
            create_program_address(&mut invoke_context, max_seeds, &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded)
            ),
        ));
        assert_eq!(
            create_program_address(&mut invoke_context, &[b"", &[1]], &address).unwrap(),
            "BwqrghZA2htAcqq8dzP1WDAhTXYTYWj7CHxF5j7TDBAe"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            create_program_address(&mut invoke_context, &["☉".as_ref(), &[0]], &address).unwrap(),
            "13yWmRpaTR4r5nAktwLqMpRNr28tnVUZw26rTvPSSB19"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            create_program_address(&mut invoke_context, &[b"Talking", b"Squirrels"], &address)
                .unwrap(),
            "2fnQrngrQT4SeLcdToJAD96phoEjNL2man2kfRLCASVk"
                .parse()
                .unwrap(),
        );
        let public_key = Pubkey::from_str("SeedPubey1111111111111111111111111111111111").unwrap();
        assert_eq!(
            create_program_address(&mut invoke_context, &[public_key.as_ref(), &[1]], &address)
                .unwrap(),
            "976ymqVnfE32QFe6NfGDctSvVa36LWnvYxhU6G2232YL"
                .parse()
                .unwrap(),
        );
        assert_ne!(
            create_program_address(&mut invoke_context, &[b"Talking", b"Squirrels"], &address)
                .unwrap(),
            create_program_address(&mut invoke_context, &[b"Talking"], &address).unwrap(),
        );
        invoke_context.mock_set_remaining(0);
        assert!(matches!(
            create_program_address(&mut invoke_context, &[b"", &[1]], &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));
    }

    #[test]
    fn test_find_program_address() {
        prepare_mockup!(
            invoke_context,
            transaction_context,
            program_id,
            bpf_loader::id(),
        );
        let cost = invoke_context
            .get_compute_budget()
            .create_program_address_units;
        let address = bpf_loader_upgradeable::id();
        let max_tries = 256; // one per seed

        for _ in 0..1_000 {
            let address = Pubkey::new_unique();
            invoke_context.mock_set_remaining(cost * max_tries);
            let (found_address, bump_seed) =
                try_find_program_address(&mut invoke_context, &[b"Lil'", b"Bits"], &address)
                    .unwrap();
            assert_eq!(
                found_address,
                create_program_address(
                    &mut invoke_context,
                    &[b"Lil'", b"Bits", &[bump_seed]],
                    &address,
                )
                .unwrap()
            );
        }

        let seeds: &[&[u8]] = &[b""];
        invoke_context.mock_set_remaining(cost * max_tries);
        let (_, bump_seed) =
            try_find_program_address(&mut invoke_context, seeds, &address).unwrap();
        invoke_context.mock_set_remaining(cost * (max_tries - bump_seed as u64));
        try_find_program_address(&mut invoke_context, seeds, &address).unwrap();
        invoke_context.mock_set_remaining(cost * (max_tries - bump_seed as u64 - 1));
        assert!(matches!(
            try_find_program_address(&mut invoke_context, seeds, &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::ComputationalBudgetExceeded)
            ),
        ));

        let exceeded_seed = &[127; MAX_SEED_LEN + 1];
        invoke_context.mock_set_remaining(cost * (max_tries - 1));
        assert!(matches!(
            try_find_program_address(&mut invoke_context, &[exceeded_seed], &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded)
            ),
        ));
        let exceeded_seeds: &[&[u8]] = &[
            &[1],
            &[2],
            &[3],
            &[4],
            &[5],
            &[6],
            &[7],
            &[8],
            &[9],
            &[10],
            &[11],
            &[12],
            &[13],
            &[14],
            &[15],
            &[16],
            &[17],
        ];
        invoke_context.mock_set_remaining(cost * (max_tries - 1));
        assert!(matches!(
            try_find_program_address(&mut invoke_context, exceeded_seeds, &address),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::BadSeeds(PubkeyError::MaxSeedLengthExceeded)
            ),
        ));

        assert!(matches!(
            call_program_address_common(
                &mut invoke_context,
                seeds,
                &address,
                true,
                SyscallTryFindProgramAddress::call,
            ),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::CopyOverlapping
            ),
        ));
    }

    #[test]
    fn test_check_type_assumptions() {
        check_type_assumptions();
    }
}
