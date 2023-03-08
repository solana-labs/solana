use {
    rand::Rng,
    solana_measure::measure::Measure,
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        ic_logger_msg,
        invoke_context::InvokeContext,
        loaded_programs::{LoadProgramMetrics, LoadedProgram, LoadedProgramType},
        log_collector::LogCollector,
        stable_log,
    },
    solana_rbpf::{
        aligned_memory::AlignedMemory,
        ebpf,
        memory_region::{MemoryMapping, MemoryRegion},
        verifier::RequisiteVerifier,
        vm::{
            BuiltInProgram, Config, ContextObject, EbpfVm, ProgramResult, VerifiedExecutable,
            PROGRAM_ENVIRONMENT_KEY_SHIFT,
        },
    },
    solana_sdk::{
        entrypoint::{HEAP_LENGTH, SUCCESS},
        feature_set::FeatureSet,
        instruction::InstructionError,
        loader_v3::{self, LoaderV3State, DEPLOYMENT_COOLDOWN_IN_SLOTS},
        loader_v3_instruction::LoaderV3Instruction,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        saturating_add_assign,
        transaction_context::{BorrowedAccount, InstructionContext},
    },
    std::{cell::RefCell, rc::Rc, sync::Arc},
};

fn get_state(data: &[u8]) -> Result<&LoaderV3State, InstructionError> {
    unsafe {
        let data = data
            .get(0..LoaderV3State::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &[u8; LoaderV3State::program_data_offset()],
            &LoaderV3State,
        >(data))
    }
}

fn get_state_mut(data: &mut [u8]) -> Result<&mut LoaderV3State, InstructionError> {
    unsafe {
        let data = data
            .get_mut(0..LoaderV3State::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &mut [u8; LoaderV3State::program_data_offset()],
            &mut LoaderV3State,
        >(data))
    }
}

pub fn load_program_from_account(
    _feature_set: &FeatureSet,
    compute_budget: &ComputeBudget,
    log_collector: Option<Rc<RefCell<LogCollector>>>,
    program: &BorrowedAccount,
    use_jit: bool,
    debugging_features: bool,
) -> Result<(Arc<LoadedProgram>, LoadProgramMetrics), InstructionError> {
    let mut load_program_metrics = LoadProgramMetrics {
        program_id: program.get_key().to_string(),
        ..LoadProgramMetrics::default()
    };
    let config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_stack_frame_gaps: false,
        instruction_meter_checkpoint_distance: 10000,
        enable_instruction_meter: true,
        enable_instruction_tracing: debugging_features,
        enable_symbol_and_section_labels: debugging_features,
        reject_broken_elfs: true,
        noop_instruction_rate: 256,
        sanitize_user_provided_values: true,
        runtime_environment_key: rand::thread_rng()
            .gen::<i32>()
            .checked_shr(PROGRAM_ENVIRONMENT_KEY_SHIFT)
            .unwrap_or(0),
        external_internal_function_hash_collision: true,
        reject_callx_r10: true,
        dynamic_stack_frames: true,
        enable_sdiv: true,
        optimize_rodata: true,
        static_syscalls: true,
        enable_elf_vaddr: true,
        reject_rodata_stack_overlap: true,
        new_elf_parser: true,
        aligned_memory_mapping: true,
        // Warning, do not use `Config::default()` so that configuration here is explicit.
    };
    let loader = BuiltInProgram::new_loader(config);
    let state = get_state(program.get_data())?;
    let programdata = program
        .get_data()
        .get(LoaderV3State::program_data_offset()..)
        .ok_or(InstructionError::AccountDataTooSmall)?;
    let loaded_program = LoadedProgram::new(
        &loader_v3::id(),
        Arc::new(loader),
        state.slot,
        programdata,
        program.get_data().len(),
        use_jit,
        &mut load_program_metrics,
    )
    .map_err(|err| {
        ic_logger_msg!(log_collector, "{}", err);
        InstructionError::InvalidAccountData
    })?;
    Ok((Arc::new(loaded_program), load_program_metrics))
}

fn calculate_heap_cost(heap_size: u64, heap_cost: u64) -> u64 {
    const KIBIBYTE: u64 = 1024;
    const PAGE_SIZE_KB: u64 = 32;
    heap_size
        .saturating_add(PAGE_SIZE_KB.saturating_mul(KIBIBYTE).saturating_sub(1))
        .saturating_div(PAGE_SIZE_KB.saturating_mul(KIBIBYTE))
        .saturating_sub(1)
        .saturating_mul(heap_cost)
}

/// Create the SBF virtual machine
pub fn create_vm<'a, 'b>(
    invoke_context: &'a mut InvokeContext<'b>,
    program: &'a VerifiedExecutable<RequisiteVerifier, InvokeContext<'b>>,
) -> Result<EbpfVm<'a, RequisiteVerifier, InvokeContext<'b>>, InstructionError> {
    let config = program.get_executable().get_config();
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size.unwrap_or(HEAP_LENGTH);
    invoke_context.consume_checked(calculate_heap_cost(
        heap_size as u64,
        compute_budget.heap_cost,
    ))?;
    let mut stack = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(config.stack_size());
    let mut heap = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(
        compute_budget.heap_size.unwrap_or(HEAP_LENGTH),
    );
    let stack_len = stack.len();
    let regions: Vec<MemoryRegion> = vec![
        program.get_executable().get_ro_region(),
        MemoryRegion::new_writable_gapped(stack.as_slice_mut(), ebpf::MM_STACK_START, 0),
        MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
    ];
    let log_collector = invoke_context.get_log_collector();
    MemoryMapping::new(regions, config)
        .and_then(|memory_mapping| EbpfVm::new(program, invoke_context, memory_mapping, stack_len))
        .map_err(|err| {
            ic_logger_msg!(log_collector, "Failed to create SBF VM: {}", err);
            InstructionError::ProgramEnvironmentSetupFailure
        })
}

fn execute(
    invoke_context: &mut InvokeContext,
    program: &VerifiedExecutable<RequisiteVerifier, InvokeContext<'static>>,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let stack_height = invoke_context.get_stack_height();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let program_id = *instruction_context.get_last_program_key(transaction_context)?;
    #[cfg(any(target_os = "windows", not(target_arch = "x86_64")))]
    let use_jit = false;
    #[cfg(all(not(target_os = "windows"), target_arch = "x86_64"))]
    let use_jit = program.get_executable().get_compiled_program().is_some();

    let compute_meter_prev = invoke_context.get_remaining();
    let mut create_vm_time = Measure::start("create_vm");
    let mut vm = create_vm(
        invoke_context,
        // We dropped the lifetime tracking in the Executor by setting it to 'static,
        // thus we need to reintroduce the correct lifetime of InvokeContext here again.
        unsafe { std::mem::transmute(program) },
    )?;
    create_vm_time.stop();

    let mut execute_time = Measure::start("execute");
    stable_log::program_invoke(&log_collector, &program_id, stack_height);
    let (compute_units_consumed, result) = vm.execute_program(!use_jit);
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
            let error = status.into();
            Err(error)
        }
        ProgramResult::Err(_) => Err(InstructionError::ProgramFailedToComplete),
        _ => Ok(()),
    }
}

fn check_program_account(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    instruction_context: &InstructionContext,
    program: &BorrowedAccount,
    authority_address: &Pubkey,
) -> Result<LoaderV3State, InstructionError> {
    if !loader_v3::check_id(program.get_owner()) {
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
    if state.authority_address.is_none() {
        ic_logger_msg!(log_collector, "Program is finalized");
        return Err(InstructionError::Immutable);
    }
    if state.authority_address != Some(*authority_address) {
        ic_logger_msg!(log_collector, "Incorrect authority provided");
        return Err(InstructionError::IncorrectAuthority);
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
    let mut payer = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .ok();
    let is_initialization = offset == 0 && program.get_data().is_empty();
    if is_initialization {
        if !loader_v3::check_id(program.get_owner()) {
            ic_logger_msg!(log_collector, "Program not owned by loader");
            return Err(InstructionError::InvalidAccountOwner);
        }
        if !program.is_writable() {
            ic_logger_msg!(log_collector, "Program is not writeable");
            return Err(InstructionError::InvalidArgument);
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
        if state.is_deployed {
            ic_logger_msg!(log_collector, "Program is not retracted");
            return Err(InstructionError::InvalidArgument);
        }
    }
    if payer.is_some() && !instruction_context.is_instruction_account_signer(2)? {
        ic_logger_msg!(log_collector, "Payer did not sign");
        return Err(InstructionError::MissingRequiredSignature);
    }
    if payer.is_some() && !instruction_context.is_instruction_account_writable(2)? {
        ic_logger_msg!(log_collector, "Payer is not writeable");
        return Err(InstructionError::InvalidArgument);
    }
    let program_size = program
        .get_data()
        .len()
        .saturating_sub(LoaderV3State::program_data_offset());
    if offset as usize > program_size {
        ic_logger_msg!(log_collector, "Write out of bounds");
        return Err(InstructionError::AccountDataTooSmall);
    }
    let end_offset = (offset as usize).saturating_add(bytes.len());
    let rent = invoke_context.get_sysvar_cache().get_rent()?;
    let required_lamports =
        rent.minimum_balance(LoaderV3State::program_data_offset().saturating_add(end_offset));
    let transfer_lamports = required_lamports.saturating_sub(program.get_lamports());
    if transfer_lamports > 0 {
        payer = payer.filter(|payer| payer.get_lamports() >= transfer_lamports);
        if payer.is_none() {
            ic_logger_msg!(
                log_collector,
                "Insufficient lamports, {} are required",
                required_lamports
            );
            return Err(InstructionError::InsufficientFunds);
        }
    }
    if end_offset > program_size {
        program.set_data_length(LoaderV3State::program_data_offset().saturating_add(end_offset))?;
    }
    if let Some(mut payer) = payer {
        payer.checked_sub_lamports(transfer_lamports)?;
        program.checked_add_lamports(transfer_lamports)?;
    }
    if is_initialization {
        let state = get_state_mut(program.get_data_mut()?)?;
        state.slot = invoke_context.get_sysvar_cache().get_clock()?.slot;
        state.is_deployed = false;
        state.authority_address = Some(*authority_address);
    }
    program
        .get_data_mut()?
        .get_mut(
            LoaderV3State::program_data_offset().saturating_add(offset as usize)
                ..LoaderV3State::program_data_offset().saturating_add(end_offset),
        )
        .ok_or(InstructionError::AccountDataTooSmall)?
        .copy_from_slice(&bytes);
    Ok(())
}

pub fn process_instruction_truncate(
    invoke_context: &mut InvokeContext,
    offset: u32,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let authority_address = instruction_context
        .get_index_of_instruction_account_in_transaction(1)
        .and_then(|index| transaction_context.get_key_of_account_at_index(index))?;
    let mut recipient =
        instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
    let state = check_program_account(
        &log_collector,
        instruction_context,
        &program,
        authority_address,
    )?;
    if state.is_deployed {
        ic_logger_msg!(log_collector, "Program is not retracted");
        return Err(InstructionError::InvalidArgument);
    }
    let program_size = program
        .get_data()
        .len()
        .saturating_sub(LoaderV3State::program_data_offset());
    if offset as usize > program_size {
        ic_logger_msg!(log_collector, "Truncate out of bounds");
        return Err(InstructionError::AccountDataTooSmall);
    }
    let required_lamports = if offset == 0 {
        program.set_data_length(0)?;
        0
    } else {
        program.set_data_length(
            LoaderV3State::program_data_offset().saturating_add(offset as usize),
        )?;
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        rent.minimum_balance(program.get_data().len())
    };
    let transfer_lamports = program.get_lamports().saturating_sub(required_lamports);
    program.checked_sub_lamports(transfer_lamports)?;
    recipient.checked_add_lamports(transfer_lamports)?;
    Ok(())
}

pub fn process_instruction_deploy(
    invoke_context: &mut InvokeContext,
    use_jit: bool,
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
    if state.is_deployed {
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
        if source_state.is_deployed {
            ic_logger_msg!(log_collector, "Source program is not retracted");
            return Err(InstructionError::InvalidArgument);
        }
        source_program
    } else {
        &program
    };
    let (_executor, load_program_metrics) = load_program_from_account(
        &invoke_context.feature_set,
        invoke_context.get_compute_budget(),
        invoke_context.get_log_collector(),
        buffer,
        use_jit,
        false,
    )?;
    load_program_metrics.submit_datapoint(&mut invoke_context.timings);
    if let Some(mut source_program) = source_program {
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        let required_lamports = rent.minimum_balance(program.get_data().len());
        let transfer_lamports = program.get_lamports().saturating_sub(required_lamports);
        program.set_data_from_slice(source_program.get_data())?;
        source_program.set_data_length(0)?;
        source_program.checked_sub_lamports(transfer_lamports)?;
        program.checked_add_lamports(transfer_lamports)?;
    }
    let state = get_state_mut(program.get_data_mut()?)?;
    state.slot = current_slot;
    state.is_deployed = true;
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
    if !state.is_deployed {
        ic_logger_msg!(log_collector, "Program is not deployed");
        return Err(InstructionError::InvalidArgument);
    }
    let state = get_state_mut(program.get_data_mut()?)?;
    state.is_deployed = false;
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
    state.authority_address = new_authority_address;
    Ok(())
}

pub fn process_instruction(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    let use_jit = true;
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    if loader_v3::check_id(program_id) {
        match limited_deserialize(instruction_data)? {
            LoaderV3Instruction::Write { offset, bytes } => {
                process_instruction_write(invoke_context, offset, bytes)
            }
            LoaderV3Instruction::Truncate { offset } => {
                process_instruction_truncate(invoke_context, offset)
            }
            LoaderV3Instruction::Deploy => process_instruction_deploy(invoke_context, use_jit),
            LoaderV3Instruction::Retract => process_instruction_retract(invoke_context),
            LoaderV3Instruction::TransferAuthority => {
                process_instruction_transfer_authority(invoke_context)
            }
        }
    } else {
        let program = instruction_context.try_borrow_last_program_account(transaction_context)?;
        if !loader_v3::check_id(program.get_owner()) {
            ic_logger_msg!(log_collector, "Program not owned by loader");
            return Err(InstructionError::InvalidAccountOwner);
        }
        if program.get_data().is_empty() {
            ic_logger_msg!(log_collector, "Program is uninitialized");
            return Err(InstructionError::InvalidAccountData);
        }
        let state = get_state(program.get_data())?;
        if !state.is_deployed {
            ic_logger_msg!(log_collector, "Program is not deployed");
            return Err(InstructionError::InvalidArgument);
        }
        let mut get_or_create_executor_time = Measure::start("get_or_create_executor_time");
        let (loaded_program, load_program_metrics) = load_program_from_account(
            &invoke_context.feature_set,
            invoke_context.get_compute_budget(),
            invoke_context.get_log_collector(),
            &program,
            use_jit,
            false,
        )?;
        load_program_metrics.submit_datapoint(&mut invoke_context.timings);
        get_or_create_executor_time.stop();
        saturating_add_assign!(
            invoke_context.timings.get_or_create_executor_us,
            get_or_create_executor_time.as_us()
        );
        drop(program);
        match &loaded_program.program {
            LoadedProgramType::Invalid => Err(InstructionError::InvalidAccountData),
            LoadedProgramType::Typed(executable) => execute(invoke_context, executable),
            _ => Err(InstructionError::IncorrectProgramId),
        }
    }
}
