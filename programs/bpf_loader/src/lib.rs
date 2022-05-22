#![deny(clippy::integer_arithmetic)]
#![deny(clippy::indexing_slicing)]

pub mod allocator_bump;
pub mod deprecated;
pub mod serialization;
pub mod syscalls;
pub mod upgradeable;
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
        stable_log,
    },
    solana_rbpf::{
        aligned_memory::AlignedMemory,
        ebpf::{HOST_ALIGN, MM_INPUT_START},
        elf::Executable,
        error::{EbpfError, UserDefinedError},
        memory_region::MemoryRegion,
        static_analysis::Analysis,
        verifier::{self, VerifierError},
        vm::{Config, EbpfVm, InstructionMeter},
    },
    solana_sdk::{
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        entrypoint::{HEAP_LENGTH, SUCCESS},
        feature_set::{
            cap_accounts_data_len, disable_bpf_deprecated_load_instructions,
            disable_bpf_unresolved_symbols_at_runtime, disable_deploy_of_alloc_free_syscall,
            disable_deprecated_loader, do_support_realloc,
            error_on_syscall_bpf_function_hash_collisions, reject_callx_r10, requestable_heap_size,
        },
        instruction::InstructionError,
        loader_instruction::LoaderInstruction,
        program_error::MAX_ACCOUNTS_DATA_SIZE_EXCEEDED,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        saturating_add_assign,
    },
    std::{cell::RefCell, fmt::Debug, pin::Pin, rc::Rc, sync::Arc},
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
        disable_unresolved_symbols_at_runtime: invoke_context
            .feature_set
            .is_active(&disable_bpf_unresolved_symbols_at_runtime::id()),
        reject_broken_elfs: reject_deployment_of_broken_elfs,
        noop_instruction_ratio: 1.0 / 256.0,
        sanitize_user_provided_values: true,
        encrypt_environment_registers: true,
        disable_deprecated_load_instructions: reject_deployment_of_broken_elfs
            && invoke_context
                .feature_set
                .is_active(&disable_bpf_deprecated_load_instructions::id()),
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
        // Warning, do not use `Config::default()` so that configuration here is explicit.
    };
    let mut create_executor_metrics = executor_metrics::CreateMetrics::default();
    let mut executable = {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let programdata = instruction_context
            .try_borrow_account(transaction_context, programdata_account_index)?;
        create_executor_metrics.program_id = programdata.get_key().to_string();
        let mut load_elf_time = Measure::start("load_elf_time");
        let executable = Executable::<BpfError, ThisInstructionMeter>::from_elf(
            programdata
                .get_data()
                .get(programdata_offset..)
                .ok_or(InstructionError::AccountDataTooSmall)?,
            None,
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
    let text_bytes = executable.get_text_bytes().1;
    let mut verify_code_time = Measure::start("verify_code_time");
    verifier::check(text_bytes, &config)
        .map_err(|e| map_ebpf_error(invoke_context, EbpfError::UserError(e.into())))?;
    verify_code_time.stop();
    create_executor_metrics.verify_code_us = verify_code_time.as_us();
    invoke_context.timings.create_executor_verify_code_us = invoke_context
        .timings
        .create_executor_verify_code_us
        .saturating_add(create_executor_metrics.verify_code_us);
    if use_jit {
        let mut jit_compile_time = Measure::start("jit_compile_time");
        let jit_compile_result =
            Executable::<BpfError, ThisInstructionMeter>::jit_compile(&mut executable);
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
        executable,
        use_jit,
    }))
}

pub(crate) fn write_program_data(
    program_account_index: usize,
    program_data_offset: usize,
    bytes: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut program =
        instruction_context.try_borrow_account(transaction_context, program_account_index)?;
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
    program: &'a Pin<Box<Executable<BpfError, ThisInstructionMeter>>>,
    parameter_bytes: &mut [u8],
    orig_account_lengths: Vec<usize>,
    invoke_context: &'a mut InvokeContext<'b>,
) -> Result<EbpfVm<'a, BpfError, ThisInstructionMeter>, EbpfError<BpfError>> {
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size.unwrap_or(HEAP_LENGTH);
    if invoke_context
        .feature_set
        .is_active(&requestable_heap_size::id())
    {
        let _ = invoke_context.get_compute_meter().borrow_mut().consume(
            ((heap_size as u64).saturating_div(32_u64.saturating_mul(1024)))
                .saturating_sub(1)
                .saturating_mul(compute_budget.heap_cost),
        );
    }
    let mut heap =
        AlignedMemory::new_with_size(compute_budget.heap_size.unwrap_or(HEAP_LENGTH), HOST_ALIGN);
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
    let program_id = instruction_context.get_program_key(transaction_context)?;
    let first_account_key = transaction_context.get_key_of_account_at_index(
        instruction_context.get_index_in_transaction(first_instruction_account)?,
    )?;
    let second_account_key = instruction_context
        .get_index_in_transaction(first_instruction_account.saturating_add(1))
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
        if instruction_context
            .try_borrow_account(transaction_context, first_instruction_account)?
            .is_executable()
        {
            ic_logger_msg!(log_collector, "BPF loader is executable");
            return Err(InstructionError::IncorrectProgramId);
        }
        first_instruction_account
    };

    let program =
        instruction_context.try_borrow_account(transaction_context, program_account_index)?;
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
                        .try_borrow_account(transaction_context, first_instruction_account)?
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
                let program_id = instruction_context.get_program_key(transaction_context)?;
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
            upgradeable::processor::process_instruction(
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

fn process_loader_instruction(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let program_id = instruction_context.get_program_key(transaction_context)?;
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
    executable: Pin<Box<Executable<BpfError, ThisInstructionMeter>>>,
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
        let program_id = *instruction_context.get_program_key(transaction_context)?;

        let mut serialize_time = Measure::start("serialize");
        let (mut parameter_bytes, account_lengths) =
            serialize_parameters(invoke_context.transaction_context, instruction_context)?;
        serialize_time.stop();

        let mut create_vm_time = Measure::start("create_vm");
        let mut execute_time;
        let execution_result = {
            let mut vm = match create_vm(
                &self.executable,
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
                let analysis = Analysis::from_executable(&self.executable).unwrap();
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
                    let error: InstructionError = if status == MAX_ACCOUNTS_DATA_SIZE_EXCEEDED
                        && !invoke_context
                            .feature_set
                            .is_active(&cap_accounts_data_len::id())
                    {
                        // Until the cap_accounts_data_len feature is enabled, map the
                        // MAX_ACCOUNTS_DATA_SIZE_EXCEEDED error to InvalidError
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
                invoke_context
                    .feature_set
                    .is_active(&do_support_realloc::id()),
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
        solana_rbpf::vm::SyscallRegistry,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            instruction::{AccountMeta, InstructionError},
            pubkey::Pubkey,
            rent::Rent,
        },
        std::{fs::File, io::Read, ops::Range},
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
        let program = Executable::<BpfError, TestInstructionMeter>::from_text_bytes(
            program,
            None,
            config,
            syscall_registry,
            bpf_functions,
        )
        .unwrap();
        let input_region = MemoryRegion::new_writable(&mut input_mem, MM_INPUT_START);
        let mut vm =
            EbpfVm::<BpfError, TestInstructionMeter>::new(&program, &mut [], vec![input_region])
                .unwrap();
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
        verifier::check(prog, &Config::default()).unwrap();
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
                is_writable: false,
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
                is_writable: false,
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
                is_writable: false,
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
                is_writable: false,
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
                is_writable: false,
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
                is_writable: false,
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
