use super::*;

fn get_sysvar<T: std::fmt::Debug + Sysvar + SysvarId + Clone>(
    sysvar: Result<Arc<T>, InstructionError>,
    var_addr: u64,
    check_aligned: bool,
    memory_mapping: &mut MemoryMapping,
    invoke_context: &mut InvokeContext,
) -> Result<u64, Error> {
    consume_compute_meter(
        invoke_context,
        invoke_context
            .get_compute_budget()
            .sysvar_base_cost
            .saturating_add(size_of::<T>() as u64),
    )?;
    let var = translate_type_mut::<T>(memory_mapping, var_addr, check_aligned)?;

    // this clone looks unecessary now, but it exists to zero out trailing alignment bytes
    // it is unclear whether this should ever matter
    // but there are tests using MemoryMapping that expect to see this
    // we preserve the previous behavior out of an abundance of caution
    let sysvar: Arc<T> = sysvar?;
    *var = T::clone(sysvar.as_ref());

    Ok(SUCCESS)
}

declare_builtin_function!(
    /// Get a Clock sysvar
    SyscallGetClockSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_clock(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a EpochSchedule sysvar
    SyscallGetEpochScheduleSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_epoch_schedule(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a EpochRewards sysvar
    SyscallGetEpochRewardsSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_epoch_rewards(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a Fees sysvar
    SyscallGetFeesSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        #[allow(deprecated)]
        {
            get_sysvar(
                invoke_context.get_sysvar_cache().get_fees(),
                var_addr,
                invoke_context.get_check_aligned(),
                memory_mapping,
                invoke_context,
            )
        }
    }
);

declare_builtin_function!(
    /// Get a Rent sysvar
    SyscallGetRentSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_rent(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a Last Restart Slot sysvar
    SyscallGetLastRestartSlotSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_last_restart_slot(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

const SYSVAR_NOT_FOUND: u64 = 2;
const OFFSET_LENGTH_EXCEEDS_SYSVAR: u64 = 1;

// quoted language from SIMD0127
// because this syscall can both return error codes and abort, well-ordered error checking is crucial
declare_builtin_function!(
    /// Get a slice of a Sysvar in-memory representation
    SyscallGetSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        sysvar_id_addr: u64,
        var_addr: u64,
        offset: u64,
        length: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let check_aligned = invoke_context.get_check_aligned();
        let ComputeBudget {
            sysvar_base_cost,
            cpi_bytes_per_unit,
            mem_op_base_cost,
            ..
        } = *invoke_context.get_compute_budget();

        // Abort: "Compute budget is exceeded."
        let sysvar_id_cost = 32_u64.checked_div(cpi_bytes_per_unit).unwrap_or(0);
        let sysvar_buf_cost = length.checked_div(cpi_bytes_per_unit).unwrap_or(0);
        consume_compute_meter(
            invoke_context,
            sysvar_base_cost
                .saturating_add(sysvar_id_cost)
                .saturating_add(std::cmp::max(sysvar_buf_cost, mem_op_base_cost)),
        )?;

        // Abort: "Not all bytes in VM memory range `[sysvar_id, sysvar_id + 32)` are readable."
        let sysvar_id = translate_type::<Pubkey>(memory_mapping, sysvar_id_addr, check_aligned)?;

        // Abort: "Not all bytes in VM memory range `[var_addr, var_addr + length)` are writable."
        let var = translate_slice_mut::<u8>(memory_mapping, var_addr, length, check_aligned)?;

        // Abort: "`offset + length` is not in `[0, 2^64)`."
        let offset_length = offset
            .checked_add(length)
            .ok_or(InstructionError::ArithmeticOverflow)?;

        // Abort: "`var_addr + length` is not in `[0, 2^64)`."
        let _ = var_addr
            .checked_add(length)
            .ok_or(InstructionError::ArithmeticOverflow)?;

        let cache = invoke_context.get_sysvar_cache();

        // "`2` if the sysvar data is not present in the Sysvar Cache."
        let sysvar_buf = match cache.sysvar_id_to_buffer(sysvar_id) {
            None => return Ok(SYSVAR_NOT_FOUND),
            Some(ref sysvar_buf) => sysvar_buf,
        };

        // "`1` if `offset + length` is greater than the length of the sysvar data."
        if let Some(sysvar_slice) = sysvar_buf.get(offset as usize..offset_length as usize) {
            var.copy_from_slice(sysvar_slice);
        } else {
            return Ok(OFFSET_LENGTH_EXCEEDS_SYSVAR);
        }

        Ok(SUCCESS)
    }
);
