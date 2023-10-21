use {
    solana_measure::measure::Measure,
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        ic_logger_msg,
        invoke_context::InvokeContext,
        loaded_programs::{
            LoadProgramMetrics, LoadedProgram, LoadedProgramType, DELAY_VISIBILITY_SLOT_OFFSET,
        },
        log_collector::LogCollector,
        stable_log,
    },
    solana_rbpf::{
        aligned_memory::AlignedMemory,
        declare_builtin_function, ebpf,
        elf::Executable,
        error::ProgramResult,
        memory_region::{MemoryMapping, MemoryRegion},
        program::{BuiltinProgram, FunctionRegistry},
        vm::{Config, ContextObject, EbpfVm},
    },
    solana_sdk::{
        entrypoint::SUCCESS,
        feature_set,
        instruction::InstructionError,
        loader_v4::{self, LoaderV4State, LoaderV4Status, DEPLOYMENT_COOLDOWN_IN_SLOTS},
        loader_v4_instruction::LoaderV4Instruction,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        saturating_add_assign,
        transaction_context::{BorrowedAccount, InstructionContext},
    },
    std::{
        cell::RefCell,
        rc::Rc,
        sync::{atomic::Ordering, Arc},
    },
};

pub const DEFAULT_COMPUTE_UNITS: u64 = 2_000;

pub fn get_state(data: &[u8]) -> Result<&LoaderV4State, InstructionError> {
    unsafe {
        let data = data
            .get(0..LoaderV4State::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &[u8; LoaderV4State::program_data_offset()],
            &LoaderV4State,
        >(data))
    }
}

fn get_state_mut(data: &mut [u8]) -> Result<&mut LoaderV4State, InstructionError> {
    unsafe {
        let data = data
            .get_mut(0..LoaderV4State::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &mut [u8; LoaderV4State::program_data_offset()],
            &mut LoaderV4State,
        >(data))
    }
}

pub fn create_program_runtime_environment_v2<'a>(
    compute_budget: &ComputeBudget,
    debugging_features: bool,
) -> BuiltinProgram<InvokeContext<'a>> {
    let config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_address_translation: true, // To be deactivated once we have BTF inference and verification
        enable_stack_frame_gaps: false,
        instruction_meter_checkpoint_distance: 10000,
        enable_instruction_meter: true,
        enable_instruction_tracing: debugging_features,
        enable_symbol_and_section_labels: debugging_features,
        reject_broken_elfs: true,
        noop_instruction_rate: 256,
        sanitize_user_provided_values: true,
        external_internal_function_hash_collision: true,
        reject_callx_r10: true,
        enable_sbpf_v1: false,
        enable_sbpf_v2: true,
        optimize_rodata: true,
        new_elf_parser: true,
        aligned_memory_mapping: true,
        // Warning, do not use `Config::default()` so that configuration here is explicit.
    };
    BuiltinProgram::new_loader(config, FunctionRegistry::default())
}

fn calculate_heap_cost(heap_size: u32, heap_cost: u64) -> u64 {
    const KIBIBYTE: u64 = 1024;
    const PAGE_SIZE_KB: u64 = 32;
    u64::from(heap_size)
        .saturating_add(PAGE_SIZE_KB.saturating_mul(KIBIBYTE).saturating_sub(1))
        .checked_div(PAGE_SIZE_KB.saturating_mul(KIBIBYTE))
        .expect("PAGE_SIZE_KB * KIBIBYTE > 0")
        .saturating_sub(1)
        .saturating_mul(heap_cost)
}

/// Create the SBF virtual machine
pub fn create_vm<'a, 'b>(
    invoke_context: &'a mut InvokeContext<'b>,
    program: &'a Executable<InvokeContext<'b>>,
) -> Result<EbpfVm<'a, InvokeContext<'b>>, Box<dyn std::error::Error>> {
    let config = program.get_config();
    let sbpf_version = program.get_sbpf_version();
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size;
    invoke_context.consume_checked(calculate_heap_cost(heap_size, compute_budget.heap_cost))?;
    let mut stack = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(config.stack_size());
    let mut heap = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(
        usize::try_from(compute_budget.heap_size).unwrap(),
    );
    let stack_len = stack.len();
    let regions: Vec<MemoryRegion> = vec![
        program.get_ro_region(),
        MemoryRegion::new_writable_gapped(stack.as_slice_mut(), ebpf::MM_STACK_START, 0),
        MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
    ];
    let log_collector = invoke_context.get_log_collector();
    let memory_mapping = MemoryMapping::new(regions, config, sbpf_version).map_err(|err| {
        ic_logger_msg!(log_collector, "Failed to create SBF VM: {}", err);
        Box::new(InstructionError::ProgramEnvironmentSetupFailure)
    })?;
    Ok(EbpfVm::new(
        program.get_loader().clone(),
        sbpf_version,
        invoke_context,
        memory_mapping,
        stack_len,
    ))
}

fn execute<'a, 'b: 'a>(
    invoke_context: &'a mut InvokeContext<'b>,
    executable: &'a Executable<InvokeContext<'static>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // We dropped the lifetime tracking in the Executor by setting it to 'static,
    // thus we need to reintroduce the correct lifetime of InvokeContext here again.
    let executable =
        unsafe { std::mem::transmute::<_, &'a Executable<InvokeContext<'b>>>(executable) };
    let log_collector = invoke_context.get_log_collector();
    let stack_height = invoke_context.get_stack_height();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let program_id = *instruction_context.get_last_program_key(transaction_context)?;
    #[cfg(any(target_os = "windows", not(target_arch = "x86_64")))]
    let use_jit = false;
    #[cfg(all(not(target_os = "windows"), target_arch = "x86_64"))]
    let use_jit = executable.get_compiled_program().is_some();

    let compute_meter_prev = invoke_context.get_remaining();
    let mut create_vm_time = Measure::start("create_vm");
    let mut vm = create_vm(invoke_context, executable)?;
    create_vm_time.stop();

    let mut execute_time = Measure::start("execute");
    stable_log::program_invoke(&log_collector, &program_id, stack_height);
    let (compute_units_consumed, result) = vm.execute_program(executable, !use_jit);
    drop(vm);
    ic_logger_msg!(
        log_collector,
        "Program {} consumed {} of {} compute units",
        &program_id,
        compute_units_consumed,
        compute_meter_prev
    );
    execute_time.stop();

    let timings = &mut invoke_context.timings;
    timings.create_vm_us = timings.create_vm_us.saturating_add(create_vm_time.as_us());
    timings.execute_us = timings.execute_us.saturating_add(execute_time.as_us());

    match result {
        ProgramResult::Ok(status) if status != SUCCESS => {
            let error: InstructionError = status.into();
            Err(error.into())
        }
        ProgramResult::Err(error) => Err(error.into()),
        _ => Ok(()),
    }
}

fn check_program_account(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    instruction_context: &InstructionContext,
    program: &BorrowedAccount,
    authority_address: &Pubkey,
) -> Result<LoaderV4State, InstructionError> {
    if !loader_v4::check_id(program.get_owner()) {
        ic_logger_msg!(log_collector, "Program not owned by loader");
        return Err(InstructionError::InvalidAccountOwner);
    }
    if program.get_data().is_empty() {
        ic_logger_msg!(log_collector, "Program is uninitialized");
        return Err(InstructionError::InvalidAccountData);
    }
    let state = get_state(program.get_data())?;
    if !program.is_writable() {
        ic_logger_msg!(log_collector, "Program is not writeable");
        return Err(InstructionError::InvalidArgument);
    }
    if !instruction_context.is_instruction_account_signer(1)? {
        ic_logger_msg!(log_collector, "Authority did not sign");
        return Err(InstructionError::MissingRequiredSignature);
    }
    if state.authority_address != *authority_address {
        ic_logger_msg!(log_collector, "Incorrect authority provided");
        return Err(InstructionError::IncorrectAuthority);
    }
    if matches!(state.status, LoaderV4Status::Finalized) {
        ic_logger_msg!(log_collector, "Program is finalized");
        return Err(InstructionError::Immutable);
    }
    Ok(*state)
}

pub fn process_instruction_write(
    invoke_context: &mut InvokeContext,
    offset: u32,
    bytes: Vec<u8>,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let state = check_program_account(
        &log_collector,
        instruction_context,
        &program,
        authority_address,
    )?;
    if !matches!(state.status, LoaderV4Status::Retracted) {
        ic_logger_msg!(log_collector, "Program is not retracted");
        return Err(InstructionError::InvalidArgument);
    }
    let end_offset = (offset as usize).saturating_add(bytes.len());
    program
        .get_data_mut()?
        .get_mut(
            LoaderV4State::program_data_offset().saturating_add(offset as usize)
                ..LoaderV4State::program_data_offset().saturating_add(end_offset),
        )
        .ok_or_else(|| {
            ic_logger_msg!(log_collector, "Write out of bounds");
            InstructionError::AccountDataTooSmall
        })?
        .copy_from_slice(&bytes);
    Ok(())
}

pub fn process_instruction_truncate(
    invoke_context: &mut InvokeContext,
    new_size: u32,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let is_initialization =
        new_size > 0 && program.get_data().len() < LoaderV4State::program_data_offset();
    if is_initialization {
        if !loader_v4::check_id(program.get_owner()) {
            ic_logger_msg!(log_collector, "Program not owned by loader");
            return Err(InstructionError::InvalidAccountOwner);
        }
        if !program.is_writable() {
            ic_logger_msg!(log_collector, "Program is not writeable");
            return Err(InstructionError::InvalidArgument);
        }
        if !program.is_signer() {
            ic_logger_msg!(log_collector, "Program did not sign");
            return Err(InstructionError::MissingRequiredSignature);
        }
        if !instruction_context.is_instruction_account_signer(1)? {
            ic_logger_msg!(log_collector, "Authority did not sign");
            return Err(InstructionError::MissingRequiredSignature);
        }
    } else {
        let state = check_program_account(
            &log_collector,
            instruction_context,
            &program,
            authority_address,
        )?;
        if !matches!(state.status, LoaderV4Status::Retracted) {
            ic_logger_msg!(log_collector, "Program is not retracted");
            return Err(InstructionError::InvalidArgument);
        }
    }
    let required_lamports = if new_size == 0 {
        0
    } else {
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        rent.minimum_balance(LoaderV4State::program_data_offset().saturating_add(new_size as usize))
    };
    match program.get_lamports().cmp(&required_lamports) {
        std::cmp::Ordering::Less => {
            ic_logger_msg!(
                log_collector,
                "Insufficient lamports, {} are required",
                required_lamports
            );
            return Err(InstructionError::InsufficientFunds);
        }
        std::cmp::Ordering::Greater => {
            let mut recipient =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            if !instruction_context.is_instruction_account_writable(2)? {
                ic_logger_msg!(log_collector, "Recipient is not writeable");
                return Err(InstructionError::InvalidArgument);
            }
            let lamports_to_receive = program.get_lamports().saturating_sub(required_lamports);
            program.checked_sub_lamports(lamports_to_receive)?;
            recipient.checked_add_lamports(lamports_to_receive)?;
        }
        std::cmp::Ordering::Equal => {}
    }
    if new_size == 0 {
        program.set_data_length(0)?;
    } else {
        program.set_data_length(
            LoaderV4State::program_data_offset().saturating_add(new_size as usize),
        )?;
        if is_initialization {
            let state = get_state_mut(program.get_data_mut()?)?;
            state.slot = 0;
            state.status = LoaderV4Status::Retracted;
            state.authority_address = *authority_address;
        }
    }
    Ok(())
}

pub fn process_instruction_deploy(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let source_program = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .ok();
    let state = check_program_account(
        &log_collector,
        instruction_context,
        &program,
        authority_address,
    )?;
    let current_slot = invoke_context.get_sysvar_cache().get_clock()?.slot;
    if state.slot.saturating_add(DEPLOYMENT_COOLDOWN_IN_SLOTS) > current_slot {
        ic_logger_msg!(
            log_collector,
            "Program was deployed recently, cooldown still in effect"
        );
        return Err(InstructionError::InvalidArgument);
    }
    if !matches!(state.status, LoaderV4Status::Retracted) {
        ic_logger_msg!(log_collector, "Destination program is not retracted");
        return Err(InstructionError::InvalidArgument);
    }
    let buffer = if let Some(ref source_program) = source_program {
        let source_state = check_program_account(
            &log_collector,
            instruction_context,
            source_program,
            authority_address,
        )?;
        if !matches!(source_state.status, LoaderV4Status::Retracted) {
            ic_logger_msg!(log_collector, "Source program is not retracted");
            return Err(InstructionError::InvalidArgument);
        }
        source_program
    } else {
        &program
    };

    let programdata = buffer
        .get_data()
        .get(LoaderV4State::program_data_offset()..)
        .ok_or(InstructionError::AccountDataTooSmall)?;

    let deployment_slot = state.slot;
    let effective_slot = deployment_slot.saturating_add(DELAY_VISIBILITY_SLOT_OFFSET);

    let mut load_program_metrics = LoadProgramMetrics {
        program_id: buffer.get_key().to_string(),
        ..LoadProgramMetrics::default()
    };
    let executor = LoadedProgram::new(
        &loader_v4::id(),
        invoke_context
            .programs_modified_by_tx
            .environments
            .program_runtime_v2
            .clone(),
        deployment_slot,
        effective_slot,
        None,
        programdata,
        buffer.get_data().len(),
        &mut load_program_metrics,
    )
    .map_err(|err| {
        ic_logger_msg!(log_collector, "{}", err);
        InstructionError::InvalidAccountData
    })?;
    load_program_metrics.submit_datapoint(&mut invoke_context.timings);
    if let Some(mut source_program) = source_program {
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        let required_lamports = rent.minimum_balance(source_program.get_data().len());
        let transfer_lamports = required_lamports.saturating_sub(program.get_lamports());
        program.set_data_from_slice(source_program.get_data())?;
        source_program.set_data_length(0)?;
        source_program.checked_sub_lamports(transfer_lamports)?;
        program.checked_add_lamports(transfer_lamports)?;
    }
    let state = get_state_mut(program.get_data_mut()?)?;
    state.slot = current_slot;
    state.status = LoaderV4Status::Deployed;

    if let Some(old_entry) = invoke_context.find_program_in_cache(program.get_key()) {
        executor.tx_usage_counter.store(
            old_entry.tx_usage_counter.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        executor.ix_usage_counter.store(
            old_entry.ix_usage_counter.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
    invoke_context
        .programs_modified_by_tx
        .replenish(*program.get_key(), Arc::new(executor));
    Ok(())
}

pub fn process_instruction_retract(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let state = check_program_account(
        &log_collector,
        instruction_context,
        &program,
        authority_address,
    )?;
    let current_slot = invoke_context.get_sysvar_cache().get_clock()?.slot;
    if state.slot.saturating_add(DEPLOYMENT_COOLDOWN_IN_SLOTS) > current_slot {
        ic_logger_msg!(
            log_collector,
            "Program was deployed recently, cooldown still in effect"
        );
        return Err(InstructionError::InvalidArgument);
    }
    if matches!(state.status, LoaderV4Status::Retracted) {
        ic_logger_msg!(log_collector, "Program is not deployed");
        return Err(InstructionError::InvalidArgument);
    }
    let state = get_state_mut(program.get_data_mut()?)?;
    state.status = LoaderV4Status::Retracted;
    Ok(())
}

pub fn process_instruction_transfer_authority(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let new_authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(2)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))
        .ok()
        .cloned();
    let _state = check_program_account(
        &log_collector,
        instruction_context,
        &program,
        authority_address,
    )?;
    if new_authority_address.is_some() && !instruction_context.is_instruction_account_signer(2)? {
        ic_logger_msg!(log_collector, "New authority did not sign");
        return Err(InstructionError::MissingRequiredSignature);
    }
    let state = get_state_mut(program.get_data_mut()?)?;
    if let Some(new_authority_address) = new_authority_address {
        state.authority_address = new_authority_address;
    } else if matches!(state.status, LoaderV4Status::Deployed) {
        state.status = LoaderV4Status::Finalized;
    } else {
        ic_logger_msg!(log_collector, "Program must be deployed to be finalized");
        return Err(InstructionError::InvalidArgument);
    }
    Ok(())
}

declare_builtin_function!(
    Entrypoint,
    fn rust(
        invoke_context: &mut InvokeContext,
        _arg0: u64,
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        process_instruction_inner(invoke_context)
    }
);

pub fn process_instruction_inner(
    invoke_context: &mut InvokeContext,
) -> Result<u64, Box<dyn std::error::Error>> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    if loader_v4::check_id(program_id) {
        if invoke_context
            .feature_set
            .is_active(&feature_set::native_programs_consume_cu::id())
        {
            invoke_context.consume_checked(DEFAULT_COMPUTE_UNITS)?;
        }
        match limited_deserialize(instruction_data)? {
            LoaderV4Instruction::Write { offset, bytes } => {
                process_instruction_write(invoke_context, offset, bytes)
            }
            LoaderV4Instruction::Truncate { new_size } => {
                process_instruction_truncate(invoke_context, new_size)
            }
            LoaderV4Instruction::Deploy => process_instruction_deploy(invoke_context),
            LoaderV4Instruction::Retract => process_instruction_retract(invoke_context),
            LoaderV4Instruction::TransferAuthority => {
                process_instruction_transfer_authority(invoke_context)
            }
        }
        .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
    } else {
        let program = instruction_context.try_borrow_last_program_account(transaction_context)?;
        if !loader_v4::check_id(program.get_owner()) {
            ic_logger_msg!(log_collector, "Program not owned by loader");
            return Err(Box::new(InstructionError::InvalidAccountOwner));
        }
        if program.get_data().is_empty() {
            ic_logger_msg!(log_collector, "Program is uninitialized");
            return Err(Box::new(InstructionError::InvalidAccountData));
        }
        let state = get_state(program.get_data())?;
        if matches!(state.status, LoaderV4Status::Retracted) {
            ic_logger_msg!(log_collector, "Program is not deployed");
            return Err(Box::new(InstructionError::InvalidArgument));
        }
        let mut get_or_create_executor_time = Measure::start("get_or_create_executor_time");
        let loaded_program = invoke_context
            .find_program_in_cache(program.get_key())
            .ok_or_else(|| {
                ic_logger_msg!(log_collector, "Program is not cached");
                InstructionError::InvalidAccountData
            })?;
        get_or_create_executor_time.stop();
        saturating_add_assign!(
            invoke_context.timings.get_or_create_executor_us,
            get_or_create_executor_time.as_us()
        );
        drop(program);
        loaded_program
            .ix_usage_counter
            .fetch_add(1, Ordering::Relaxed);
        match &loaded_program.program {
            LoadedProgramType::FailedVerification(_)
            | LoadedProgramType::Closed
            | LoadedProgramType::DelayVisibility => {
                ic_logger_msg!(log_collector, "Program is not deployed");
                Err(Box::new(InstructionError::InvalidAccountData) as Box<dyn std::error::Error>)
            }
            LoadedProgramType::Typed(executable) => execute(invoke_context, executable),
            _ => Err(Box::new(InstructionError::IncorrectProgramId) as Box<dyn std::error::Error>),
        }
    }
    .map(|_| 0)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_sdk::{
            account::{
                create_account_shared_data_for_test, AccountSharedData, ReadableAccount,
                WritableAccount,
            },
            instruction::AccountMeta,
            slot_history::Slot,
            sysvar::{clock, rent},
            transaction_context::IndexOfAccount,
        },
        std::{fs::File, io::Read, path::Path},
    };

    pub fn load_all_invoked_programs(invoke_context: &mut InvokeContext) {
        let mut load_program_metrics = LoadProgramMetrics::default();
        let num_accounts = invoke_context.transaction_context.get_number_of_accounts();
        for index in 0..num_accounts {
            let account = invoke_context
                .transaction_context
                .get_account_at_index(index)
                .expect("Failed to get the account")
                .borrow();

            let owner = account.owner();
            if loader_v4::check_id(owner) {
                let pubkey = invoke_context
                    .transaction_context
                    .get_key_of_account_at_index(index)
                    .expect("Failed to get account key");

                if let Some(programdata) =
                    account.data().get(LoaderV4State::program_data_offset()..)
                {
                    if let Ok(loaded_program) = LoadedProgram::new(
                        &loader_v4::id(),
                        invoke_context
                            .programs_modified_by_tx
                            .environments
                            .program_runtime_v2
                            .clone(),
                        0,
                        0,
                        None,
                        programdata,
                        account.data().len(),
                        &mut load_program_metrics,
                    ) {
                        invoke_context.programs_modified_by_tx.set_slot_for_tests(0);
                        invoke_context
                            .programs_modified_by_tx
                            .replenish(*pubkey, Arc::new(loaded_program));
                    }
                }
            }
        }
    }

    fn process_instruction(
        program_indices: Vec<IndexOfAccount>,
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: &[(IndexOfAccount, bool, bool)],
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        let instruction_accounts = instruction_accounts
            .iter()
            .map(
                |(index_in_transaction, is_signer, is_writable)| AccountMeta {
                    pubkey: transaction_accounts[*index_in_transaction as usize].0,
                    is_signer: *is_signer,
                    is_writable: *is_writable,
                },
            )
            .collect::<Vec<_>>();
        mock_process_instruction(
            &loader_v4::id(),
            program_indices,
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            expected_result,
            Entrypoint::vm,
            |invoke_context| {
                invoke_context
                    .programs_modified_by_tx
                    .environments
                    .program_runtime_v2 = Arc::new(create_program_runtime_environment_v2(
                    &ComputeBudget::default(),
                    false,
                ));
                load_all_invoked_programs(invoke_context);
            },
            |_invoke_context| {},
        )
    }

    fn load_program_account_from_elf(
        authority_address: Pubkey,
        status: LoaderV4Status,
        path: &str,
    ) -> AccountSharedData {
        let path = Path::new("test_elfs/out/").join(path).with_extension("so");
        let mut file = File::open(path).expect("file open failed");
        let mut elf_bytes = Vec::new();
        file.read_to_end(&mut elf_bytes).unwrap();
        let rent = rent::Rent::default();
        let account_size =
            loader_v4::LoaderV4State::program_data_offset().saturating_add(elf_bytes.len());
        let mut program_account = AccountSharedData::new(
            rent.minimum_balance(account_size),
            account_size,
            &loader_v4::id(),
        );
        let state = get_state_mut(program_account.data_as_mut_slice()).unwrap();
        state.slot = 0;
        state.authority_address = authority_address;
        state.status = status;
        program_account.data_as_mut_slice()[loader_v4::LoaderV4State::program_data_offset()..]
            .copy_from_slice(&elf_bytes);
        program_account
    }

    fn clock(slot: Slot) -> AccountSharedData {
        let clock = clock::Clock {
            slot,
            ..clock::Clock::default()
        };
        create_account_shared_data_for_test(&clock)
    }

    fn test_loader_instruction_general_errors(instruction: LoaderV4Instruction) {
        let instruction = bincode::serialize(&instruction).unwrap();
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Deployed,
                    "relative_call",
                ),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Finalized,
                    "relative_call",
                ),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&clock::Clock::default()),
            ),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Error: Missing program account
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Error: Missing authority account
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[(0, false, true)],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Error: Program not owned by loader
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[(1, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidAccountOwner),
        );

        // Error: Program is not writeable
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[(0, false, false), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Authority did not sign
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[(0, false, true), (1, false, false), (2, true, true)],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Error: Program is finalized
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts.clone(),
            &[(2, false, true), (1, true, false), (0, true, true)],
            Err(InstructionError::Immutable),
        );

        // Error: Incorrect authority provided
        process_instruction(
            vec![],
            &instruction,
            transaction_accounts,
            &[(0, false, true), (2, true, false), (2, true, true)],
            Err(InstructionError::IncorrectAuthority),
        );
    }

    #[test]
    fn test_loader_instruction_write() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "relative_call",
                ),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Deployed,
                    "relative_call",
                ),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&clock::Clock::default()),
            ),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Overwrite existing data
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Write {
                offset: 2,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );

        // Empty write
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Write {
                offset: 2,
                bytes: Vec::new(),
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );

        // Error: Program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(2, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Write out of bounds
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Write {
                offset: transaction_accounts[0]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    .saturating_sub(3) as u32,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::AccountDataTooSmall),
        );

        test_loader_instruction_general_errors(LoaderV4Instruction::Write {
            offset: 0,
            bytes: Vec::new(),
        });
    }

    #[test]
    fn test_loader_instruction_truncate() {
        let authority_address = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "relative_call",
                ),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &loader_v4::id()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(40000000, 0, &loader_v4::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "rodata_section",
                ),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Deployed,
                    "relative_call",
                ),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&clock::Clock::default()),
            ),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // No change
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate {
                new_size: transaction_accounts[0]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    as u32,
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[2].lamports(), transaction_accounts[2].1.lamports());
        let lamports = transaction_accounts[4].1.lamports();
        transaction_accounts[0].1.set_lamports(lamports);

        // Initialize program account
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate {
                new_size: transaction_accounts[0]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    as u32,
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(3, true, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        assert_eq!(
            accounts[3].data().len(),
            transaction_accounts[0].1.data().len(),
        );

        // Increase program account size
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate {
                new_size: transaction_accounts[4]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    as u32,
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[4].1.data().len(),
        );

        // Decrease program account size
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate {
                new_size: transaction_accounts[0]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    as u32,
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(4, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        assert_eq!(
            accounts[4].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[2].1.lamports().saturating_add(
                transaction_accounts[4]
                    .1
                    .lamports()
                    .saturating_sub(accounts[4].lamports())
            ),
        );

        // Close program account
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        assert_eq!(accounts[0].data().len(), 0);
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[2].1.lamports().saturating_add(
                transaction_accounts[0]
                    .1
                    .lamports()
                    .saturating_sub(accounts[0].lamports())
            ),
        );

        // Error: Program not owned by loader
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 8 }).unwrap(),
            transaction_accounts.clone(),
            &[(1, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidAccountOwner),
        );

        // Error: Program is not writeable
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 8 }).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, false), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 8 }).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Error: Authority did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 8 }).unwrap(),
            transaction_accounts.clone(),
            &[(3, true, true), (1, false, false), (2, true, true)],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Error: Program is and stays uninitialized
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 8 }).unwrap(),
            transaction_accounts.clone(),
            &[(5, false, true), (1, true, false), (2, false, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Missing recipient account
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(0, true, true), (1, true, false)],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Error: Recipient is not writeable
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate { new_size: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Insufficient funds
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Truncate {
                new_size: transaction_accounts[4]
                    .1
                    .data()
                    .len()
                    .saturating_sub(loader_v4::LoaderV4State::program_data_offset())
                    .saturating_add(1) as u32,
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InsufficientFunds),
        );

        test_loader_instruction_general_errors(LoaderV4Instruction::Truncate { new_size: 0 });
    }

    #[test]
    fn test_loader_instruction_deploy() {
        let authority_address = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "rodata_section",
                ),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "relative_call",
                ),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &loader_v4::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "invalid",
                ),
            ),
            (clock::id(), clock(1000)),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Deploy from its own data
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        transaction_accounts[0].1 = accounts[0].clone();
        transaction_accounts[5].1 = clock(2000);
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: Source program is not writable
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Source program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(2, false, true), (1, true, false), (0, false, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Redeploy: Retract, then replace data by other source
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        transaction_accounts[0].1 = accounts[0].clone();
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        transaction_accounts[0].1 = accounts[0].clone();
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[2].1.data().len(),
        );
        assert_eq!(accounts[2].data().len(), 0,);
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[2].1.lamports().saturating_sub(
                accounts[0]
                    .lamports()
                    .saturating_sub(transaction_accounts[0].1.lamports())
            ),
        );

        // Error: Program was deployed recently, cooldown still in effect
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );
        transaction_accounts[5].1 = clock(3000);

        // Error: Program is uninitialized
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program fails verification
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(4, false, true), (1, true, false)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program is deployed already
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        test_loader_instruction_general_errors(LoaderV4Instruction::Deploy);
    }

    #[test]
    fn test_loader_instruction_retract() {
        let authority_address = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Deployed,
                    "rodata_section",
                ),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &loader_v4::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "rodata_section",
                ),
            ),
            (clock::id(), clock(1000)),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Retract program
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: Program is uninitialized
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(2, false, true), (1, true, false)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program is not deployed
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program was deployed recently, cooldown still in effect
        transaction_accounts[4].1 = clock(0);
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        test_loader_instruction_general_errors(LoaderV4Instruction::Retract);
    }

    #[test]
    fn test_loader_instruction_transfer_authority() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Deployed,
                    "rodata_section",
                ),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "rodata_section",
                ),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &loader_v4::id()),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                clock::id(),
                create_account_shared_data_for_test(&clock::Clock::default()),
            ),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Transfer authority
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (3, true, false), (4, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Finalize program
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (3, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: Program must be deployed to be finalized
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(1, false, true), (3, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program is uninitialized
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(2, false, true), (3, true, false), (4, true, false)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: New authority did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&LoaderV4Instruction::TransferAuthority).unwrap(),
            transaction_accounts,
            &[(0, false, true), (3, true, false), (4, false, false)],
            Err(InstructionError::MissingRequiredSignature),
        );

        test_loader_instruction_general_errors(LoaderV4Instruction::TransferAuthority);
    }

    #[test]
    fn test_execute_program() {
        let program_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                program_address,
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Finalized,
                    "rodata_section",
                ),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(10000000, 32, &program_address),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &loader_v4::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Retracted,
                    "rodata_section",
                ),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(
                    authority_address,
                    LoaderV4Status::Finalized,
                    "invalid",
                ),
            ),
        ];

        // Execute program
        process_instruction(
            vec![0],
            &[0, 1, 2, 3],
            transaction_accounts.clone(),
            &[(1, false, true)],
            Err(InstructionError::Custom(42)),
        );

        // Error: Program not owned by loader
        process_instruction(
            vec![1],
            &[0, 1, 2, 3],
            transaction_accounts.clone(),
            &[(1, false, true)],
            Err(InstructionError::InvalidAccountOwner),
        );

        // Error: Program is uninitialized
        process_instruction(
            vec![2],
            &[0, 1, 2, 3],
            transaction_accounts.clone(),
            &[(1, false, true)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program is not deployed
        process_instruction(
            vec![3],
            &[0, 1, 2, 3],
            transaction_accounts.clone(),
            &[(1, false, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program fails verification
        process_instruction(
            vec![4],
            &[0, 1, 2, 3],
            transaction_accounts,
            &[(1, false, true)],
            Err(InstructionError::InvalidAccountData),
        );
    }
}
