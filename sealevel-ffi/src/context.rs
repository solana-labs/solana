use {
    solana_bpf_loader_program::syscalls::{
        SyscallAbort, SyscallAllocFree, SyscallBlake3, SyscallCreateProgramAddress,
        SyscallCurveGroupOps, SyscallCurvePointValidation, SyscallGetClockSysvar,
        SyscallGetEpochScheduleSysvar, SyscallGetFeesSysvar, SyscallGetProcessedSiblingInstruction,
        SyscallGetRentSysvar, SyscallGetReturnData, SyscallGetStackHeight, SyscallInvokeSignedC,
        SyscallInvokeSignedRust, SyscallKeccak256, SyscallLog, SyscallLogBpfComputeUnits,
        SyscallLogData, SyscallLogPubkey, SyscallLogU64, SyscallMemcmp, SyscallMemcpy,
        SyscallMemmove, SyscallMemset, SyscallPanic, SyscallSecp256k1Recover, SyscallSetReturnData,
        SyscallSha256, SyscallTryFindProgramAddress, SyscallZkTokenElgamalOp,
        SyscallZkTokenElgamalOpWithLoHi, SyscallZkTokenElgamalOpWithScalar,
    },
    solana_rbpf::vm::{SyscallObject, SyscallRegistry},
    std::ffi::c_void,
};
use crate::error::hoist_error;

/// The map of syscalls provided by the virtual machine.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct sealevel_syscall_registry(pub(crate) *mut c_void);

#[repr(C)]
pub enum sealevel_syscall_id {
    SEALEVEL_SYSCALL_INVALID,
    SEALEVEL_SYSCALL_ABORT,
    SEALEVEL_SYSCALL_SOL_PANIC,
    SEALEVEL_SYSCALL_SOL_LOG,
    SEALEVEL_SYSCALL_SOL_LOG_64,
    SEALEVEL_SYSCALL_SOL_LOG_COMPUTE_UNITS,
    SEALEVEL_SYSCALL_SOL_LOG_PUBKEY,
    SEALEVEL_SYSCALL_SOL_CREATE_PROGRAM_ADDRESS,
    SEALEVEL_SYSCALL_SOL_TRY_FIND_PROGRAM_ADDRESS,
    SEALEVEL_SYSCALL_SOL_SHA256,
    SEALEVEL_SYSCALL_SOL_KECCAK256,
    SEALEVEL_SYSCALL_SOL_SECP256K1_RECOVER,
    SEALEVEL_SYSCALL_SOL_BLAKE3,
    SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP,
    SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP_WITH_LO_HI,
    SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP_WITH_SCALAR,
    SEALEVEL_SYSCALL_SOL_CURVE_VALIDATE_POINT,
    SEALEVEL_SYSCALL_SOL_CURVE_GROUP_OP,
    SEALEVEL_SYSCALL_SOL_GET_CLOCK_SYSVAR,
    SEALEVEL_SYSCALL_SOL_GET_EPOCH_SCHEDULE_SYSVAR,
    SEALEVEL_SYSCALL_SOL_GET_FEES_SYSVAR,
    SEALEVEL_SYSCALL_SOL_GET_RENT_SYSVAR,
    SEALEVEL_SYSCALL_SOL_MEMCPY,
    SEALEVEL_SYSCALL_SOL_MEMMOVE,
    SEALEVEL_SYSCALL_SOL_MEMCMP,
    SEALEVEL_SYSCALL_SOL_MEMSET,
    SEALEVEL_SYSCALL_SOL_INVOKE_SIGNED_C,
    SEALEVEL_SYSCALL_SOL_INVOKE_SIGNED_RUST,
    SEALEVEL_SYSCALL_SOL_ALLOC_FREE,
    SEALEVEL_SYSCALL_SOL_SET_RETURN_DATA,
    SEALEVEL_SYSCALL_SOL_GET_RETURN_DATA,
    SEALEVEL_SYSCALL_SOL_LOG_DATA,
    SEALEVEL_SYSCALL_SOL_GET_PROCESSED_SIBLING_INSTRUCTION,
    SEALEVEL_SYSCALL_SOL_GET_STACK_HEIGHT,
}

impl sealevel_syscall_registry {
    fn unwrap(self) -> *mut SyscallRegistry {
        self.0 as *mut SyscallRegistry
    }
}

/// Creates a new, empty syscall registry.
///
/// # Safety
/// Call `sealevel_syscall_registry_free` on the return value after you are done using it.
/// Failure to do so results in a memory leak.
#[no_mangle]
pub extern "C" fn sealevel_syscall_registry_new() -> sealevel_syscall_registry {
    let registry = SyscallRegistry::default();
    sealevel_syscall_registry(Box::into_raw(Box::new(registry)) as *mut c_void)
}

/// Frees a syscall registry.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function twice on the same object.
/// - Using the syscall registry after calling this function.
#[no_mangle]
pub unsafe extern "C" fn sealevel_syscall_registry_free(registry: sealevel_syscall_registry) {
    if registry.0.is_null() {
        return;
    }
    drop(Box::from_raw(registry.unwrap()))
}

/// Registers a Solana builtin syscall.
///
/// Returns TRUE when registration was successful.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Passing a NULL pointer.
/// - Passing a registry that was already freed with `sealevel_syscall_registry_free`.
/// - Passing a registry that was created from another thread.
#[no_mangle]
pub unsafe extern "C" fn sealevel_syscall_register_builtin(
    registry: sealevel_syscall_registry,
    syscall_id: sealevel_syscall_id,
) -> bool {
    let registry_ptr = registry.unwrap();
    let syscall_registry = &mut (*registry_ptr);
    // TODO: this is copy pasted from //programs/bpf-loader/src/syscalls.rs
    // TODO: handle unknown/future enum IDs to meet ABI stability
    let result = match syscall_id {
        sealevel_syscall_id::SEALEVEL_SYSCALL_INVALID => return false,
        sealevel_syscall_id::SEALEVEL_SYSCALL_ABORT => syscall_registry.register_syscall_by_name(
            b"abort",
            SyscallAbort::init,
            SyscallAbort::call,
        ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_PANIC => syscall_registry
            .register_syscall_by_name(b"sol_panic_", SyscallPanic::init, SyscallPanic::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_LOG => syscall_registry.register_syscall_by_name(
            b"sol_log_",
            SyscallLog::init,
            SyscallLog::call,
        ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_LOG_64 => syscall_registry
            .register_syscall_by_name(b"sol_log_64_", SyscallLogU64::init, SyscallLogU64::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_LOG_COMPUTE_UNITS => syscall_registry
            .register_syscall_by_name(
                b"sol_log_compute_units_",
                SyscallLogBpfComputeUnits::init,
                SyscallLogBpfComputeUnits::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_LOG_PUBKEY => syscall_registry
            .register_syscall_by_name(
                b"sol_log_pubkey",
                SyscallLogPubkey::init,
                SyscallLogPubkey::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_CREATE_PROGRAM_ADDRESS => syscall_registry
            .register_syscall_by_name(
                b"sol_create_program_address",
                SyscallCreateProgramAddress::init,
                SyscallCreateProgramAddress::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_TRY_FIND_PROGRAM_ADDRESS => syscall_registry
            .register_syscall_by_name(
                b"sol_try_find_program_address",
                SyscallTryFindProgramAddress::init,
                SyscallTryFindProgramAddress::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_SHA256 => syscall_registry
            .register_syscall_by_name(b"sol_sha256", SyscallSha256::init, SyscallSha256::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_KECCAK256 => syscall_registry
            .register_syscall_by_name(
                b"sol_keccak256",
                SyscallKeccak256::init,
                SyscallKeccak256::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_SECP256K1_RECOVER => syscall_registry
            .register_syscall_by_name(
                b"sol_secp256k1_recover",
                SyscallSecp256k1Recover::init,
                SyscallSecp256k1Recover::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_BLAKE3 => syscall_registry
            .register_syscall_by_name(b"sol_blake3", SyscallBlake3::init, SyscallBlake3::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP => syscall_registry
            .register_syscall_by_name(
                b"sol_zk_token_elgamal_op",
                SyscallZkTokenElgamalOp::init,
                SyscallZkTokenElgamalOp::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP_WITH_LO_HI => {
            syscall_registry.register_syscall_by_name(
                b"sol_zk_token_elgamal_op_with_lo_hi",
                SyscallZkTokenElgamalOpWithLoHi::init,
                SyscallZkTokenElgamalOpWithLoHi::call,
            )
        }
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_ZK_TOKEN_ELGAMAL_OP_WITH_SCALAR => {
            syscall_registry.register_syscall_by_name(
                b"sol_zk_token_elgamal_op_with_scalar",
                SyscallZkTokenElgamalOpWithScalar::init,
                SyscallZkTokenElgamalOpWithScalar::call,
            )
        }
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_CURVE_VALIDATE_POINT => syscall_registry
            .register_syscall_by_name(
                b"sol_curve_validate_point",
                SyscallCurvePointValidation::init,
                SyscallCurvePointValidation::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_CURVE_GROUP_OP => syscall_registry
            .register_syscall_by_name(
                b"sol_curve_group_op",
                SyscallCurveGroupOps::init,
                SyscallCurveGroupOps::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_CLOCK_SYSVAR => syscall_registry
            .register_syscall_by_name(
                b"sol_get_clock_sysvar",
                SyscallGetClockSysvar::init,
                SyscallGetClockSysvar::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_EPOCH_SCHEDULE_SYSVAR => syscall_registry
            .register_syscall_by_name(
                b"sol_get_epoch_schedule_sysvar",
                SyscallGetEpochScheduleSysvar::init,
                SyscallGetEpochScheduleSysvar::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_FEES_SYSVAR => syscall_registry
            .register_syscall_by_name(
                b"sol_get_fees_sysvar",
                SyscallGetFeesSysvar::init,
                SyscallGetFeesSysvar::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_RENT_SYSVAR => syscall_registry
            .register_syscall_by_name(
                b"sol_get_rent_sysvar",
                SyscallGetRentSysvar::init,
                SyscallGetRentSysvar::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_MEMCPY => syscall_registry
            .register_syscall_by_name(b"sol_memcpy_", SyscallMemcpy::init, SyscallMemcpy::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_MEMMOVE => syscall_registry
            .register_syscall_by_name(b"sol_memmove_", SyscallMemmove::init, SyscallMemmove::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_MEMCMP => syscall_registry
            .register_syscall_by_name(b"sol_memcmp_", SyscallMemcmp::init, SyscallMemcmp::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_MEMSET => syscall_registry
            .register_syscall_by_name(b"sol_memset_", SyscallMemset::init, SyscallMemset::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_INVOKE_SIGNED_C => syscall_registry
            .register_syscall_by_name(
                b"sol_invoke_signed_c",
                SyscallInvokeSignedC::init,
                SyscallInvokeSignedC::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_INVOKE_SIGNED_RUST => syscall_registry
            .register_syscall_by_name(
                b"sol_invoke_signed_rust",
                SyscallInvokeSignedRust::init,
                SyscallInvokeSignedRust::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_ALLOC_FREE => syscall_registry
            .register_syscall_by_name(
                b"sol_alloc_free_",
                SyscallAllocFree::init,
                SyscallAllocFree::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_SET_RETURN_DATA => syscall_registry
            .register_syscall_by_name(
                b"sol_set_return_data",
                SyscallSetReturnData::init,
                SyscallSetReturnData::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_RETURN_DATA => syscall_registry
            .register_syscall_by_name(
                b"sol_get_return_data",
                SyscallGetReturnData::init,
                SyscallGetReturnData::call,
            ),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_LOG_DATA => syscall_registry
            .register_syscall_by_name(b"sol_log_data", SyscallLogData::init, SyscallLogData::call),
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_PROCESSED_SIBLING_INSTRUCTION => {
            syscall_registry.register_syscall_by_name(
                b"sol_get_processed_sibling_instruction",
                SyscallGetProcessedSiblingInstruction::init,
                SyscallGetProcessedSiblingInstruction::call,
            )
        }
        sealevel_syscall_id::SEALEVEL_SYSCALL_SOL_GET_STACK_HEIGHT => syscall_registry
            .register_syscall_by_name(
                b"sol_get_stack_height",
                SyscallGetStackHeight::init,
                SyscallGetStackHeight::call,
            ),
    };
    hoist_error(result).is_some()
}

/// The invoke context holds the state of a single transaction execution.
/// It tracks the execution progress (instruction being executed),
/// interfaces with account data,
/// and specifies the on-chain execution rules (precompiles, syscalls, sysvars).
pub struct sealevel_invoke_context {}

/// Drops an invoke context and all programs created with it. Noop given a null pointer.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function twice on the same object.
/// - Using the invoke context after calling this function.
#[no_mangle]
pub unsafe extern "C" fn sealevel_invoke_context_free(this: *mut sealevel_invoke_context) {
    if this.is_null() {
        return;
    }
    drop(Box::from_raw(this))
}
