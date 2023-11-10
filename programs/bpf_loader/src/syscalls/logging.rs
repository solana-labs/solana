use {super::*, solana_rbpf::vm::ContextObject};

declare_builtin_function!(
    /// Log a user's info message
    SyscallLog,
    fn rust(
        invoke_context: &mut InvokeContext,
        addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let cost = invoke_context
            .get_compute_budget()
            .syscall_base_cost
            .max(len);
        consume_compute_meter(invoke_context, cost)?;

        translate_string_and_do(
            memory_mapping,
            addr,
            len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
            invoke_context
                .feature_set
                .is_active(&stop_truncating_strings_in_syscalls::id()),
            &mut |string: &str| {
                stable_log::program_log(&invoke_context.get_log_collector(), string);
                Ok(0)
            },
        )?;
        Ok(0)
    }
);

declare_builtin_function!(
    /// Log 5 64-bit values
    SyscallLogU64,
    fn rust(
        invoke_context: &mut InvokeContext,
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let cost = invoke_context.get_compute_budget().log_64_units;
        consume_compute_meter(invoke_context, cost)?;

        stable_log::program_log(
            &invoke_context.get_log_collector(),
            &format!("{arg1:#x}, {arg2:#x}, {arg3:#x}, {arg4:#x}, {arg5:#x}"),
        );
        Ok(0)
    }
);

declare_builtin_function!(
    /// Log current compute consumption
    SyscallLogBpfComputeUnits,
    fn rust(
        invoke_context: &mut InvokeContext,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let cost = invoke_context.get_compute_budget().syscall_base_cost;
        consume_compute_meter(invoke_context, cost)?;

        ic_logger_msg!(
            invoke_context.get_log_collector(),
            "Program consumption: {} units remaining",
            invoke_context.get_remaining(),
        );
        Ok(0)
    }
);

declare_builtin_function!(
    /// Log 5 64-bit values
    SyscallLogPubkey,
    fn rust(
        invoke_context: &mut InvokeContext,
        pubkey_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let cost = invoke_context.get_compute_budget().log_pubkey_units;
        consume_compute_meter(invoke_context, cost)?;

        let pubkey = translate_type::<Pubkey>(
            memory_mapping,
            pubkey_addr,
            invoke_context.get_check_aligned(),
        )?;
        stable_log::program_log(&invoke_context.get_log_collector(), &pubkey.to_string());
        Ok(0)
    }
);

declare_builtin_function!(
    /// Log data handling
    SyscallLogData,
    fn rust(
        invoke_context: &mut InvokeContext,
        addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let budget = invoke_context.get_compute_budget();

        consume_compute_meter(invoke_context, budget.syscall_base_cost)?;

        let untranslated_fields = translate_slice::<&[u8]>(
            memory_mapping,
            addr,
            len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        consume_compute_meter(
            invoke_context,
            budget
                .syscall_base_cost
                .saturating_mul(untranslated_fields.len() as u64),
        )?;
        consume_compute_meter(
            invoke_context,
            untranslated_fields
                .iter()
                .fold(0, |total, e| total.saturating_add(e.len() as u64)),
        )?;

        let mut fields = Vec::with_capacity(untranslated_fields.len());

        for untranslated_field in untranslated_fields {
            fields.push(translate_slice::<u8>(
                memory_mapping,
                untranslated_field.as_ptr() as *const _ as u64,
                untranslated_field.len() as u64,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?);
        }

        let log_collector = invoke_context.get_log_collector();

        stable_log::program_data(&log_collector, &fields);

        Ok(0)
    }
);
