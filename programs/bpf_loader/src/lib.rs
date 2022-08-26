#![deny(clippy::integer_arithmetic)]
#![deny(clippy::indexing_slicing)]

pub mod allocator_bump;
pub mod deprecated;
pub mod serialization;
pub mod syscalls;
pub mod upgradeable;
pub mod upgradeable_with_jit;
pub mod with_jit;

#[macro_use]
extern crate solana_metrics;

use {
    crate::{
        serialization::{deserialize_parameters, serialize_parameters},
        syscalls::SyscallError,
    },
    log::{log_enabled, trace, Level::Trace},
    solana_measure::measure::Measure,
    solana_program_runtime::{
        ic_logger_msg, ic_msg,
        invoke_context::{ComputeMeter, Executor, InvokeContext},
        log_collector::LogCollector,
        stable_log,
        sysvar_cache::get_sysvar_with_account_check,
    },
    solana_rbpf::{
        aligned_memory::AlignedMemory,
        ebpf::{HOST_ALIGN, MM_INPUT_START},
        elf::Executable,
        error::{EbpfError, UserDefinedError},
        memory_region::MemoryRegion,
        static_analysis::Analysis,
        verifier::{RequisiteVerifier, VerifierError},
        vm::{Config, EbpfVm, InstructionMeter, VerifiedExecutable},
    },
    solana_sdk::{
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        entrypoint::{HEAP_LENGTH, SUCCESS},
        feature_set::{
            cap_accounts_data_allocations_per_transaction, cap_bpf_program_instruction_accounts,
            disable_deploy_of_alloc_free_syscall, disable_deprecated_loader,
            enable_bpf_loader_extend_program_data_ix,
            error_on_syscall_bpf_function_hash_collisions, reject_callx_r10,
        },
        instruction::{AccountMeta, InstructionError},
        loader_instruction::LoaderInstruction,
        loader_upgradeable_instruction::UpgradeableLoaderInstruction,
        program_error::MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        saturating_add_assign,
        system_instruction::{self, MAX_PERMITTED_DATA_LENGTH},
        transaction_context::{BorrowedAccount, InstructionContext, TransactionContext},
    },
    std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc},
    thiserror::Error,
};

solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    solana_bpf_loader_program::process_instruction
);

/// Errors returned by functions the BPF Loader registers with the VM
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BpfError {
    #[error("{0}")]
    VerifierError(#[from] VerifierError),
    #[error("{0}")]
    SyscallError(#[from] SyscallError),
}
impl UserDefinedError for BpfError {}

fn map_ebpf_error(invoke_context: &InvokeContext, e: EbpfError<BpfError>) -> InstructionError {
    ic_msg!(invoke_context, "{}", e);
    InstructionError::InvalidAccountData
}

mod executor_metrics {
    #[derive(Debug, Default)]
    pub struct CreateMetrics {
        pub program_id: String,
        pub load_elf_us: u64,
        pub verify_code_us: u64,
        pub jit_compile_us: u64,
    }

    impl CreateMetrics {
        pub fn submit_datapoint(&self) {
            datapoint_trace!(
                "create_executor_trace",
                ("program_id", self.program_id, String),
                ("load_elf_us", self.load_elf_us, i64),
                ("verify_code_us", self.verify_code_us, i64),
                ("jit_compile_us", self.jit_compile_us, i64),
            );
        }
    }
}

// The BPF loader is special in that it is the only place in the runtime and its built-in programs,
// where data comes not only from instruction account but also program accounts.
// Thus, these two helper methods have to distinguish the mixed sources via index_in_instruction.

fn get_index_in_transaction(
    instruction_context: &InstructionContext,
    index_in_instruction: usize,
) -> Result<usize, InstructionError> {
    if index_in_instruction < instruction_context.get_number_of_program_accounts() {
        instruction_context.get_index_of_program_account_in_transaction(index_in_instruction)
    } else {
        instruction_context.get_index_of_instruction_account_in_transaction(
            index_in_instruction
                .saturating_sub(instruction_context.get_number_of_program_accounts()),
        )
    }
}

fn try_borrow_account<'a>(
    transaction_context: &'a TransactionContext,
    instruction_context: &'a InstructionContext,
    index_in_instruction: usize,
) -> Result<BorrowedAccount<'a>, InstructionError> {
    if index_in_instruction < instruction_context.get_number_of_program_accounts() {
        instruction_context.try_borrow_program_account(transaction_context, index_in_instruction)
    } else {
        instruction_context.try_borrow_instruction_account(
            transaction_context,
            index_in_instruction
                .saturating_sub(instruction_context.get_number_of_program_accounts()),
        )
    }
}

pub fn create_executor(
    programdata_account_index: usize,
    programdata_offset: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
    reject_deployment_of_broken_elfs: bool,
    disable_deploy_of_alloc_free_syscall: bool,
) -> Result<Arc<BpfExecutor>, InstructionError> {
    let mut register_syscalls_time = Measure::start("register_syscalls_time");
    let register_syscall_result =
        syscalls::register_syscalls(invoke_context, disable_deploy_of_alloc_free_syscall);
    register_syscalls_time.stop();
    invoke_context.timings.create_executor_register_syscalls_us = invoke_context
        .timings
        .create_executor_register_syscalls_us
        .saturating_add(register_syscalls_time.as_us());
    let syscall_registry = register_syscall_result.map_err(|e| {
        ic_msg!(invoke_context, "Failed to register syscalls: {}", e);
        InstructionError::ProgramEnvironmentSetupFailure
    })?;
    let compute_budget = invoke_context.get_compute_budget();
    let config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_stack_frame_gaps: true,
        instruction_meter_checkpoint_distance: 10000,
        enable_instruction_meter: true,
        enable_instruction_tracing: log_enabled!(Trace),
        enable_symbol_and_section_labels: false,
        reject_broken_elfs: reject_deployment_of_broken_elfs,
        noop_instruction_rate: 256,
        sanitize_user_provided_values: true,
        encrypt_environment_registers: true,
        syscall_bpf_function_hash_collision: invoke_context
            .feature_set
            .is_active(&error_on_syscall_bpf_function_hash_collisions::id()),
        reject_callx_r10: invoke_context
            .feature_set
            .is_active(&reject_callx_r10::id()),
        dynamic_stack_frames: false,
        enable_sdiv: false,
        optimize_rodata: false,
        static_syscalls: false,
        enable_elf_vaddr: false,
        reject_rodata_stack_overlap: false,
        new_elf_parser: false,
        // Warning, do not use `Config::default()` so that configuration here is explicit.
    };
    let mut create_executor_metrics = executor_metrics::CreateMetrics::default();
    let executable = {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let programdata = try_borrow_account(
            transaction_context,
            instruction_context,
            programdata_account_index,
        )?;
        create_executor_metrics.program_id = programdata.get_key().to_string();
        let mut load_elf_time = Measure::start("load_elf_time");
        let executable = Executable::<BpfError, ThisInstructionMeter>::from_elf(
            programdata
                .get_data()
                .get(programdata_offset..)
                .ok_or(InstructionError::AccountDataTooSmall)?,
            config,
            syscall_registry,
        );
        load_elf_time.stop();
        create_executor_metrics.load_elf_us = load_elf_time.as_us();
        invoke_context.timings.create_executor_load_elf_us = invoke_context
            .timings
            .create_executor_load_elf_us
            .saturating_add(create_executor_metrics.load_elf_us);
        executable
    }
    .map_err(|e| map_ebpf_error(invoke_context, e))?;
    let mut verify_code_time = Measure::start("verify_code_time");
    let mut verified_executable =
        VerifiedExecutable::<RequisiteVerifier, BpfError, ThisInstructionMeter>::from_executable(
            executable,
        )
        .map_err(|e| map_ebpf_error(invoke_context, e))?;
    verify_code_time.stop();
    create_executor_metrics.verify_code_us = verify_code_time.as_us();
    invoke_context.timings.create_executor_verify_code_us = invoke_context
        .timings
        .create_executor_verify_code_us
        .saturating_add(create_executor_metrics.verify_code_us);
    if use_jit {
        let mut jit_compile_time = Measure::start("jit_compile_time");
        let jit_compile_result = verified_executable.jit_compile();
        jit_compile_time.stop();
        create_executor_metrics.jit_compile_us = jit_compile_time.as_us();
        invoke_context.timings.create_executor_jit_compile_us = invoke_context
            .timings
            .create_executor_jit_compile_us
            .saturating_add(create_executor_metrics.jit_compile_us);
        if let Err(err) = jit_compile_result {
            ic_msg!(invoke_context, "Failed to compile program {:?}", err);
            return Err(InstructionError::ProgramFailedToCompile);
        }
    }
    create_executor_metrics.submit_datapoint();
    Ok(Arc::new(BpfExecutor {
        verified_executable,
        use_jit,
    }))
}

fn write_program_data(
    program_account_index: usize,
    program_data_offset: usize,
    bytes: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program = try_borrow_account(
        transaction_context,
        instruction_context,
        program_account_index,
    )?;
    let data = program.get_data_mut()?;
    let write_offset = program_data_offset.saturating_add(bytes.len());
    if data.len() < write_offset {
        ic_msg!(
            invoke_context,
            "Write overflow: {} < {}",
            data.len(),
            write_offset,
        );
        return Err(InstructionError::AccountDataTooSmall);
    }
    data.get_mut(program_data_offset..write_offset)
        .ok_or(InstructionError::AccountDataTooSmall)?
        .copy_from_slice(bytes);
    Ok(())
}

fn check_loader_id(id: &Pubkey) -> bool {
    bpf_loader::check_id(id)
        || bpf_loader_deprecated::check_id(id)
        || bpf_loader_upgradeable::check_id(id)
}

/// Create the BPF virtual machine
pub fn create_vm<'a, 'b>(
    program: &'a VerifiedExecutable<RequisiteVerifier, BpfError, ThisInstructionMeter>,
    parameter_bytes: &mut [u8],
    orig_account_lengths: Vec<usize>,
    invoke_context: &'a mut InvokeContext<'b>,
) -> Result<EbpfVm<'a, RequisiteVerifier, BpfError, ThisInstructionMeter>, EbpfError<BpfError>> {
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size.unwrap_or(HEAP_LENGTH);
    let _ = invoke_context.get_compute_meter().borrow_mut().consume(
        ((heap_size as u64).saturating_div(32_u64.saturating_mul(1024)))
            .saturating_sub(1)
            .saturating_mul(compute_budget.heap_cost),
    );
    let mut heap =
        AlignedMemory::<HOST_ALIGN>::zero_filled(compute_budget.heap_size.unwrap_or(HEAP_LENGTH));
    let parameter_region = MemoryRegion::new_writable(parameter_bytes, MM_INPUT_START);
    let mut vm = EbpfVm::new(program, heap.as_slice_mut(), vec![parameter_region])?;
    syscalls::bind_syscall_context_objects(&mut vm, invoke_context, heap, orig_account_lengths)?;
    Ok(vm)
}

pub fn process_instruction(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(first_instruction_account, invoke_context, false)
}

pub fn process_instruction_jit(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(first_instruction_account, invoke_context, true)
}

fn process_instruction_common(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    let first_account_key = transaction_context.get_key_of_account_at_index(
        get_index_in_transaction(instruction_context, first_instruction_account)?,
    )?;
    let second_account_key = get_index_in_transaction(
        instruction_context,
        first_instruction_account.saturating_add(1),
    )
    .and_then(|index_in_transaction| {
        transaction_context.get_key_of_account_at_index(index_in_transaction)
    });

    let program_account_index = if first_account_key == program_id {
        first_instruction_account
    } else if second_account_key
        .map(|key| key == program_id)
        .unwrap_or(false)
    {
        first_instruction_account.saturating_add(1)
    } else {
        let first_account = try_borrow_account(
            transaction_context,
            instruction_context,
            first_instruction_account,
        )?;
        if first_account.is_executable() {
            ic_logger_msg!(log_collector, "BPF loader is executable");
            return Err(InstructionError::IncorrectProgramId);
        }
        first_instruction_account
    };

    let program = try_borrow_account(
        transaction_context,
        instruction_context,
        program_account_index,
    )?;
    if program.is_executable() {
        // First instruction account can only be zero if called from CPI, which
        // means stack height better be greater than one
        debug_assert_eq!(
            first_instruction_account == 0,
            invoke_context.get_stack_height() > 1
        );

        if !check_loader_id(program.get_owner()) {
            ic_logger_msg!(
                log_collector,
                "Executable account not owned by the BPF loader"
            );
            return Err(InstructionError::IncorrectProgramId);
        }

        let program_data_offset = if bpf_loader_upgradeable::check_id(program.get_owner()) {
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program.get_state()?
            {
                if programdata_address != *first_account_key {
                    ic_logger_msg!(
                        log_collector,
                        "Wrong ProgramData account for this Program account"
                    );
                    return Err(InstructionError::InvalidArgument);
                }
                if !matches!(
                    instruction_context
                        .try_borrow_program_account(transaction_context, first_instruction_account)?
                        .get_state()?,
                    UpgradeableLoaderState::ProgramData {
                        slot: _,
                        upgrade_authority_address: _,
                    }
                ) {
                    ic_logger_msg!(log_collector, "Program has been closed");
                    return Err(InstructionError::InvalidAccountData);
                }
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                ic_logger_msg!(log_collector, "Invalid Program account");
                return Err(InstructionError::InvalidAccountData);
            }
        } else {
            0
        };
        drop(program);

        let mut get_or_create_executor_time = Measure::start("get_or_create_executor_time");
        let executor = match invoke_context.get_executor(program_id) {
            Some(executor) => executor,
            None => {
                let executor = create_executor(
                    first_instruction_account,
                    program_data_offset,
                    invoke_context,
                    use_jit,
                    false, /* reject_deployment_of_broken_elfs */
                    // allow _sol_alloc_free syscall for execution
                    false, /* disable_sol_alloc_free_syscall */
                )?;
                let transaction_context = &invoke_context.transaction_context;
                let instruction_context = transaction_context.get_current_instruction_context()?;
                let program_id = instruction_context.get_last_program_key(transaction_context)?;
                invoke_context.add_executor(program_id, executor.clone());
                executor
            }
        };
        get_or_create_executor_time.stop();
        saturating_add_assign!(
            invoke_context.timings.get_or_create_executor_us,
            get_or_create_executor_time.as_us()
        );

        executor.execute(program_account_index, invoke_context)
    } else {
        drop(program);
        debug_assert_eq!(first_instruction_account, 1);
        let disable_deprecated_loader = invoke_context
            .feature_set
            .is_active(&disable_deprecated_loader::id());
        if bpf_loader_upgradeable::check_id(program_id) {
            process_loader_upgradeable_instruction(
                first_instruction_account,
                invoke_context,
                use_jit,
            )
        } else if bpf_loader::check_id(program_id)
            || (!disable_deprecated_loader && bpf_loader_deprecated::check_id(program_id))
        {
            process_loader_instruction(first_instruction_account, invoke_context, use_jit)
        } else if disable_deprecated_loader && bpf_loader_deprecated::check_id(program_id) {
            ic_logger_msg!(log_collector, "Deprecated loader is no longer supported");
            Err(InstructionError::UnsupportedProgramId)
        } else {
            ic_logger_msg!(log_collector, "Invalid BPF loader id");
            Err(InstructionError::IncorrectProgramId)
        }
    }
}

fn process_loader_upgradeable_instruction(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;

    match limited_deserialize(instruction_data)? {
        UpgradeableLoaderInstruction::InitializeBuffer => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let mut buffer =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

            if UpgradeableLoaderState::Uninitialized != buffer.get_state()? {
                ic_logger_msg!(log_collector, "Buffer account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }

            let authority_key = Some(*transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(1)?,
            )?);

            buffer.set_state(&UpgradeableLoaderState::Buffer {
                authority_address: authority_key,
            })?;
        }
        UpgradeableLoaderInstruction::Write { offset, bytes } => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let buffer =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.get_state()? {
                if authority_address.is_none() {
                    ic_logger_msg!(log_collector, "Buffer is immutable");
                    return Err(InstructionError::Immutable); // TODO better error code
                }
                let authority_key = Some(*transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(1)?,
                )?);
                if authority_address != authority_key {
                    ic_logger_msg!(log_collector, "Incorrect buffer authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if !instruction_context.is_instruction_account_signer(1)? {
                    ic_logger_msg!(log_collector, "Buffer authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidAccountData);
            }
            drop(buffer);
            write_program_data(
                first_instruction_account,
                UpgradeableLoaderState::size_of_buffer_metadata().saturating_add(offset as usize),
                &bytes,
                invoke_context,
            )?;
        }
        UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
            instruction_context.check_number_of_instruction_accounts(4)?;
            let payer_key = *transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(0)?,
            )?;
            let programdata_key = *transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(1)?,
            )?;
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 4)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 5)?;
            instruction_context.check_number_of_instruction_accounts(8)?;
            let authority_key = Some(*transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(7)?,
            )?);

            // Verify Program account

            let program =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            if UpgradeableLoaderState::Uninitialized != program.get_state()? {
                ic_logger_msg!(log_collector, "Program account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }
            if program.get_data().len() < UpgradeableLoaderState::size_of_program() {
                ic_logger_msg!(log_collector, "Program account too small");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if program.get_lamports() < rent.minimum_balance(program.get_data().len()) {
                ic_logger_msg!(log_collector, "Program account not rent-exempt");
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }
            let new_program_id = *program.get_key();
            drop(program);

            // Verify Buffer account

            let buffer =
                instruction_context.try_borrow_instruction_account(transaction_context, 3)?;
            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.get_state()? {
                if authority_address != authority_key {
                    ic_logger_msg!(log_collector, "Buffer and upgrade authority don't match");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if !instruction_context.is_instruction_account_signer(7)? {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }
            let buffer_key = *buffer.get_key();
            let buffer_lamports = buffer.get_lamports();
            let buffer_data_offset = UpgradeableLoaderState::size_of_buffer_metadata();
            let buffer_data_len = buffer.get_data().len().saturating_sub(buffer_data_offset);
            let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
            let programdata_len = UpgradeableLoaderState::size_of_programdata(max_data_len);
            if buffer.get_data().len() < UpgradeableLoaderState::size_of_buffer_metadata()
                || buffer_data_len == 0
            {
                ic_logger_msg!(log_collector, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }
            drop(buffer);
            if max_data_len < buffer_data_len {
                ic_logger_msg!(
                    log_collector,
                    "Max data length is too small to hold Buffer data"
                );
                return Err(InstructionError::AccountDataTooSmall);
            }
            if programdata_len > MAX_PERMITTED_DATA_LENGTH as usize {
                ic_logger_msg!(log_collector, "Max data length is too large");
                return Err(InstructionError::InvalidArgument);
            }

            // Create ProgramData account

            let (derived_address, bump_seed) =
                Pubkey::find_program_address(&[new_program_id.as_ref()], program_id);
            if derived_address != programdata_key {
                ic_logger_msg!(log_collector, "ProgramData address is not derived");
                return Err(InstructionError::InvalidArgument);
            }

            // Drain the Buffer account to payer before paying for programdata account
            {
                let mut payer =
                    instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
                payer.checked_add_lamports(buffer_lamports)?;
                drop(payer);
                let mut buffer =
                    instruction_context.try_borrow_instruction_account(transaction_context, 3)?;
                buffer.set_lamports(0)?;
            }

            let mut instruction = system_instruction::create_account(
                &payer_key,
                &programdata_key,
                1.max(rent.minimum_balance(programdata_len)),
                programdata_len as u64,
                program_id,
            );

            // pass an extra account to avoid the overly strict UnbalancedInstruction error
            instruction
                .accounts
                .push(AccountMeta::new(buffer_key, false));

            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let caller_program_id =
                instruction_context.get_last_program_key(transaction_context)?;
            let signers = [&[new_program_id.as_ref(), &[bump_seed]]]
                .iter()
                .map(|seeds| Pubkey::create_program_address(*seeds, caller_program_id))
                .collect::<Result<Vec<Pubkey>, solana_sdk::pubkey::PubkeyError>>()?;
            invoke_context.native_invoke(instruction, signers.as_slice())?;

            // Load and verify the program bits
            let executor = create_executor(
                first_instruction_account.saturating_add(3),
                buffer_data_offset,
                invoke_context,
                use_jit,
                true,
                invoke_context
                    .feature_set
                    .is_active(&disable_deploy_of_alloc_free_syscall::id()),
            )?;
            invoke_context.update_executor(&new_program_id, executor);

            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;

            // Update the ProgramData account and record the program bits
            {
                let mut programdata =
                    instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
                programdata.set_state(&UpgradeableLoaderState::ProgramData {
                    slot: clock.slot,
                    upgrade_authority_address: authority_key,
                })?;
                let dst_slice = programdata
                    .get_data_mut()?
                    .get_mut(
                        programdata_data_offset
                            ..programdata_data_offset.saturating_add(buffer_data_len),
                    )
                    .ok_or(InstructionError::AccountDataTooSmall)?;
                let buffer =
                    instruction_context.try_borrow_instruction_account(transaction_context, 3)?;
                let src_slice = buffer
                    .get_data()
                    .get(buffer_data_offset..)
                    .ok_or(InstructionError::AccountDataTooSmall)?;
                dst_slice.copy_from_slice(src_slice);
            }

            // Update the Program account
            let mut program =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            program.set_state(&UpgradeableLoaderState::Program {
                programdata_address: programdata_key,
            })?;
            program.set_executable(true)?;
            drop(program);

            ic_logger_msg!(log_collector, "Deployed program {:?}", new_program_id);
        }
        UpgradeableLoaderInstruction::Upgrade => {
            instruction_context.check_number_of_instruction_accounts(3)?;
            let programdata_key = *transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(0)?,
            )?;
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 4)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 5)?;
            instruction_context.check_number_of_instruction_accounts(7)?;
            let authority_key = Some(*transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(6)?,
            )?);

            // Verify Program account

            let program =
                instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
            if !program.is_executable() {
                ic_logger_msg!(log_collector, "Program account not executable");
                return Err(InstructionError::AccountNotExecutable);
            }
            if !program.is_writable() {
                ic_logger_msg!(log_collector, "Program account not writeable");
                return Err(InstructionError::InvalidArgument);
            }
            if program.get_owner() != program_id {
                ic_logger_msg!(log_collector, "Program account not owned by loader");
                return Err(InstructionError::IncorrectProgramId);
            }
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program.get_state()?
            {
                if programdata_address != programdata_key {
                    ic_logger_msg!(log_collector, "Program and ProgramData account mismatch");
                    return Err(InstructionError::InvalidArgument);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Program account");
                return Err(InstructionError::InvalidAccountData);
            }
            let new_program_id = *program.get_key();
            drop(program);

            // Verify Buffer account

            let buffer =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.get_state()? {
                if authority_address != authority_key {
                    ic_logger_msg!(log_collector, "Buffer and upgrade authority don't match");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if !instruction_context.is_instruction_account_signer(6)? {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }
            let buffer_lamports = buffer.get_lamports();
            let buffer_data_offset = UpgradeableLoaderState::size_of_buffer_metadata();
            let buffer_data_len = buffer.get_data().len().saturating_sub(buffer_data_offset);
            if buffer.get_data().len() < UpgradeableLoaderState::size_of_buffer_metadata()
                || buffer_data_len == 0
            {
                ic_logger_msg!(log_collector, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }
            drop(buffer);

            // Verify ProgramData account

            let programdata =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
            let programdata_balance_required =
                1.max(rent.minimum_balance(programdata.get_data().len()));
            if programdata.get_data().len()
                < UpgradeableLoaderState::size_of_programdata(buffer_data_len)
            {
                ic_logger_msg!(log_collector, "ProgramData account not large enough");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if programdata.get_lamports().saturating_add(buffer_lamports)
                < programdata_balance_required
            {
                ic_logger_msg!(
                    log_collector,
                    "Buffer account balance too low to fund upgrade"
                );
                return Err(InstructionError::InsufficientFunds);
            }
            if let UpgradeableLoaderState::ProgramData {
                slot: _,
                upgrade_authority_address,
            } = programdata.get_state()?
            {
                if upgrade_authority_address.is_none() {
                    ic_logger_msg!(log_collector, "Program not upgradeable");
                    return Err(InstructionError::Immutable);
                }
                if upgrade_authority_address != authority_key {
                    ic_logger_msg!(log_collector, "Incorrect upgrade authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if !instruction_context.is_instruction_account_signer(6)? {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid ProgramData account");
                return Err(InstructionError::InvalidAccountData);
            }
            drop(programdata);

            // Load and verify the program bits
            let executor = create_executor(
                first_instruction_account.saturating_add(2),
                buffer_data_offset,
                invoke_context,
                use_jit,
                true,
                invoke_context
                    .feature_set
                    .is_active(&disable_deploy_of_alloc_free_syscall::id()),
            )?;
            invoke_context.update_executor(&new_program_id, executor);

            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;

            // Update the ProgramData account, record the upgraded data, and zero
            // the rest
            let mut programdata =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            {
                programdata.set_state(&UpgradeableLoaderState::ProgramData {
                    slot: clock.slot,
                    upgrade_authority_address: authority_key,
                })?;
                let dst_slice = programdata
                    .get_data_mut()?
                    .get_mut(
                        programdata_data_offset
                            ..programdata_data_offset.saturating_add(buffer_data_len),
                    )
                    .ok_or(InstructionError::AccountDataTooSmall)?;
                let buffer =
                    instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
                let src_slice = buffer
                    .get_data()
                    .get(buffer_data_offset..)
                    .ok_or(InstructionError::AccountDataTooSmall)?;
                dst_slice.copy_from_slice(src_slice);
            }
            programdata
                .get_data_mut()?
                .get_mut(programdata_data_offset.saturating_add(buffer_data_len)..)
                .ok_or(InstructionError::AccountDataTooSmall)?
                .fill(0);

            // Fund ProgramData to rent-exemption, spill the rest

            let programdata_lamports = programdata.get_lamports();
            programdata.set_lamports(programdata_balance_required)?;
            drop(programdata);

            let mut buffer =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            buffer.set_lamports(0)?;
            drop(buffer);

            let mut spill =
                instruction_context.try_borrow_instruction_account(transaction_context, 3)?;
            spill.checked_add_lamports(
                programdata_lamports
                    .saturating_add(buffer_lamports)
                    .saturating_sub(programdata_balance_required),
            )?;

            ic_logger_msg!(log_collector, "Upgraded program {:?}", new_program_id);
        }
        UpgradeableLoaderInstruction::SetAuthority => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let mut account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let present_authority_key = transaction_context.get_key_of_account_at_index(
                instruction_context.get_index_of_instruction_account_in_transaction(1)?,
            )?;
            let new_authority = instruction_context
                .get_index_of_instruction_account_in_transaction(2)
                .and_then(|index_in_transaction| {
                    transaction_context.get_key_of_account_at_index(index_in_transaction)
                })
                .ok();

            match account.get_state()? {
                UpgradeableLoaderState::Buffer { authority_address } => {
                    if new_authority.is_none() {
                        ic_logger_msg!(log_collector, "Buffer authority is not optional");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if authority_address.is_none() {
                        ic_logger_msg!(log_collector, "Buffer is immutable");
                        return Err(InstructionError::Immutable);
                    }
                    if authority_address != Some(*present_authority_key) {
                        ic_logger_msg!(log_collector, "Incorrect buffer authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if !instruction_context.is_instruction_account_signer(1)? {
                        ic_logger_msg!(log_collector, "Buffer authority did not sign");
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                    account.set_state(&UpgradeableLoaderState::Buffer {
                        authority_address: new_authority.cloned(),
                    })?;
                }
                UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address,
                } => {
                    if upgrade_authority_address.is_none() {
                        ic_logger_msg!(log_collector, "Program not upgradeable");
                        return Err(InstructionError::Immutable);
                    }
                    if upgrade_authority_address != Some(*present_authority_key) {
                        ic_logger_msg!(log_collector, "Incorrect upgrade authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if !instruction_context.is_instruction_account_signer(1)? {
                        ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                    account.set_state(&UpgradeableLoaderState::ProgramData {
                        slot,
                        upgrade_authority_address: new_authority.cloned(),
                    })?;
                }
                _ => {
                    ic_logger_msg!(log_collector, "Account does not support authorities");
                    return Err(InstructionError::InvalidArgument);
                }
            }

            ic_logger_msg!(log_collector, "New authority {:?}", new_authority);
        }
        UpgradeableLoaderInstruction::Close => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            if instruction_context.get_index_of_instruction_account_in_transaction(0)?
                == instruction_context.get_index_of_instruction_account_in_transaction(1)?
            {
                ic_logger_msg!(
                    log_collector,
                    "Recipient is the same as the account being closed"
                );
                return Err(InstructionError::InvalidArgument);
            }
            let mut close_account =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            let close_key = *close_account.get_key();
            match close_account.get_state()? {
                UpgradeableLoaderState::Uninitialized => {
                    let close_lamports = close_account.get_lamports();
                    close_account.set_lamports(0)?;
                    drop(close_account);
                    let mut recipient_account = instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?;
                    recipient_account.checked_add_lamports(close_lamports)?;

                    ic_logger_msg!(log_collector, "Closed Uninitialized {}", close_key);
                }
                UpgradeableLoaderState::Buffer { authority_address } => {
                    instruction_context.check_number_of_instruction_accounts(3)?;
                    drop(close_account);
                    common_close_account(
                        &authority_address,
                        transaction_context,
                        instruction_context,
                        &log_collector,
                    )?;

                    ic_logger_msg!(log_collector, "Closed Buffer {}", close_key);
                }
                UpgradeableLoaderState::ProgramData {
                    slot: _,
                    upgrade_authority_address: authority_address,
                } => {
                    instruction_context.check_number_of_instruction_accounts(4)?;
                    drop(close_account);
                    let program_account = instruction_context
                        .try_borrow_instruction_account(transaction_context, 3)?;
                    let program_key = *program_account.get_key();

                    if !program_account.is_writable() {
                        ic_logger_msg!(log_collector, "Program account is not writable");
                        return Err(InstructionError::InvalidArgument);
                    }
                    if program_account.get_owner() != program_id {
                        ic_logger_msg!(log_collector, "Program account not owned by loader");
                        return Err(InstructionError::IncorrectProgramId);
                    }

                    match program_account.get_state()? {
                        UpgradeableLoaderState::Program {
                            programdata_address,
                        } => {
                            if programdata_address != close_key {
                                ic_logger_msg!(
                                    log_collector,
                                    "ProgramData account does not match ProgramData account"
                                );
                                return Err(InstructionError::InvalidArgument);
                            }

                            drop(program_account);
                            common_close_account(
                                &authority_address,
                                transaction_context,
                                instruction_context,
                                &log_collector,
                            )?;
                        }
                        _ => {
                            ic_logger_msg!(log_collector, "Invalid Program account");
                            return Err(InstructionError::InvalidArgument);
                        }
                    }

                    ic_logger_msg!(log_collector, "Closed Program {}", program_key);
                }
                _ => {
                    ic_logger_msg!(log_collector, "Account does not support closing");
                    return Err(InstructionError::InvalidArgument);
                }
            }
        }
        UpgradeableLoaderInstruction::ExtendProgramData { additional_bytes } => {
            if !invoke_context
                .feature_set
                .is_active(&enable_bpf_loader_extend_program_data_ix::ID)
            {
                return Err(InstructionError::InvalidInstructionData);
            }

            if additional_bytes == 0 {
                ic_logger_msg!(log_collector, "Additional bytes must be greater than 0");
                return Err(InstructionError::InvalidInstructionData);
            }

            const PROGRAM_DATA_ACCOUNT_INDEX: usize = 0;
            #[allow(dead_code)]
            // System program is only required when a CPI is performed
            const OPTIONAL_SYSTEM_PROGRAM_ACCOUNT_INDEX: usize = 1;
            const OPTIONAL_PAYER_ACCOUNT_INDEX: usize = 2;

            let programdata_account = instruction_context
                .try_borrow_instruction_account(transaction_context, PROGRAM_DATA_ACCOUNT_INDEX)?;
            let programdata_key = *programdata_account.get_key();

            if program_id != programdata_account.get_owner() {
                ic_logger_msg!(log_collector, "ProgramData owner is invalid");
                return Err(InstructionError::InvalidAccountOwner);
            }
            if !programdata_account.is_writable() {
                ic_logger_msg!(log_collector, "ProgramData is not writable");
                return Err(InstructionError::InvalidArgument);
            }

            let old_len = programdata_account.get_data().len();
            let new_len = old_len.saturating_add(additional_bytes as usize);
            if new_len > MAX_PERMITTED_DATA_LENGTH as usize {
                ic_logger_msg!(
                    log_collector,
                    "Extended ProgramData length of {} bytes exceeds max account data length of {} bytes",
                    new_len,
                    MAX_PERMITTED_DATA_LENGTH
                );
                return Err(InstructionError::InvalidRealloc);
            }

            if let UpgradeableLoaderState::ProgramData {
                slot: _,
                upgrade_authority_address,
            } = programdata_account.get_state()?
            {
                if upgrade_authority_address.is_none() {
                    ic_logger_msg!(
                        log_collector,
                        "Cannot extend ProgramData accounts that are not upgradeable"
                    );
                    return Err(InstructionError::Immutable);
                }
            } else {
                ic_logger_msg!(log_collector, "ProgramData state is invalid");
                return Err(InstructionError::InvalidAccountData);
            }

            let required_payment = {
                let balance = programdata_account.get_lamports();
                let rent = invoke_context.get_sysvar_cache().get_rent()?;
                let min_balance = rent.minimum_balance(new_len).max(1);
                min_balance.saturating_sub(balance)
            };

            // Borrowed accounts need to be dropped before native_invoke
            drop(programdata_account);

            if required_payment > 0 {
                let payer_key = *transaction_context.get_key_of_account_at_index(
                    instruction_context.get_index_of_instruction_account_in_transaction(
                        OPTIONAL_PAYER_ACCOUNT_INDEX,
                    )?,
                )?;

                invoke_context.native_invoke(
                    system_instruction::transfer(&payer_key, &programdata_key, required_payment),
                    &[],
                )?;
            }

            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let mut programdata_account = instruction_context
                .try_borrow_instruction_account(transaction_context, PROGRAM_DATA_ACCOUNT_INDEX)?;
            programdata_account.set_data_length(new_len)?;

            ic_logger_msg!(
                log_collector,
                "Extended ProgramData account by {} bytes",
                additional_bytes
            );
        }
    }

    Ok(())
}

fn common_close_account(
    authority_address: &Option<Pubkey>,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
) -> Result<(), InstructionError> {
    if authority_address.is_none() {
        ic_logger_msg!(log_collector, "Account is immutable");
        return Err(InstructionError::Immutable);
    }
    if *authority_address
        != Some(*transaction_context.get_key_of_account_at_index(
            instruction_context.get_index_of_instruction_account_in_transaction(2)?,
        )?)
    {
        ic_logger_msg!(log_collector, "Incorrect authority provided");
        return Err(InstructionError::IncorrectAuthority);
    }
    if !instruction_context.is_instruction_account_signer(2)? {
        ic_logger_msg!(log_collector, "Authority did not sign");
        return Err(InstructionError::MissingRequiredSignature);
    }

    let mut close_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let mut recipient_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
    recipient_account.checked_add_lamports(close_account.get_lamports())?;
    close_account.set_lamports(0)?;
    close_account.set_state(&UpgradeableLoaderState::Uninitialized)?;
    Ok(())
}

fn process_loader_instruction(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_last_program_key(transaction_context)?;
    let program = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    if program.get_owner() != program_id {
        ic_msg!(
            invoke_context,
            "Executable account not owned by the BPF loader"
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    let is_program_signer = program.is_signer();
    drop(program);
    match limited_deserialize(instruction_data)? {
        LoaderInstruction::Write { offset, bytes } => {
            if !is_program_signer {
                ic_msg!(invoke_context, "Program account did not sign");
                return Err(InstructionError::MissingRequiredSignature);
            }
            write_program_data(
                first_instruction_account,
                offset as usize,
                &bytes,
                invoke_context,
            )?;
        }
        LoaderInstruction::Finalize => {
            if !is_program_signer {
                ic_msg!(invoke_context, "key[0] did not sign the transaction");
                return Err(InstructionError::MissingRequiredSignature);
            }
            let executor = create_executor(
                first_instruction_account,
                0,
                invoke_context,
                use_jit,
                true,
                invoke_context
                    .feature_set
                    .is_active(&disable_deploy_of_alloc_free_syscall::id()),
            )?;
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let mut program =
                instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
            invoke_context.update_executor(program.get_key(), executor);
            program.set_executable(true)?;
            ic_msg!(invoke_context, "Finalized account {:?}", program.get_key());
        }
    }

    Ok(())
}

/// Passed to the VM to enforce the compute budget
pub struct ThisInstructionMeter {
    pub compute_meter: Rc<RefCell<ComputeMeter>>,
}
impl ThisInstructionMeter {
    fn new(compute_meter: Rc<RefCell<ComputeMeter>>) -> Self {
        Self { compute_meter }
    }
}
impl InstructionMeter for ThisInstructionMeter {
    fn consume(&mut self, amount: u64) {
        // 1 to 1 instruction to compute unit mapping
        // ignore error, Ebpf will bail if exceeded
        let _ = self.compute_meter.borrow_mut().consume(amount);
    }
    fn get_remaining(&self) -> u64 {
        self.compute_meter.borrow().get_remaining()
    }
}

/// BPF Loader's Executor implementation
pub struct BpfExecutor {
    verified_executable: VerifiedExecutable<RequisiteVerifier, BpfError, ThisInstructionMeter>,
    use_jit: bool,
}

// Well, implement Debug for solana_rbpf::vm::Executable in solana-rbpf...
impl Debug for BpfExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BpfExecutor({:p})", self)
    }
}

impl Executor for BpfExecutor {
    fn execute(
        &self,
        _first_instruction_account: usize,
        invoke_context: &mut InvokeContext,
    ) -> Result<(), InstructionError> {
        let log_collector = invoke_context.get_log_collector();
        let compute_meter = invoke_context.get_compute_meter();
        let stack_height = invoke_context.get_stack_height();
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let program_id = *instruction_context.get_last_program_key(transaction_context)?;

        let mut serialize_time = Measure::start("serialize");
        let (mut parameter_bytes, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            instruction_context,
            invoke_context
                .feature_set
                .is_active(&cap_bpf_program_instruction_accounts::ID),
        )?;
        serialize_time.stop();

        let mut create_vm_time = Measure::start("create_vm");
        let mut execute_time;
        let execution_result = {
            let mut vm = match create_vm(
                &self.verified_executable,
                parameter_bytes.as_slice_mut(),
                account_lengths,
                invoke_context,
            ) {
                Ok(info) => info,
                Err(e) => {
                    ic_logger_msg!(log_collector, "Failed to create BPF VM: {}", e);
                    return Err(InstructionError::ProgramEnvironmentSetupFailure);
                }
            };
            create_vm_time.stop();

            execute_time = Measure::start("execute");
            stable_log::program_invoke(&log_collector, &program_id, stack_height);
            let mut instruction_meter = ThisInstructionMeter::new(compute_meter.clone());
            let before = compute_meter.borrow().get_remaining();
            let result = if self.use_jit {
                vm.execute_program_jit(&mut instruction_meter)
            } else {
                vm.execute_program_interpreted(&mut instruction_meter)
            };
            let after = compute_meter.borrow().get_remaining();
            ic_logger_msg!(
                log_collector,
                "Program {} consumed {} of {} compute units",
                &program_id,
                before.saturating_sub(after),
                before
            );
            if log_enabled!(Trace) {
                let mut trace_buffer = Vec::<u8>::new();
                let analysis =
                    Analysis::from_executable(self.verified_executable.get_executable()).unwrap();
                vm.get_tracer().write(&mut trace_buffer, &analysis).unwrap();
                let trace_string = String::from_utf8(trace_buffer).unwrap();
                trace!("BPF Program Instruction Trace:\n{}", trace_string);
            }
            drop(vm);
            let (_returned_from_program_id, return_data) =
                invoke_context.transaction_context.get_return_data();
            if !return_data.is_empty() {
                stable_log::program_return(&log_collector, &program_id, return_data);
            }
            match result {
                Ok(status) if status != SUCCESS => {
                    let error: InstructionError = if status
                        == MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED
                        && !invoke_context
                            .feature_set
                            .is_active(&cap_accounts_data_allocations_per_transaction::id())
                    {
                        // Until the cap_accounts_data_allocations_per_transaction feature is
                        // enabled, map the MAX_ACCOUNTS_DATA_ALLOCATIONS_EXCEEDED error to InvalidError
                        InstructionError::InvalidError
                    } else {
                        status.into()
                    };
                    stable_log::program_failure(&log_collector, &program_id, &error);
                    Err(error)
                }
                Err(error) => {
                    let error = match error {
                        EbpfError::UserError(BpfError::SyscallError(
                            SyscallError::InstructionError(error),
                        )) => error,
                        err => {
                            ic_logger_msg!(log_collector, "Program failed to complete: {}", err);
                            InstructionError::ProgramFailedToComplete
                        }
                    };
                    stable_log::program_failure(&log_collector, &program_id, &error);
                    Err(error)
                }
                _ => Ok(()),
            }
        };
        execute_time.stop();

        let mut deserialize_time = Measure::start("deserialize");
        let execute_or_deserialize_result = execution_result.and_then(|_| {
            deserialize_parameters(
                invoke_context.transaction_context,
                invoke_context
                    .transaction_context
                    .get_current_instruction_context()?,
                parameter_bytes.as_slice(),
                invoke_context.get_orig_account_lengths()?,
            )
        });
        deserialize_time.stop();

        // Update the timings
        let timings = &mut invoke_context.timings;
        timings.serialize_us = timings.serialize_us.saturating_add(serialize_time.as_us());
        timings.create_vm_us = timings.create_vm_us.saturating_add(create_vm_time.as_us());
        timings.execute_us = timings.execute_us.saturating_add(execute_time.as_us());
        timings.deserialize_us = timings
            .deserialize_us
            .saturating_add(deserialize_time.as_us());

        if execute_or_deserialize_result.is_ok() {
            stable_log::program_success(&log_collector, &program_id);
        }
        execute_or_deserialize_result
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand::Rng,
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_rbpf::{verifier::Verifier, vm::SyscallRegistry},
        solana_runtime::{bank::Bank, bank_client::BankClient},
        solana_sdk::{
            account::{
                create_account_shared_data_for_test as create_account_for_test, AccountSharedData,
                ReadableAccount, WritableAccount,
            },
            account_utils::StateMut,
            client::SyncClient,
            clock::Clock,
            feature_set::FeatureSet,
            genesis_config::create_genesis_config,
            instruction::{AccountMeta, Instruction, InstructionError},
            message::Message,
            native_token::LAMPORTS_PER_SOL,
            pubkey::Pubkey,
            rent::Rent,
            signature::{Keypair, Signer},
            system_program, sysvar,
            transaction::TransactionError,
        },
        std::{fs::File, io::Read, ops::Range, sync::Arc},
    };

    struct TestInstructionMeter {
        remaining: u64,
    }
    impl InstructionMeter for TestInstructionMeter {
        fn consume(&mut self, amount: u64) {
            self.remaining = self.remaining.saturating_sub(amount);
        }
        fn get_remaining(&self) -> u64 {
            self.remaining
        }
    }

    fn process_instruction(
        loader_id: &Pubkey,
        program_indices: &[usize],
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            loader_id,
            program_indices.to_vec(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            None,
            None,
            expected_result,
            super::process_instruction,
        )
    }

    fn load_program_account_from_elf(loader_id: &Pubkey, path: &str) -> AccountSharedData {
        let mut file = File::open(path).expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let rent = Rent::default();
        let mut program_account =
            AccountSharedData::new(rent.minimum_balance(elf.len()), 0, loader_id);
        program_account.set_data(elf);
        program_account.set_executable(true);
        program_account
    }

    struct TautologyVerifier {}
    impl Verifier for TautologyVerifier {
        fn verify(_prog: &[u8], _config: &Config) -> std::result::Result<(), VerifierError> {
            Ok(())
        }
    }

    #[test]
    #[should_panic(expected = "ExceededMaxInstructions(31, 10)")]
    fn test_bpf_loader_non_terminating_program() {
        #[rustfmt::skip]
        let program = &[
            0x07, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // r6 + 1
            0x05, 0x00, 0xfe, 0xff, 0x00, 0x00, 0x00, 0x00, // goto -2
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // exit
        ];
        let mut input_mem = [0x00];
        let config = Config::default();
        let syscall_registry = SyscallRegistry::default();
        let mut bpf_functions = std::collections::BTreeMap::<u32, (usize, String)>::new();
        solana_rbpf::elf::register_bpf_function(
            &config,
            &mut bpf_functions,
            &syscall_registry,
            0,
            "entrypoint",
        )
        .unwrap();
        let executable = Executable::<BpfError, TestInstructionMeter>::from_text_bytes(
            program,
            config,
            syscall_registry,
            bpf_functions,
        )
        .unwrap();
        let verified_executable = VerifiedExecutable::<
            TautologyVerifier,
            BpfError,
            TestInstructionMeter,
        >::from_executable(executable)
        .unwrap();
        let input_region = MemoryRegion::new_writable(&mut input_mem, MM_INPUT_START);
        let mut vm = EbpfVm::new(&verified_executable, &mut [], vec![input_region]).unwrap();
        let mut instruction_meter = TestInstructionMeter { remaining: 10 };
        vm.execute_program_interpreted(&mut instruction_meter)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "LDDWCannotBeLast")]
    fn test_bpf_loader_check_load_dw() {
        let prog = &[
            0x18, 0x00, 0x00, 0x00, 0x88, 0x77, 0x66, 0x55, // first half of lddw
        ];
        RequisiteVerifier::verify(prog, &Config::default()).unwrap();
    }

    #[test]
    fn test_bpf_loader_write() {
        let loader_id = bpf_loader::id();
        let program_id = Pubkey::new_unique();
        let mut program_account = AccountSharedData::new(1, 0, &loader_id);
        let instruction_data = bincode::serialize(&LoaderInstruction::Write {
            offset: 3,
            bytes: vec![1, 2, 3],
        })
        .unwrap();

        // Case: No program account
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Case: Not signed
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account.clone())],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: Write bytes to an offset
        program_account.set_data(vec![0; 6]);
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account.clone())],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
        );
        assert_eq!(&vec![0, 0, 0, 1, 2, 3], accounts.first().unwrap().data());

        // Case: Overflow
        program_account.set_data(vec![0; 5]);
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account)],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::AccountDataTooSmall),
        );
    }

    #[test]
    fn test_bpf_loader_finalize() {
        let loader_id = bpf_loader::id();
        let program_id = Pubkey::new_unique();
        let mut program_account =
            load_program_account_from_elf(&loader_id, "test_elfs/out/noop_aligned.so");
        program_account.set_executable(false);
        let instruction_data = bincode::serialize(&LoaderInstruction::Finalize).unwrap();

        // Case: No program account
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Case: Not signed
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account.clone())],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: Finalize
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account.clone())],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
        );
        assert!(accounts.first().unwrap().executable());

        // Case: Finalize bad ELF
        *program_account.data_as_mut_slice().get_mut(0).unwrap() = 0;
        process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![(program_id, program_account)],
            vec![AccountMeta {
                pubkey: program_id,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_bpf_loader_invoke_main() {
        let loader_id = bpf_loader::id();
        let program_id = Pubkey::new_unique();
        let mut program_account =
            load_program_account_from_elf(&loader_id, "test_elfs/out/noop_aligned.so");
        let parameter_id = Pubkey::new_unique();
        let parameter_account = AccountSharedData::new(1, 0, &loader_id);
        let parameter_meta = AccountMeta {
            pubkey: parameter_id,
            is_signer: false,
            is_writable: false,
        };

        // Case: No program account
        process_instruction(
            &loader_id,
            &[],
            &[],
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Case: Only a program account
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![(program_id, program_account.clone())],
            Vec::new(),
            Ok(()),
        );

        // Case: With program and parameter account
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account.clone()),
                (parameter_id, parameter_account.clone()),
            ],
            vec![parameter_meta.clone()],
            Ok(()),
        );

        // Case: With duplicate accounts
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account.clone()),
                (parameter_id, parameter_account),
            ],
            vec![parameter_meta.clone(), parameter_meta],
            Ok(()),
        );

        // Case: limited budget
        mock_process_instruction(
            &loader_id,
            vec![0],
            &[],
            vec![(program_id, program_account.clone())],
            Vec::new(),
            None,
            None,
            Err(InstructionError::ProgramFailedToComplete),
            |first_instruction_account: usize, invoke_context: &mut InvokeContext| {
                invoke_context
                    .get_compute_meter()
                    .borrow_mut()
                    .mock_set_remaining(0);
                super::process_instruction(first_instruction_account, invoke_context)
            },
        );

        // Case: Account not a program
        program_account.set_executable(false);
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![(program_id, program_account)],
            Vec::new(),
            Err(InstructionError::IncorrectProgramId),
        );
    }

    #[test]
    fn test_bpf_loader_serialize_unaligned() {
        let loader_id = bpf_loader_deprecated::id();
        let program_id = Pubkey::new_unique();
        let program_account =
            load_program_account_from_elf(&loader_id, "test_elfs/out/noop_unaligned.so");
        let parameter_id = Pubkey::new_unique();
        let parameter_account = AccountSharedData::new(1, 0, &loader_id);
        let parameter_meta = AccountMeta {
            pubkey: parameter_id,
            is_signer: false,
            is_writable: false,
        };

        // Case: With program and parameter account
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account.clone()),
                (parameter_id, parameter_account.clone()),
            ],
            vec![parameter_meta.clone()],
            Ok(()),
        );

        // Case: With duplicate accounts
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account),
                (parameter_id, parameter_account),
            ],
            vec![parameter_meta.clone(), parameter_meta],
            Ok(()),
        );
    }

    #[test]
    fn test_bpf_loader_serialize_aligned() {
        let loader_id = bpf_loader::id();
        let program_id = Pubkey::new_unique();
        let program_account =
            load_program_account_from_elf(&loader_id, "test_elfs/out/noop_aligned.so");
        let parameter_id = Pubkey::new_unique();
        let parameter_account = AccountSharedData::new(1, 0, &loader_id);
        let parameter_meta = AccountMeta {
            pubkey: parameter_id,
            is_signer: false,
            is_writable: false,
        };

        // Case: With program and parameter account
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account.clone()),
                (parameter_id, parameter_account.clone()),
            ],
            vec![parameter_meta.clone()],
            Ok(()),
        );

        // Case: With duplicate accounts
        process_instruction(
            &loader_id,
            &[0],
            &[],
            vec![
                (program_id, program_account),
                (parameter_id, parameter_account),
            ],
            vec![parameter_meta.clone(), parameter_meta],
            Ok(()),
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_initialize_buffer() {
        let loader_id = bpf_loader_upgradeable::id();
        let buffer_address = Pubkey::new_unique();
        let buffer_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(9), &loader_id);
        let authority_address = Pubkey::new_unique();
        let authority_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(9), &loader_id);
        let instruction_data =
            bincode::serialize(&UpgradeableLoaderInstruction::InitializeBuffer).unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: buffer_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Case: Success
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![
                (buffer_address, buffer_account),
                (authority_address, authority_account),
            ],
            instruction_accounts.clone(),
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address)
            }
        );

        // Case: Already initialized
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction_data,
            vec![
                (buffer_address, accounts.first().unwrap().clone()),
                (authority_address, accounts.get(1).unwrap().clone()),
            ],
            instruction_accounts,
            Err(InstructionError::AccountAlreadyInitialized),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address)
            }
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_write() {
        let loader_id = bpf_loader_upgradeable::id();
        let buffer_address = Pubkey::new_unique();
        let mut buffer_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(9), &loader_id);
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: buffer_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: buffer_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // Case: Not initialized
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );

        // Case: Write entire buffer
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts.clone(),
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &accounts
                .first()
                .unwrap()
                .data()
                .get(UpgradeableLoaderState::size_of_buffer_metadata()..)
                .unwrap(),
            &[42; 9]
        );

        // Case: Write portion of the buffer
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 3,
            bytes: vec![42; 6],
        })
        .unwrap();
        let mut buffer_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(9), &loader_id);
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts.clone(),
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &accounts
                .first()
                .unwrap()
                .data()
                .get(UpgradeableLoaderState::size_of_buffer_metadata()..)
                .unwrap(),
            &[0, 0, 0, 42, 42, 42, 42, 42, 42]
        );

        // Case: overflow size
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 10],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts.clone(),
            Err(InstructionError::AccountDataTooSmall),
        );

        // Case: overflow offset
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts.clone(),
            Err(InstructionError::AccountDataTooSmall),
        );

        // Case: Not signed
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            vec![
                AccountMeta {
                    pubkey: buffer_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: buffer_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: wrong authority
        let authority_address = Pubkey::new_unique();
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (buffer_address, buffer_account.clone()),
                (authority_address, buffer_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: buffer_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authority_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: None authority
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![(buffer_address, buffer_account.clone())],
            instruction_accounts,
            Err(InstructionError::Immutable),
        );
    }

    fn truncate_data(account: &mut AccountSharedData, len: usize) {
        let mut data = account.data().to_vec();
        data.truncate(len);
        account.set_data(data);
    }

    #[test]
    fn test_bpf_loader_upgradeable_deploy_with_max_len() {
        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000_000);
        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.add_builtin(
            "solana_bpf_loader_upgradeable_program",
            &bpf_loader_upgradeable::id(),
            super::process_instruction,
        );
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        // Setup keypairs and addresses
        let payer_keypair = Keypair::new();
        let program_keypair = Keypair::new();
        let buffer_address = Pubkey::new_unique();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_keypair.pubkey().as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let upgrade_authority_keypair = Keypair::new();

        // Load program file
        let mut file = File::open("test_elfs/out/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        // Compute rent exempt balances
        let program_len = elf.len();
        let min_program_balance =
            bank.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program());
        let min_buffer_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::size_of_buffer(program_len),
        );
        let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::size_of_programdata(program_len),
        );

        // Setup accounts
        let buffer_account = {
            let mut account = AccountSharedData::new(
                min_buffer_balance,
                UpgradeableLoaderState::size_of_buffer(elf.len()),
                &bpf_loader_upgradeable::id(),
            );
            account
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(upgrade_authority_keypair.pubkey()),
                })
                .unwrap();
            account
                .data_as_mut_slice()
                .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
                .unwrap()
                .copy_from_slice(&elf);
            account
        };
        let program_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_program(),
            &bpf_loader_upgradeable::id(),
        );
        let programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_programdata(elf.len()),
            &bpf_loader_upgradeable::id(),
        );

        // Test successful deploy
        let payer_base_balance = LAMPORTS_PER_SOL;
        let deploy_fees = {
            let fee_calculator = genesis_config.fee_rate_governor.create_fee_calculator();
            3 * fee_calculator.lamports_per_signature
        };
        let min_payer_balance = min_program_balance
            .saturating_add(min_programdata_balance)
            .saturating_sub(min_buffer_balance.saturating_add(deploy_fees));
        bank.store_account(
            &payer_keypair.pubkey(),
            &AccountSharedData::new(
                payer_base_balance.saturating_add(min_payer_balance),
                0,
                &system_program::id(),
            ),
        );
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &payer_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&payer_keypair.pubkey()),
        );
        assert!(bank_client
            .send_and_confirm_message(
                &[&payer_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .is_ok());
        assert_eq!(
            bank.get_balance(&payer_keypair.pubkey()),
            payer_base_balance
        );
        assert_eq!(bank.get_balance(&buffer_address), 0);
        assert_eq!(None, bank.get_account(&buffer_address));
        let post_program_account = bank.get_account(&program_keypair.pubkey()).unwrap();
        assert_eq!(post_program_account.lamports(), min_program_balance);
        assert_eq!(post_program_account.owner(), &bpf_loader_upgradeable::id());
        assert_eq!(
            post_program_account.data().len(),
            UpgradeableLoaderState::size_of_program()
        );
        let state: UpgradeableLoaderState = post_program_account.state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Program {
                programdata_address
            }
        );
        let post_programdata_account = bank.get_account(&programdata_address).unwrap();
        assert_eq!(post_programdata_account.lamports(), min_programdata_balance);
        assert_eq!(
            post_programdata_account.owner(),
            &bpf_loader_upgradeable::id()
        );
        let state: UpgradeableLoaderState = post_programdata_account.state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot: bank_client.get_slot().unwrap(),
                upgrade_authority_address: Some(upgrade_authority_keypair.pubkey())
            }
        );
        for (i, byte) in post_programdata_account
            .data()
            .get(UpgradeableLoaderState::size_of_programdata_metadata()..)
            .unwrap()
            .iter()
            .enumerate()
        {
            assert_eq!(*elf.get(i).unwrap(), *byte);
        }

        // Invoke deployed program
        process_instruction(
            &bpf_loader_upgradeable::id(),
            &[0, 1],
            &[],
            vec![
                (programdata_address, post_programdata_account),
                (program_keypair.pubkey(), post_program_account),
            ],
            Vec::new(),
            Ok(()),
        );

        // Test initialized program account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                    AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), true),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized),
            bank_client
                .send_and_confirm_message(&[&mint_keypair, &upgrade_authority_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test initialized ProgramData account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::Custom(0)),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test deploy no authority
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &program_account);
        bank.store_account(&programdata_address, &programdata_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::NotEnoughAccountKeys),
            bank_client
                .send_and_confirm_message(&[&mint_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test deploy authority not a signer
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &program_account);
        bank.store_account(&programdata_address, &programdata_account);
        let message = Message::new(
            &[Instruction::new_with_bincode(
                bpf_loader_upgradeable::id(),
                &UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                    max_data_len: elf.len(),
                },
                vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(programdata_address, false),
                    AccountMeta::new(program_keypair.pubkey(), false),
                    AccountMeta::new(buffer_address, false),
                    AccountMeta::new_readonly(sysvar::rent::id(), false),
                    AccountMeta::new_readonly(sysvar::clock::id(), false),
                    AccountMeta::new_readonly(system_program::id(), false),
                    AccountMeta::new_readonly(upgrade_authority_keypair.pubkey(), false),
                ],
            )],
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature),
            bank_client
                .send_and_confirm_message(&[&mint_keypair], message)
                .unwrap_err()
                .unwrap()
        );

        // Test invalid Buffer account state
        bank.clear_signatures();
        bank.store_account(&buffer_address, &AccountSharedData::default());
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account not rent exempt
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance.saturating_sub(1),
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account not rent exempt because data is larger than needed
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions.get_mut(0).unwrap() = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            (UpgradeableLoaderState::size_of_program() as u64).saturating_add(1),
            &id(),
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::ExecutableAccountNotRentExempt),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test program account too small
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions.get_mut(0).unwrap() = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            (UpgradeableLoaderState::size_of_program() as u64).saturating_sub(1),
            &id(),
        );
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test Insufficient payer funds (need more funds to cover the
        // difference between buffer lamports and programdata lamports)
        bank.clear_signatures();
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(
                deploy_fees.saturating_add(min_program_balance),
                0,
                &system_program::id(),
            ),
        );
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::Custom(1)),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(1_000_000_000, 0, &system_program::id()),
        );

        // Test max_data_len
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len().saturating_sub(1),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::AccountDataTooSmall),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test max_data_len too large
        bank.clear_signatures();
        bank.store_account(
            &mint_keypair.pubkey(),
            &AccountSharedData::new(u64::MAX / 2, 0, &system_program::id()),
        );
        let mut modified_buffer_account = buffer_account.clone();
        modified_buffer_account.set_lamports(u64::MAX / 2);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                usize::MAX,
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidArgument),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test not the system account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let mut instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            &buffer_address,
            &upgrade_authority_keypair.pubkey(),
            min_program_balance,
            elf.len(),
        )
        .unwrap();
        *instructions
            .get_mut(1)
            .unwrap()
            .accounts
            .get_mut(6)
            .unwrap() = AccountMeta::new_readonly(Pubkey::new_unique(), false);
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::MissingAccount),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test Bad ELF data
        bank.clear_signatures();
        let mut modified_buffer_account = buffer_account;
        truncate_data(
            &mut modified_buffer_account,
            UpgradeableLoaderState::size_of_buffer(1),
        );
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Test small buffer account
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        truncate_data(&mut modified_buffer_account, 5);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::InvalidAccountData),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Mismatched buffer and program authority
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );

        // Deploy buffer with mismatched None authority
        bank.clear_signatures();
        let mut modified_buffer_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::size_of_buffer(elf.len()),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        modified_buffer_account
            .data_as_mut_slice()
            .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
            .unwrap()
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &AccountSharedData::default());
        bank.store_account(&programdata_address, &AccountSharedData::default());
        let message = Message::new(
            &bpf_loader_upgradeable::deploy_with_max_program_len(
                &mint_keypair.pubkey(),
                &program_keypair.pubkey(),
                &buffer_address,
                &upgrade_authority_keypair.pubkey(),
                min_program_balance,
                elf.len(),
            )
            .unwrap(),
            Some(&mint_keypair.pubkey()),
        );
        assert_eq!(
            TransactionError::InstructionError(1, InstructionError::IncorrectAuthority),
            bank_client
                .send_and_confirm_message(
                    &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                    message
                )
                .unwrap_err()
                .unwrap()
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_upgrade() {
        let mut file = File::open("test_elfs/out/noop_aligned.so").expect("file open failed");
        let mut elf_orig = Vec::new();
        file.read_to_end(&mut elf_orig).unwrap();
        let mut file = File::open("test_elfs/out/noop_unaligned.so").expect("file open failed");
        let mut elf_new = Vec::new();
        file.read_to_end(&mut elf_new).unwrap();
        assert_ne!(elf_orig.len(), elf_new.len());
        const SLOT: u64 = 42;
        let buffer_address = Pubkey::new_unique();
        let upgrade_authority_address = Pubkey::new_unique();

        fn get_accounts(
            buffer_address: &Pubkey,
            buffer_authority: &Pubkey,
            upgrade_authority_address: &Pubkey,
            elf_orig: &[u8],
            elf_new: &[u8],
        ) -> (Vec<(Pubkey, AccountSharedData)>, Vec<AccountMeta>) {
            let loader_id = bpf_loader_upgradeable::id();
            let program_address = Pubkey::new_unique();
            let spill_address = Pubkey::new_unique();
            let rent = Rent::default();
            let min_program_balance =
                1.max(rent.minimum_balance(UpgradeableLoaderState::size_of_program()));
            let min_programdata_balance = 1.max(rent.minimum_balance(
                UpgradeableLoaderState::size_of_programdata(elf_orig.len().max(elf_new.len())),
            ));
            let (programdata_address, _) =
                Pubkey::find_program_address(&[program_address.as_ref()], &loader_id);
            let mut buffer_account = AccountSharedData::new(
                1,
                UpgradeableLoaderState::size_of_buffer(elf_new.len()),
                &bpf_loader_upgradeable::id(),
            );
            buffer_account
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(*buffer_authority),
                })
                .unwrap();
            buffer_account
                .data_as_mut_slice()
                .get_mut(UpgradeableLoaderState::size_of_buffer_metadata()..)
                .unwrap()
                .copy_from_slice(elf_new);
            let mut programdata_account = AccountSharedData::new(
                min_programdata_balance,
                UpgradeableLoaderState::size_of_programdata(elf_orig.len().max(elf_new.len())),
                &bpf_loader_upgradeable::id(),
            );
            programdata_account
                .set_state(&UpgradeableLoaderState::ProgramData {
                    slot: SLOT,
                    upgrade_authority_address: Some(*upgrade_authority_address),
                })
                .unwrap();
            let mut program_account = AccountSharedData::new(
                min_program_balance,
                UpgradeableLoaderState::size_of_program(),
                &bpf_loader_upgradeable::id(),
            );
            program_account.set_executable(true);
            program_account
                .set_state(&UpgradeableLoaderState::Program {
                    programdata_address,
                })
                .unwrap();
            let spill_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
            let rent_account = create_account_for_test(&rent);
            let clock_account = create_account_for_test(&Clock {
                slot: SLOT,
                ..Clock::default()
            });
            let upgrade_authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
            let transaction_accounts = vec![
                (programdata_address, programdata_account),
                (program_address, program_account),
                (*buffer_address, buffer_account),
                (spill_address, spill_account),
                (sysvar::rent::id(), rent_account),
                (sysvar::clock::id(), clock_account),
                (*upgrade_authority_address, upgrade_authority_account),
            ];
            let instruction_accounts = vec![
                AccountMeta {
                    pubkey: programdata_address,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: program_address,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: *buffer_address,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: spill_address,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: *upgrade_authority_address,
                    is_signer: true,
                    is_writable: false,
                },
            ];
            (transaction_accounts, instruction_accounts)
        }

        fn process_instruction(
            transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
            instruction_accounts: Vec<AccountMeta>,
            expected_result: Result<(), InstructionError>,
        ) -> Vec<AccountSharedData> {
            let instruction_data =
                bincode::serialize(&UpgradeableLoaderInstruction::Upgrade).unwrap();
            mock_process_instruction(
                &bpf_loader_upgradeable::id(),
                Vec::new(),
                &instruction_data,
                transaction_accounts,
                instruction_accounts,
                None,
                None,
                expected_result,
                super::process_instruction,
            )
        }

        // Case: Success
        let (transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        let accounts = process_instruction(transaction_accounts, instruction_accounts, Ok(()));
        let min_programdata_balance = Rent::default().minimum_balance(
            UpgradeableLoaderState::size_of_programdata(elf_orig.len().max(elf_new.len())),
        );
        assert_eq!(
            min_programdata_balance,
            accounts.first().unwrap().lamports()
        );
        assert_eq!(0, accounts.get(2).unwrap().lamports());
        assert_eq!(1, accounts.get(3).unwrap().lamports());
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot: SLOT,
                upgrade_authority_address: Some(upgrade_authority_address)
            }
        );
        for (i, byte) in accounts
            .first()
            .unwrap()
            .data()
            .get(
                UpgradeableLoaderState::size_of_programdata_metadata()
                    ..UpgradeableLoaderState::size_of_programdata(elf_new.len()),
            )
            .unwrap()
            .iter()
            .enumerate()
        {
            assert_eq!(*elf_new.get(i).unwrap(), *byte);
        }

        // Case: not upgradable
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(0)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot: SLOT,
                upgrade_authority_address: None,
            })
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::Immutable),
        );

        // Case: wrong authority
        let (mut transaction_accounts, mut instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        let invalid_upgrade_authority_address = Pubkey::new_unique();
        transaction_accounts.get_mut(6).unwrap().0 = invalid_upgrade_authority_address;
        instruction_accounts.get_mut(6).unwrap().pubkey = invalid_upgrade_authority_address;
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: authority did not sign
        let (transaction_accounts, mut instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        instruction_accounts.get_mut(6).unwrap().is_signer = false;
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: Program account not executable
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(1)
            .unwrap()
            .1
            .set_executable(false);
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::AccountNotExecutable),
        );

        // Case: Program account now owned by loader
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(1)
            .unwrap()
            .1
            .set_owner(Pubkey::new_unique());
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectProgramId),
        );

        // Case: Program account not writable
        let (transaction_accounts, mut instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        instruction_accounts.get_mut(1).unwrap().is_writable = false;
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidArgument),
        );

        // Case: Program account not initialized
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(1)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidAccountData),
        );

        // Case: Program ProgramData account mismatch
        let (mut transaction_accounts, mut instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        let invalid_programdata_address = Pubkey::new_unique();
        transaction_accounts.get_mut(0).unwrap().0 = invalid_programdata_address;
        instruction_accounts.get_mut(0).unwrap().pubkey = invalid_programdata_address;
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidArgument),
        );

        // Case: Buffer account not initialized
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(2)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidArgument),
        );

        // Case: Buffer account too big
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts.get_mut(2).unwrap().1 = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_buffer(
                elf_orig.len().max(elf_new.len()).saturating_add(1),
            ),
            &bpf_loader_upgradeable::id(),
        );
        transaction_accounts
            .get_mut(2)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::AccountDataTooSmall),
        );

        // Test small buffer account
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(2)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        truncate_data(&mut transaction_accounts.get_mut(2).unwrap().1, 5);
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidAccountData),
        );

        // Case: Mismatched buffer and program authority
        let (transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &buffer_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: None buffer authority
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &buffer_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(2)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: None buffer and program authority
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &buffer_address,
            &upgrade_authority_address,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts
            .get_mut(0)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot: SLOT,
                upgrade_authority_address: None,
            })
            .unwrap();
        transaction_accounts
            .get_mut(2)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        process_instruction(
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectAuthority),
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_set_upgrade_authority() {
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap();
        let loader_id = bpf_loader_upgradeable::id();
        let slot = 0;
        let upgrade_authority_address = Pubkey::new_unique();
        let upgrade_authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let new_upgrade_authority_address = Pubkey::new_unique();
        let new_upgrade_authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let program_address = Pubkey::new_unique();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let mut programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_programdata(0),
            &bpf_loader_upgradeable::id(),
        );
        programdata_account
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        let programdata_meta = AccountMeta {
            pubkey: programdata_address,
            is_signer: false,
            is_writable: true,
        };
        let upgrade_authority_meta = AccountMeta {
            pubkey: upgrade_authority_address,
            is_signer: true,
            is_writable: false,
        };
        let new_upgrade_authority_meta = AccountMeta {
            pubkey: new_upgrade_authority_address,
            is_signer: false,
            is_writable: false,
        };

        // Case: Set to new authority
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (upgrade_authority_address, upgrade_authority_account.clone()),
                (
                    new_upgrade_authority_address,
                    new_upgrade_authority_account.clone(),
                ),
            ],
            vec![
                programdata_meta.clone(),
                upgrade_authority_meta.clone(),
                new_upgrade_authority_meta.clone(),
            ],
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(new_upgrade_authority_address),
            }
        );

        // Case: Not upgradeable
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (upgrade_authority_address, upgrade_authority_account.clone()),
            ],
            vec![programdata_meta.clone(), upgrade_authority_meta.clone()],
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            }
        );

        // Case: Authority did not sign
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (upgrade_authority_address, upgrade_authority_account.clone()),
            ],
            vec![
                programdata_meta.clone(),
                AccountMeta {
                    pubkey: upgrade_authority_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: wrong authority
        let invalid_upgrade_authority_address = Pubkey::new_unique();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (
                    invalid_upgrade_authority_address,
                    upgrade_authority_account.clone(),
                ),
                (new_upgrade_authority_address, new_upgrade_authority_account),
            ],
            vec![
                programdata_meta.clone(),
                AccountMeta {
                    pubkey: invalid_upgrade_authority_address,
                    is_signer: true,
                    is_writable: false,
                },
                new_upgrade_authority_meta,
            ],
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: No authority
        programdata_account
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (upgrade_authority_address, upgrade_authority_account.clone()),
            ],
            vec![programdata_meta.clone(), upgrade_authority_meta.clone()],
            Err(InstructionError::Immutable),
        );

        // Case: Not a ProgramData account
        programdata_account
            .set_state(&UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (upgrade_authority_address, upgrade_authority_account),
            ],
            vec![programdata_meta, upgrade_authority_meta],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_set_buffer_authority() {
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap();
        let loader_id = bpf_loader_upgradeable::id();
        let invalid_authority_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let new_authority_address = Pubkey::new_unique();
        let new_authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let buffer_address = Pubkey::new_unique();
        let mut buffer_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(0), &loader_id);
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        let mut transaction_accounts = vec![
            (buffer_address, buffer_account.clone()),
            (authority_address, authority_account.clone()),
            (new_authority_address, new_authority_account.clone()),
        ];
        let buffer_meta = AccountMeta {
            pubkey: buffer_address,
            is_signer: false,
            is_writable: true,
        };
        let authority_meta = AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        };
        let new_authority_meta = AccountMeta {
            pubkey: new_authority_address,
            is_signer: false,
            is_writable: false,
        };

        // Case: New authority required
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![buffer_meta.clone(), authority_meta.clone()],
            Err(InstructionError::IncorrectAuthority),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            }
        );

        // Case: Set to new authority
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![
                buffer_meta.clone(),
                authority_meta.clone(),
                new_authority_meta.clone(),
            ],
            Ok(()),
        );
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(new_authority_address),
            }
        );

        // Case: Authority did not sign
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![
                buffer_meta.clone(),
                AccountMeta {
                    pubkey: authority_address,
                    is_signer: false,
                    is_writable: false,
                },
                new_authority_meta.clone(),
            ],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Case: wrong authority
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (buffer_address, buffer_account.clone()),
                (invalid_authority_address, authority_account),
                (new_authority_address, new_authority_account),
            ],
            vec![
                buffer_meta.clone(),
                AccountMeta {
                    pubkey: invalid_authority_address,
                    is_signer: true,
                    is_writable: false,
                },
                new_authority_meta.clone(),
            ],
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: No authority
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![buffer_meta.clone(), authority_meta.clone()],
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: Set to no authority
        transaction_accounts
            .get_mut(0)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![
                buffer_meta.clone(),
                authority_meta.clone(),
                new_authority_meta.clone(),
            ],
            Err(InstructionError::Immutable),
        );

        // Case: Not a Buffer account
        transaction_accounts
            .get_mut(0)
            .unwrap()
            .1
            .set_state(&UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(),
            })
            .unwrap();
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts.clone(),
            vec![buffer_meta, authority_meta, new_authority_meta],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_close() {
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Close).unwrap();
        let loader_id = bpf_loader_upgradeable::id();
        let invalid_authority_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let authority_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let recipient_address = Pubkey::new_unique();
        let recipient_account = AccountSharedData::new(1, 0, &Pubkey::new_unique());
        let buffer_address = Pubkey::new_unique();
        let mut buffer_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_buffer(0), &loader_id);
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        let uninitialized_address = Pubkey::new_unique();
        let mut uninitialized_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_programdata(0),
            &loader_id,
        );
        uninitialized_account
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        let programdata_address = Pubkey::new_unique();
        let mut programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::size_of_programdata(0),
            &loader_id,
        );
        programdata_account
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(authority_address),
            })
            .unwrap();
        let program_address = Pubkey::new_unique();
        let mut program_account =
            AccountSharedData::new(1, UpgradeableLoaderState::size_of_program(), &loader_id);
        program_account.set_executable(true);
        program_account
            .set_state(&UpgradeableLoaderState::Program {
                programdata_address,
            })
            .unwrap();
        let transaction_accounts = vec![
            (buffer_address, buffer_account.clone()),
            (recipient_address, recipient_account.clone()),
            (authority_address, authority_account.clone()),
        ];
        let buffer_meta = AccountMeta {
            pubkey: buffer_address,
            is_signer: false,
            is_writable: true,
        };
        let recipient_meta = AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: true,
        };
        let authority_meta = AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        };

        // Case: close a buffer account
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            transaction_accounts,
            vec![
                buffer_meta.clone(),
                recipient_meta.clone(),
                authority_meta.clone(),
            ],
            Ok(()),
        );
        assert_eq!(0, accounts.first().unwrap().lamports());
        assert_eq!(2, accounts.get(1).unwrap().lamports());
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(state, UpgradeableLoaderState::Uninitialized);

        // Case: close with wrong authority
        process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (buffer_address, buffer_account.clone()),
                (recipient_address, recipient_account.clone()),
                (invalid_authority_address, authority_account.clone()),
            ],
            vec![
                buffer_meta,
                recipient_meta.clone(),
                AccountMeta {
                    pubkey: invalid_authority_address,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::IncorrectAuthority),
        );

        // Case: close an uninitialized account
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (uninitialized_address, uninitialized_account.clone()),
                (recipient_address, recipient_account.clone()),
                (invalid_authority_address, authority_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: uninitialized_address,
                    is_signer: false,
                    is_writable: true,
                },
                recipient_meta.clone(),
                authority_meta.clone(),
            ],
            Ok(()),
        );
        assert_eq!(0, accounts.first().unwrap().lamports());
        assert_eq!(2, accounts.get(1).unwrap().lamports());
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(state, UpgradeableLoaderState::Uninitialized);

        // Case: close a program account
        let accounts = process_instruction(
            &loader_id,
            &[],
            &instruction,
            vec![
                (programdata_address, programdata_account.clone()),
                (recipient_address, recipient_account),
                (authority_address, authority_account),
                (program_address, program_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: programdata_address,
                    is_signer: false,
                    is_writable: true,
                },
                recipient_meta,
                authority_meta,
                AccountMeta {
                    pubkey: program_address,
                    is_signer: false,
                    is_writable: true,
                },
            ],
            Ok(()),
        );
        assert_eq!(0, accounts.first().unwrap().lamports());
        assert_eq!(2, accounts.get(1).unwrap().lamports());
        let state: UpgradeableLoaderState = accounts.first().unwrap().state().unwrap();
        assert_eq!(state, UpgradeableLoaderState::Uninitialized);

        // Try to invoke closed account
        process_instruction(
            &program_address,
            &[0, 1],
            &[],
            vec![
                (programdata_address, programdata_account.clone()),
                (program_address, program_account.clone()),
            ],
            Vec::new(),
            Err(InstructionError::InvalidAccountData),
        );
    }

    /// fuzzing utility function
    fn fuzz<F>(
        bytes: &[u8],
        outer_iters: usize,
        inner_iters: usize,
        offset: Range<usize>,
        value: Range<u8>,
        work: F,
    ) where
        F: Fn(&mut [u8]),
    {
        let mut rng = rand::thread_rng();
        for _ in 0..outer_iters {
            let mut mangled_bytes = bytes.to_vec();
            for _ in 0..inner_iters {
                let offset = rng.gen_range(offset.start, offset.end);
                let value = rng.gen_range(value.start, value.end);
                *mangled_bytes.get_mut(offset).unwrap() = value;
                work(&mut mangled_bytes);
            }
        }
    }

    #[test]
    #[ignore]
    fn test_fuzz() {
        let loader_id = bpf_loader::id();
        let program_id = Pubkey::new_unique();

        // Create program account
        let mut file = File::open("test_elfs/out/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        // Mangle the whole file
        fuzz(
            &elf,
            1_000_000_000,
            100,
            0..elf.len(),
            0..255,
            |bytes: &mut [u8]| {
                let mut program_account = AccountSharedData::new(1, 0, &loader_id);
                program_account.set_data(bytes.to_vec());
                program_account.set_executable(true);
                process_instruction(
                    &loader_id,
                    &[],
                    &[],
                    vec![(program_id, program_account)],
                    Vec::new(),
                    Ok(()),
                );
            },
        );
    }
}
