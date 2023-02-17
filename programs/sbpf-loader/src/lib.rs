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
        ebpf::HOST_ALIGN,
        memory_region::MemoryRegion,
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
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        saturating_add_assign,
        sbpf_loader::{self, SbfLoaderState, DEPLOYMENT_COOLDOWN_IN_SLOTS},
        sbpf_loader_instruction::SbfLoaderInstruction,
        transaction_context::{BorrowedAccount, IndexOfAccount, InstructionContext},
    },
    std::{cell::RefCell, rc::Rc, sync::Arc},
};

fn get_state(data: &[u8]) -> Result<&SbfLoaderState, InstructionError> {
    unsafe {
        let data = data
            .get(0..SbfLoaderState::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &[u8; SbfLoaderState::program_data_offset()],
            &SbfLoaderState,
        >(data))
    }
}

fn get_state_mut(data: &mut [u8]) -> Result<&mut SbfLoaderState, InstructionError> {
    unsafe {
        let data = data
            .get_mut(0..SbfLoaderState::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into()
            .unwrap();
        Ok(std::mem::transmute::<
            &mut [u8; SbfLoaderState::program_data_offset()],
            &mut SbfLoaderState,
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
        enable_stack_frame_gaps: true,
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
        .get(SbfLoaderState::program_data_offset()..)
        .ok_or(InstructionError::AccountDataTooSmall)?;
    let loaded_program = LoadedProgram::new(
        &sbpf_loader::id(),
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

/// Create the SBF virtual machine
pub fn create_vm<'a, 'b>(
    invoke_context: &'a mut InvokeContext<'b>,
    program: &'a VerifiedExecutable<RequisiteVerifier, InvokeContext<'b>>,
    regions: Vec<MemoryRegion>,
) -> Result<EbpfVm<'a, RequisiteVerifier, InvokeContext<'b>>, InstructionError> {
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size.unwrap_or(HEAP_LENGTH);
    invoke_context.consume_checked(
        ((heap_size as u64).saturating_div(32_u64.saturating_mul(1024)))
            .saturating_sub(1)
            .saturating_mul(compute_budget.heap_cost),
    )?;
    let mut heap =
        AlignedMemory::<HOST_ALIGN>::zero_filled(compute_budget.heap_size.unwrap_or(HEAP_LENGTH));
    let log_collector = invoke_context.get_log_collector();
    EbpfVm::new(program, invoke_context, heap.as_slice_mut(), regions).map_err(|err| {
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
        Vec::new(),
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
            stable_log::program_failure(&log_collector, &program_id, &error);
            Err(error)
        }
        ProgramResult::Err(error) => {
            stable_log::program_failure(&log_collector, &program_id, &error);
            Err(InstructionError::ProgramFailedToComplete)
        }
        _ => {
            stable_log::program_success(&log_collector, &program_id);
            Ok(())
        }
    }
}

fn check_program_account(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    instruction_context: &InstructionContext,
    program: &BorrowedAccount,
    authority_address: &Pubkey,
) -> Result<SbfLoaderState, InstructionError> {
    if !sbpf_loader::check_id(program.get_owner()) {
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

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let use_jit = true;
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    if sbpf_loader::check_id(program_id) {
        match limited_deserialize(instruction_data)? {
            SbfLoaderInstruction::Write { offset, bytes } => {
                let mut program =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                let authority_address = transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?;
                let mut payer = instruction_context
                    .try_borrow_instruction_account(transaction_context, 2)
                    .ok();
                let is_initialization = offset == 0 && program.get_data().is_empty();
                if is_initialization {
                    if !sbpf_loader::check_id(program.get_owner()) {
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
                    .saturating_sub(SbfLoaderState::program_data_offset());
                if offset as usize > program_size {
                    ic_logger_msg!(log_collector, "Write out of bounds");
                    return Err(InstructionError::AccountDataTooSmall);
                }
                let end_offset = (offset as usize).saturating_add(bytes.len());
                let rent = invoke_context.get_sysvar_cache().get_rent()?;
                let required_lamports = rent.minimum_balance(
                    SbfLoaderState::program_data_offset().saturating_add(end_offset),
                );
                let transfer_lamports = required_lamports.saturating_sub(program.get_lamports());
                if transfer_lamports > 0 {
                    payer = payer.filter(|payer| payer.get_lamports() >= transfer_lamports);
                    if payer.is_none() {
                        ic_logger_msg!(
                            log_collector,
                            "Insufficient lamports {} needed {}",
                            payer
                                .as_ref()
                                .map(|payer| payer.get_lamports())
                                .unwrap_or(0),
                            required_lamports
                        );
                        return Err(InstructionError::InsufficientFunds);
                    }
                }
                if end_offset > program_size {
                    program.set_data_length(
                        SbfLoaderState::program_data_offset().saturating_add(end_offset),
                    )?;
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
                        SbfLoaderState::program_data_offset().saturating_add(offset as usize)
                            ..SbfLoaderState::program_data_offset().saturating_add(end_offset),
                    )
                    .ok_or(InstructionError::AccountDataTooSmall)?
                    .copy_from_slice(&bytes);
            }
            SbfLoaderInstruction::Truncate { offset } => {
                let mut program =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                let authority_address = transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?;
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
                    .saturating_sub(SbfLoaderState::program_data_offset());
                if offset as usize > program_size {
                    ic_logger_msg!(log_collector, "Truncate out of bounds");
                    return Err(InstructionError::AccountDataTooSmall);
                }
                let required_lamports = if offset == 0 {
                    program.set_data_length(0)?;
                    0
                } else {
                    program.set_data_length(
                        SbfLoaderState::program_data_offset().saturating_add(offset as usize),
                    )?;
                    let rent = invoke_context.get_sysvar_cache().get_rent()?;
                    rent.minimum_balance(program.get_data().len())
                };
                let transfer_lamports = program.get_lamports().saturating_sub(required_lamports);
                program.checked_sub_lamports(transfer_lamports)?;
                recipient.checked_add_lamports(transfer_lamports)?;
            }
            SbfLoaderInstruction::Deploy => {
                let mut program =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                let authority_address = transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?;
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
                    if state.is_deployed {
                        ic_logger_msg!(log_collector, "Program is deployed already");
                        return Err(InstructionError::InvalidArgument);
                    }
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
                    let transfer_lamports =
                        program.get_lamports().saturating_sub(required_lamports);
                    program.set_data_from_slice(source_program.get_data())?;
                    source_program.set_data_length(0)?;
                    source_program.checked_sub_lamports(transfer_lamports)?;
                    program.checked_add_lamports(transfer_lamports)?;
                }
                let state = get_state_mut(program.get_data_mut()?)?;
                state.slot = current_slot;
                state.is_deployed = true;
            }
            SbfLoaderInstruction::Retract => {
                let mut program =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                let authority_address = transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?;
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
                state.slot = current_slot;
                state.is_deployed = false;
            }
            SbfLoaderInstruction::TransferAuthority => {
                let mut program =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                let authority_address = transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?;
                let new_authority_address = instruction_context
                    .get_index_of_instruction_account_in_transaction(2)
                    .and_then(|index_in_transaction| {
                        transaction_context.get_key_of_account_at_index(index_in_transaction)
                    })
                    .ok()
                    .cloned();
                let _state = check_program_account(
                    &log_collector,
                    instruction_context,
                    &program,
                    authority_address,
                )?;
                if new_authority_address.is_some()
                    && !instruction_context.is_instruction_account_signer(2)?
                {
                    ic_logger_msg!(log_collector, "New authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
                let state = get_state_mut(program.get_data_mut()?)?;
                state.authority_address = new_authority_address;
            }
        }
        Ok(())
    } else {
        let program = instruction_context.try_borrow_last_program_account(transaction_context)?;
        if !sbpf_loader::check_id(program.get_owner()) {
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            account::{
                create_account_shared_data_for_test, AccountSharedData, ReadableAccount,
                WritableAccount,
            },
            account_utils::StateMut,
            native_loader,
            sysvar::{clock, rent},
            transaction_context::{IndexOfAccount, InstructionAccount, TransactionContext},
        },
        std::{fs::File, io::Read, path::Path},
    };

    fn process_instruction(
        mut program_indices: Vec<IndexOfAccount>,
        instruction_data: &[u8],
        mut transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: &[(IndexOfAccount, bool, bool)],
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        program_indices.insert(0, transaction_accounts.len() as IndexOfAccount);
        let processor_account = AccountSharedData::new(0, 0, &native_loader::id());
        transaction_accounts.push((sbpf_loader::id(), processor_account));
        let compute_budget = ComputeBudget::default();
        let mut transaction_context = TransactionContext::new(
            transaction_accounts,
            Some(rent::Rent::default()),
            compute_budget.max_invoke_stack_height,
            compute_budget.max_instruction_trace_length,
        );
        transaction_context.enable_cap_accounts_data_allocations_per_transaction();
        let instruction_accounts = instruction_accounts
            .iter()
            .enumerate()
            .map(
                |(instruction_account_index, (index_in_transaction, is_signer, is_writable))| {
                    InstructionAccount {
                        index_in_transaction: *index_in_transaction,
                        index_in_caller: *index_in_transaction,
                        index_in_callee: instruction_account_index as IndexOfAccount,
                        is_signer: *is_signer,
                        is_writable: *is_writable,
                    }
                },
            )
            .collect::<Vec<_>>();
        let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        invoke_context
            .transaction_context
            .get_next_instruction_context()
            .unwrap()
            .configure(&program_indices, &instruction_accounts, instruction_data);
        let result = invoke_context
            .push()
            .and_then(|_| super::process_instruction(1, &mut invoke_context));
        let pop_result = invoke_context.pop();
        assert_eq!(result.and(pop_result), expected_result);
        let mut transaction_accounts = transaction_context.deconstruct_without_keys().unwrap();
        transaction_accounts.pop();
        transaction_accounts
    }

    fn load_program_account_from_elf(
        is_deployed: bool,
        authority_address: Option<Pubkey>,
        path: &str,
    ) -> AccountSharedData {
        let path = Path::new("test_elfs/out/").join(path).with_extension("so");
        let mut file = File::open(path).expect("file open failed");
        let mut elf_bytes = Vec::new();
        file.read_to_end(&mut elf_bytes).unwrap();
        let rent = rent::Rent::default();
        let account_size =
            sbpf_loader::SbfLoaderState::program_data_offset().saturating_add(elf_bytes.len());
        let mut program_account = AccountSharedData::new(
            rent.minimum_balance(account_size),
            account_size,
            &sbpf_loader::id(),
        );
        program_account
            .set_state(&sbpf_loader::SbfLoaderState {
                slot: 0,
                is_deployed,
                authority_address,
            })
            .unwrap();
        program_account.data_mut()[sbpf_loader::SbfLoaderState::program_data_offset()..]
            .copy_from_slice(&elf_bytes);
        program_account
    }

    fn clock() -> AccountSharedData {
        let clock = clock::Clock {
            slot: 1000,
            ..clock::Clock::default()
        };
        create_account_shared_data_for_test(&clock)
    }

    #[test]
    fn test_loader_instruction_general_errors() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "noop"),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &sbpf_loader::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, None, "noop"),
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
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Error: Missing authority account
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true)],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Error: Program not owned by loader
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(1, false, true), (1, true, false)],
            Err(InstructionError::InvalidAccountOwner),
        );

        // Error: Program is uninitialized
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(2, false, true), (1, true, false)],
            Err(InstructionError::InvalidAccountData),
        );

        // Error: Program is not writeable
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, false), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Authority did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, false, false)],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Error: Program is finalized
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false)],
            Err(InstructionError::Immutable),
        );

        // Error: Incorrect authority provided
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts,
            &[(0, false, true), (2, true, false)],
            Err(InstructionError::IncorrectAuthority),
        );
    }

    #[test]
    fn test_loader_instruction_write() {
        let authority_address = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &sbpf_loader::id()),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(10000000, 0, &sbpf_loader::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "noop"),
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

        // Initialize account by first write
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 0,
                bytes: vec![0, 1, 2, 3],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, true, true)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            sbpf_loader::SbfLoaderState::program_data_offset().saturating_add(4),
        );
        assert_eq!(accounts[0].lamports(), 1252800);
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[2]
                .1
                .lamports()
                .saturating_sub(accounts[0].lamports()),
        );

        // Extend account by writing at the end
        transaction_accounts[0].1 = accounts[0].clone();
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 4,
                bytes: vec![4, 5, 6, 7],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, true, true)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            sbpf_loader::SbfLoaderState::program_data_offset().saturating_add(8),
        );
        assert_eq!(
            accounts[0].lamports(),
            transaction_accounts[0].1.lamports().saturating_add(27840),
        );
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[2].1.lamports().saturating_sub(
                accounts[0]
                    .lamports()
                    .saturating_sub(transaction_accounts[0].1.lamports()),
            ),
        );

        // Overwrite existing data (no payer required)
        transaction_accounts[0].1 = accounts[0].clone();
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 2,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            sbpf_loader::SbfLoaderState::program_data_offset().saturating_add(8),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: Program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Payer did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Error: Payer is not writeable
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Write out of bounds
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 9,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, true, true)],
            Err(InstructionError::AccountDataTooSmall),
        );

        // Error: Insufficient funds (Bankrupt payer account)
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (1, true, true)],
            Err(InstructionError::InsufficientFunds),
        );

        // Error: Insufficient funds (No payer account)
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Write {
                offset: 8,
                bytes: vec![8, 8, 8, 8],
            })
            .unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InsufficientFunds),
        );
    }

    #[test]
    fn test_loader_instruction_truncate() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, Some(authority_address), "noop"),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &sbpf_loader::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "noop"),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &sbpf_loader::id()),
            ),
            (clock::id(), clock()),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Cut the end off
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Truncate { offset: 4 }).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            sbpf_loader::SbfLoaderState::program_data_offset().saturating_add(4),
        );
        assert_eq!(accounts[0].lamports(), 1252800);
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[0]
                .1
                .lamports()
                .saturating_sub(accounts[0].lamports()),
        );

        // Close program account
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Truncate { offset: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
        assert_eq!(accounts[0].data().len(), 0);
        assert_eq!(
            accounts[2].lamports(),
            transaction_accounts[0]
                .1
                .lamports()
                .saturating_sub(accounts[0].lamports()),
        );

        // Error: Program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Truncate { offset: 0 }).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false), (2, false, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Truncate out of bounds
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Truncate { offset: 10000 }).unwrap(),
            transaction_accounts,
            &[(0, false, true), (1, true, false), (2, false, true)],
            Err(InstructionError::AccountDataTooSmall),
        );
    }

    #[test]
    fn test_loader_instruction_deploy() {
        let authority_address = Pubkey::new_unique();
        let mut transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, Some(authority_address), "rodata"),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, Some(authority_address), "noop"),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "noop"),
            ),
            (clock::id(), clock()),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Deploy from its own data
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Deploy by replacing data by other source
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, false, true)],
            Ok(()),
        );
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

        // Error: Source program is not retracted
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (3, false, true)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program is deployed already
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(3, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );

        // Error: Program was deployed recently, cooldown still in effect
        transaction_accounts[0].1 = accounts[0].clone();
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Deploy).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_loader_instruction_retract() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "rodata"),
            ),
            (
                authority_address,
                AccountSharedData::new(0, 0, &Pubkey::new_unique()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, Some(authority_address), "rodata"),
            ),
            (clock::id(), clock()),
            (
                rent::id(),
                create_account_shared_data_for_test(&rent::Rent::default()),
            ),
        ];

        // Retract program
        let accounts = process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: Program is not deployed
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::Retract).unwrap(),
            transaction_accounts,
            &[(2, false, true), (1, true, false)],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_loader_instruction_transfer_authority() {
        let authority_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(true, Some(authority_address), "rodata"),
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
            &bincode::serialize(&SbfLoaderInstruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false), (2, true, false)],
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
            &bincode::serialize(&SbfLoaderInstruction::TransferAuthority).unwrap(),
            transaction_accounts.clone(),
            &[(0, false, true), (1, true, false)],
            Ok(()),
        );
        assert_eq!(
            accounts[0].data().len(),
            transaction_accounts[0].1.data().len(),
        );
        assert_eq!(accounts[0].lamports(), transaction_accounts[0].1.lamports());

        // Error: New authority did not sign
        process_instruction(
            vec![],
            &bincode::serialize(&SbfLoaderInstruction::TransferAuthority).unwrap(),
            transaction_accounts,
            &[(0, false, true), (1, true, false), (2, false, false)],
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_invoke_program() {
        let program_address = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                program_address,
                load_program_account_from_elf(true, None, "rodata"),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(10000000, 32, &program_address),
            ),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(0, 0, &sbpf_loader::id()),
            ),
            (
                Pubkey::new_unique(),
                load_program_account_from_elf(false, None, "rodata"),
            ),
        ];

        // Invoke program
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
            transaction_accounts,
            &[(1, false, true)],
            Err(InstructionError::InvalidArgument),
        );
    }
}
