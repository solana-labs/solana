#![allow(clippy::integer_arithmetic)]
pub mod alloc;
pub mod allocator_bump;
pub mod deprecated;
pub mod serialization;
pub mod syscalls;
pub mod upgradeable;
pub mod upgradeable_with_jit;
pub mod with_jit;

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
    },
    solana_rbpf::{
        aligned_memory::AlignedMemory,
        ebpf::HOST_ALIGN,
        elf::Executable,
        error::{EbpfError, UserDefinedError},
        static_analysis::Analysis,
        verifier::{self, VerifierError},
        vm::{Config, EbpfVm, InstructionMeter},
    },
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        account_utils::State,
        bpf_loader, bpf_loader_deprecated,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::Clock,
        entrypoint::{HEAP_LENGTH, SUCCESS},
        feature_set::{
            cap_accounts_data_len, do_support_realloc, reduce_required_deploy_balance,
            reject_all_elf_rw, reject_deployment_of_unresolved_syscalls,
            reject_section_virtual_address_file_offset_mismatch, requestable_heap_size,
            start_verify_shift32_imm, stop_verify_mul64_imm_nonzero,
        },
        instruction::{AccountMeta, InstructionError},
        keyed_account::{from_keyed_account, keyed_account_at_index, KeyedAccount},
        loader_instruction::LoaderInstruction,
        loader_upgradeable_instruction::UpgradeableLoaderInstruction,
        program_error::ACCOUNTS_DATA_BUDGET_EXCEEDED,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        rent::Rent,
        system_instruction::{self, MAX_PERMITTED_DATA_LENGTH},
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
#[derive(Debug, Error, PartialEq)]
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

pub fn create_executor(
    programdata_account_index: usize,
    programdata_offset: usize,
    invoke_context: &mut InvokeContext,
    use_jit: bool,
    reject_deployment_of_broken_elfs: bool,
) -> Result<Arc<BpfExecutor>, InstructionError> {
    let syscall_registry = syscalls::register_syscalls(invoke_context).map_err(|e| {
        ic_msg!(invoke_context, "Failed to register syscalls: {}", e);
        InstructionError::ProgramEnvironmentSetupFailure
    })?;
    let compute_budget = invoke_context.get_compute_budget();
    let config = Config {
        max_call_depth: compute_budget.max_call_depth,
        stack_frame_size: compute_budget.stack_frame_size,
        enable_instruction_tracing: log_enabled!(Trace),
        reject_unresolved_syscalls: reject_deployment_of_broken_elfs
            && invoke_context
                .feature_set
                .is_active(&reject_deployment_of_unresolved_syscalls::id()),
        reject_section_virtual_address_file_offset_mismatch: reject_deployment_of_broken_elfs
            && invoke_context
                .feature_set
                .is_active(&reject_section_virtual_address_file_offset_mismatch::id()),
        verify_mul64_imm_nonzero: !invoke_context
            .feature_set
            .is_active(&stop_verify_mul64_imm_nonzero::id()),
        verify_shift32_imm: invoke_context
            .feature_set
            .is_active(&start_verify_shift32_imm::id()),
        reject_all_writable_sections: invoke_context
            .feature_set
            .is_active(&reject_all_elf_rw::id()),
        ..Config::default()
    };
    let mut executable = {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;
        let programdata = keyed_account_at_index(keyed_accounts, programdata_account_index)?;
        Executable::<BpfError, ThisInstructionMeter>::from_elf(
            &programdata.try_account_ref()?.data()[programdata_offset..],
            None,
            config,
            syscall_registry,
        )
    }
    .map_err(|e| map_ebpf_error(invoke_context, e))?;
    let text_bytes = executable.get_text_bytes().1;
    verifier::check(text_bytes, &config)
        .map_err(|e| map_ebpf_error(invoke_context, EbpfError::UserError(e.into())))?;
    if use_jit {
        if let Err(err) = Executable::<BpfError, ThisInstructionMeter>::jit_compile(&mut executable)
        {
            ic_msg!(invoke_context, "Failed to compile program {:?}", err);
            return Err(InstructionError::ProgramFailedToCompile);
        }
    }
    Ok(Arc::new(BpfExecutor { executable }))
}

fn write_program_data(
    program_account_index: usize,
    program_data_offset: usize,
    bytes: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;
    let program = keyed_account_at_index(keyed_accounts, program_account_index)?;
    let mut account = program.try_account_ref_mut()?;
    let data = &mut account.data_as_mut_slice();
    let len = bytes.len();
    if data.len() < program_data_offset + len {
        ic_msg!(
            invoke_context,
            "Write overflow: {} < {}",
            data.len(),
            program_data_offset + len
        );
        return Err(InstructionError::AccountDataTooSmall);
    }
    data[program_data_offset..program_data_offset + len].copy_from_slice(bytes);
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
    invoke_context: &'a mut InvokeContext<'b>,
    orig_data_lens: &'a [usize],
) -> Result<EbpfVm<'a, BpfError, ThisInstructionMeter>, EbpfError<BpfError>> {
    let compute_budget = invoke_context.get_compute_budget();
    let heap_size = compute_budget.heap_size.unwrap_or(HEAP_LENGTH);
    if invoke_context
        .feature_set
        .is_active(&requestable_heap_size::id())
    {
        let _ = invoke_context
            .get_compute_meter()
            .borrow_mut()
            .consume((heap_size as u64 / (32 * 1024)).saturating_sub(1) * compute_budget.heap_cost);
    }
    let mut heap =
        AlignedMemory::new_with_size(compute_budget.heap_size.unwrap_or(HEAP_LENGTH), HOST_ALIGN);
    let mut vm = EbpfVm::new(program, heap.as_slice_mut(), parameter_bytes)?;
    syscalls::bind_syscall_context_objects(&mut vm, invoke_context, heap, orig_data_lens)?;
    Ok(vm)
}

pub fn process_instruction(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(
        first_instruction_account,
        instruction_data,
        invoke_context,
        false,
    )
}

pub fn process_instruction_jit(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(
        first_instruction_account,
        instruction_data,
        invoke_context,
        true,
    )
}

fn process_instruction_common(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let program_id = invoke_context.transaction_context.get_program_key()?;

    let keyed_accounts = invoke_context.get_keyed_accounts()?;
    let first_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
    let second_account = keyed_account_at_index(keyed_accounts, first_instruction_account + 1);
    let (program, next_first_instruction_account) = if first_account.unsigned_key() == program_id {
        (first_account, first_instruction_account)
    } else if second_account
        .as_ref()
        .map(|keyed_account| keyed_account.unsigned_key() == program_id)
        .unwrap_or(false)
    {
        (second_account?, first_instruction_account + 1)
    } else {
        if first_account.executable()? {
            ic_logger_msg!(log_collector, "BPF loader is executable");
            return Err(InstructionError::IncorrectProgramId);
        }
        (first_account, first_instruction_account)
    };

    if program.executable()? {
        debug_assert_eq!(
            first_instruction_account,
            1 - (invoke_context.invoke_depth() > 1) as usize,
        );

        if !check_loader_id(&program.owner()?) {
            ic_logger_msg!(
                log_collector,
                "Executable account not owned by the BPF loader"
            );
            return Err(InstructionError::IncorrectProgramId);
        }

        let program_data_offset = if bpf_loader_upgradeable::check_id(&program.owner()?) {
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program.state()?
            {
                if programdata_address != *first_account.unsigned_key() {
                    ic_logger_msg!(
                        log_collector,
                        "Wrong ProgramData account for this Program account"
                    );
                    return Err(InstructionError::InvalidArgument);
                }
                if !matches!(
                    first_account.state()?,
                    UpgradeableLoaderState::ProgramData {
                        slot: _,
                        upgrade_authority_address: _,
                    }
                ) {
                    ic_logger_msg!(log_collector, "Program has been closed");
                    return Err(InstructionError::InvalidAccountData);
                }
                UpgradeableLoaderState::programdata_data_offset()?
            } else {
                ic_logger_msg!(log_collector, "Invalid Program account");
                return Err(InstructionError::InvalidAccountData);
            }
        } else {
            0
        };

        let mut get_or_create_executor_time = Measure::start("get_or_create_executor_time");
        let executor = match invoke_context.get_executor(program_id) {
            Some(executor) => executor,
            None => {
                let executor = create_executor(
                    first_instruction_account,
                    program_data_offset,
                    invoke_context,
                    use_jit,
                    false,
                )?;
                let program_id = invoke_context.transaction_context.get_program_key()?;
                invoke_context.add_executor(program_id, executor.clone());
                executor
            }
        };
        get_or_create_executor_time.stop();
        invoke_context.timings.get_or_create_executor_us += get_or_create_executor_time.as_us();

        executor.execute(
            next_first_instruction_account,
            instruction_data,
            invoke_context,
            use_jit,
        )
    } else {
        debug_assert_eq!(first_instruction_account, 1);
        if bpf_loader_upgradeable::check_id(program_id) {
            process_loader_upgradeable_instruction(
                first_instruction_account,
                instruction_data,
                invoke_context,
                use_jit,
            )
        } else if bpf_loader::check_id(program_id) || bpf_loader_deprecated::check_id(program_id) {
            process_loader_instruction(
                first_instruction_account,
                instruction_data,
                invoke_context,
                use_jit,
            )
        } else {
            ic_logger_msg!(log_collector, "Invalid BPF loader id");
            Err(InstructionError::IncorrectProgramId)
        }
    }
}

fn process_loader_upgradeable_instruction(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let log_collector = invoke_context.get_log_collector();
    let program_id = invoke_context.transaction_context.get_program_key()?;
    let keyed_accounts = invoke_context.get_keyed_accounts()?;

    match limited_deserialize(instruction_data)? {
        UpgradeableLoaderInstruction::InitializeBuffer => {
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account)?;

            if UpgradeableLoaderState::Uninitialized != buffer.state()? {
                ic_logger_msg!(log_collector, "Buffer account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }

            let authority = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;

            buffer.set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(*authority.unsigned_key()),
            })?;
        }
        UpgradeableLoaderInstruction::Write { offset, bytes } => {
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let authority = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if authority_address.is_none() {
                    ic_logger_msg!(log_collector, "Buffer is immutable");
                    return Err(InstructionError::Immutable); // TODO better error code
                }
                if authority_address != Some(*authority.unsigned_key()) {
                    ic_logger_msg!(log_collector, "Incorrect buffer authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority.signer_key().is_none() {
                    ic_logger_msg!(log_collector, "Buffer authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidAccountData);
            }
            write_program_data(
                first_instruction_account,
                UpgradeableLoaderState::buffer_data_offset()? + offset as usize,
                &bytes,
                invoke_context,
            )?;
        }
        UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
            let payer = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let programdata =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let program = keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?;
            let rent = from_keyed_account::<Rent>(keyed_account_at_index(
                keyed_accounts,
                first_instruction_account + 4,
            )?)?;
            let clock = from_keyed_account::<Clock>(keyed_account_at_index(
                keyed_accounts,
                first_instruction_account + 5,
            )?)?;
            let authority = keyed_account_at_index(keyed_accounts, first_instruction_account + 7)?;
            let upgrade_authority_address = Some(*authority.unsigned_key());
            let upgrade_authority_signer = authority.signer_key().is_none();

            // Verify Program account

            if UpgradeableLoaderState::Uninitialized != program.state()? {
                ic_logger_msg!(log_collector, "Program account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }
            if program.data_len()? < UpgradeableLoaderState::program_len()? {
                ic_logger_msg!(log_collector, "Program account too small");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if program.lamports()? < rent.minimum_balance(program.data_len()?) {
                ic_logger_msg!(log_collector, "Program account not rent-exempt");
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }

            let new_program_id = *program.unsigned_key();

            // Verify Buffer account

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if authority_address != upgrade_authority_address {
                    ic_logger_msg!(log_collector, "Buffer and upgrade authority don't match");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if upgrade_authority_signer {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }

            let buffer_data_offset = UpgradeableLoaderState::buffer_data_offset()?;
            let buffer_data_len = buffer.data_len()?.saturating_sub(buffer_data_offset);
            let programdata_data_offset = UpgradeableLoaderState::programdata_data_offset()?;
            let programdata_len = UpgradeableLoaderState::programdata_len(max_data_len)?;

            if buffer.data_len()? < UpgradeableLoaderState::buffer_data_offset()?
                || buffer_data_len == 0
            {
                ic_logger_msg!(log_collector, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }
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
            if derived_address != *programdata.unsigned_key() {
                ic_logger_msg!(log_collector, "ProgramData address is not derived");
                return Err(InstructionError::InvalidArgument);
            }

            let predrain_buffer = invoke_context
                .feature_set
                .is_active(&reduce_required_deploy_balance::id());
            if predrain_buffer {
                // Drain the Buffer account to payer before paying for programdata account
                payer
                    .try_account_ref_mut()?
                    .checked_add_lamports(buffer.lamports()?)?;
                buffer.try_account_ref_mut()?.set_lamports(0);
            }

            let mut instruction = system_instruction::create_account(
                payer.unsigned_key(),
                programdata.unsigned_key(),
                1.max(rent.minimum_balance(programdata_len)),
                programdata_len as u64,
                program_id,
            );

            // pass an extra account to avoid the overly strict UnbalancedInstruction error
            instruction
                .accounts
                .push(AccountMeta::new(*buffer.unsigned_key(), false));

            let caller_program_id = invoke_context.transaction_context.get_program_key()?;
            let signers = [&[new_program_id.as_ref(), &[bump_seed]]]
                .iter()
                .map(|seeds| Pubkey::create_program_address(*seeds, caller_program_id))
                .collect::<Result<Vec<Pubkey>, solana_sdk::pubkey::PubkeyError>>()?;
            invoke_context.native_invoke(instruction, signers.as_slice())?;

            // Load and verify the program bits
            let executor = create_executor(
                first_instruction_account + 3,
                buffer_data_offset,
                invoke_context,
                use_jit,
                true,
            )?;
            invoke_context.update_executor(&new_program_id, executor);

            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            let payer = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let programdata =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let program = keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?;

            // Update the ProgramData account and record the program bits
            programdata.set_state(&UpgradeableLoaderState::ProgramData {
                slot: clock.slot,
                upgrade_authority_address,
            })?;
            programdata.try_account_ref_mut()?.data_as_mut_slice()
                [programdata_data_offset..programdata_data_offset + buffer_data_len]
                .copy_from_slice(&buffer.try_account_ref()?.data()[buffer_data_offset..]);

            // Update the Program account
            program.set_state(&UpgradeableLoaderState::Program {
                programdata_address: *programdata.unsigned_key(),
            })?;
            program.try_account_ref_mut()?.set_executable(true);

            if !predrain_buffer {
                // Drain the Buffer account back to the payer
                payer
                    .try_account_ref_mut()?
                    .checked_add_lamports(buffer.lamports()?)?;
                buffer.try_account_ref_mut()?.set_lamports(0);
            }

            ic_logger_msg!(log_collector, "Deployed program {:?}", new_program_id);
        }
        UpgradeableLoaderInstruction::Upgrade => {
            let programdata = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let program = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
            let rent = from_keyed_account::<Rent>(keyed_account_at_index(
                keyed_accounts,
                first_instruction_account + 4,
            )?)?;
            let clock = from_keyed_account::<Clock>(keyed_account_at_index(
                keyed_accounts,
                first_instruction_account + 5,
            )?)?;
            let authority = keyed_account_at_index(keyed_accounts, first_instruction_account + 6)?;

            // Verify Program account

            if !program.executable()? {
                ic_logger_msg!(log_collector, "Program account not executable");
                return Err(InstructionError::AccountNotExecutable);
            }
            if !program.is_writable() {
                ic_logger_msg!(log_collector, "Program account not writeable");
                return Err(InstructionError::InvalidArgument);
            }
            if &program.owner()? != program_id {
                ic_logger_msg!(log_collector, "Program account not owned by loader");
                return Err(InstructionError::IncorrectProgramId);
            }
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program.state()?
            {
                if programdata_address != *programdata.unsigned_key() {
                    ic_logger_msg!(log_collector, "Program and ProgramData account mismatch");
                    return Err(InstructionError::InvalidArgument);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Program account");
                return Err(InstructionError::InvalidAccountData);
            }

            let new_program_id = *program.unsigned_key();

            // Verify Buffer account

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if authority_address != Some(*authority.unsigned_key()) {
                    ic_logger_msg!(log_collector, "Buffer and upgrade authority don't match");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority.signer_key().is_none() {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }

            let buffer_data_offset = UpgradeableLoaderState::buffer_data_offset()?;
            let buffer_data_len = buffer.data_len()?.saturating_sub(buffer_data_offset);
            let programdata_data_offset = UpgradeableLoaderState::programdata_data_offset()?;
            let programdata_balance_required = 1.max(rent.minimum_balance(programdata.data_len()?));

            if buffer.data_len()? < UpgradeableLoaderState::buffer_data_offset()?
                || buffer_data_len == 0
            {
                ic_logger_msg!(log_collector, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }

            // Verify ProgramData account

            if programdata.data_len()? < UpgradeableLoaderState::programdata_len(buffer_data_len)? {
                ic_logger_msg!(log_collector, "ProgramData account not large enough");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if programdata.lamports()? + buffer.lamports()? < programdata_balance_required {
                ic_logger_msg!(
                    log_collector,
                    "Buffer account balance too low to fund upgrade"
                );
                return Err(InstructionError::InsufficientFunds);
            }
            if let UpgradeableLoaderState::ProgramData {
                slot: _,
                upgrade_authority_address,
            } = programdata.state()?
            {
                if upgrade_authority_address.is_none() {
                    ic_logger_msg!(log_collector, "Program not upgradeable");
                    return Err(InstructionError::Immutable);
                }
                if upgrade_authority_address != Some(*authority.unsigned_key()) {
                    ic_logger_msg!(log_collector, "Incorrect upgrade authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority.signer_key().is_none() {
                    ic_logger_msg!(log_collector, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(log_collector, "Invalid ProgramData account");
                return Err(InstructionError::InvalidAccountData);
            }

            // Load and verify the program bits
            let executor = create_executor(
                first_instruction_account + 2,
                buffer_data_offset,
                invoke_context,
                use_jit,
                true,
            )?;
            invoke_context.update_executor(&new_program_id, executor);

            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            let programdata = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let buffer = keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
            let spill = keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?;
            let authority = keyed_account_at_index(keyed_accounts, first_instruction_account + 6)?;

            // Update the ProgramData account, record the upgraded data, and zero
            // the rest
            programdata.set_state(&UpgradeableLoaderState::ProgramData {
                slot: clock.slot,
                upgrade_authority_address: Some(*authority.unsigned_key()),
            })?;
            programdata.try_account_ref_mut()?.data_as_mut_slice()
                [programdata_data_offset..programdata_data_offset + buffer_data_len]
                .copy_from_slice(&buffer.try_account_ref()?.data()[buffer_data_offset..]);
            programdata.try_account_ref_mut()?.data_as_mut_slice()
                [programdata_data_offset + buffer_data_len..]
                .fill(0);

            // Fund ProgramData to rent-exemption, spill the rest

            spill.try_account_ref_mut()?.checked_add_lamports(
                (programdata.lamports()? + buffer.lamports()?)
                    .saturating_sub(programdata_balance_required),
            )?;
            buffer.try_account_ref_mut()?.set_lamports(0);
            programdata
                .try_account_ref_mut()?
                .set_lamports(programdata_balance_required);

            ic_logger_msg!(log_collector, "Upgraded program {:?}", new_program_id);
        }
        UpgradeableLoaderInstruction::SetAuthority => {
            let account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let present_authority =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let new_authority =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)
                    .ok()
                    .map(|account| account.unsigned_key());

            match account.state()? {
                UpgradeableLoaderState::Buffer { authority_address } => {
                    if new_authority.is_none() {
                        ic_logger_msg!(log_collector, "Buffer authority is not optional");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if authority_address.is_none() {
                        ic_logger_msg!(log_collector, "Buffer is immutable");
                        return Err(InstructionError::Immutable);
                    }
                    if authority_address != Some(*present_authority.unsigned_key()) {
                        ic_logger_msg!(log_collector, "Incorrect buffer authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if present_authority.signer_key().is_none() {
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
                    if upgrade_authority_address != Some(*present_authority.unsigned_key()) {
                        ic_logger_msg!(log_collector, "Incorrect upgrade authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if present_authority.signer_key().is_none() {
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
            let close_account = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            let recipient_account =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            if close_account.unsigned_key() == recipient_account.unsigned_key() {
                ic_logger_msg!(
                    log_collector,
                    "Recipient is the same as the account being closed"
                );
                return Err(InstructionError::InvalidArgument);
            }

            match close_account.state()? {
                UpgradeableLoaderState::Uninitialized => {
                    recipient_account
                        .try_account_ref_mut()?
                        .checked_add_lamports(close_account.lamports()?)?;
                    close_account.try_account_ref_mut()?.set_lamports(0);

                    ic_logger_msg!(
                        log_collector,
                        "Closed Uninitialized {}",
                        close_account.unsigned_key()
                    );
                }
                UpgradeableLoaderState::Buffer { authority_address } => {
                    let authority =
                        keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;

                    common_close_account(
                        &authority_address,
                        authority,
                        close_account,
                        recipient_account,
                        &log_collector,
                    )?;

                    ic_logger_msg!(
                        log_collector,
                        "Closed Buffer {}",
                        close_account.unsigned_key()
                    );
                }
                UpgradeableLoaderState::ProgramData {
                    slot: _,
                    upgrade_authority_address: authority_address,
                } => {
                    let program_account =
                        keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?;

                    if !program_account.is_writable() {
                        ic_logger_msg!(log_collector, "Program account is not writable");
                        return Err(InstructionError::InvalidArgument);
                    }
                    if &program_account.owner()? != program_id {
                        ic_logger_msg!(log_collector, "Program account not owned by loader");
                        return Err(InstructionError::IncorrectProgramId);
                    }

                    match program_account.state()? {
                        UpgradeableLoaderState::Program {
                            programdata_address,
                        } => {
                            if programdata_address != *close_account.unsigned_key() {
                                ic_logger_msg!(
                                    log_collector,
                                    "ProgramData account does not match ProgramData account"
                                );
                                return Err(InstructionError::InvalidArgument);
                            }

                            let authority = keyed_account_at_index(
                                keyed_accounts,
                                first_instruction_account + 2,
                            )?;
                            common_close_account(
                                &authority_address,
                                authority,
                                close_account,
                                recipient_account,
                                &log_collector,
                            )?;
                        }
                        _ => {
                            ic_logger_msg!(log_collector, "Invalid Program account");
                            return Err(InstructionError::InvalidArgument);
                        }
                    }

                    ic_logger_msg!(
                        log_collector,
                        "Closed Program {}",
                        program_account.unsigned_key()
                    );
                }
                _ => {
                    ic_logger_msg!(log_collector, "Account does not support closing");
                    return Err(InstructionError::InvalidArgument);
                }
            }
        }
    }

    Ok(())
}

fn common_close_account(
    authority_address: &Option<Pubkey>,
    authority_account: &KeyedAccount,
    close_account: &KeyedAccount,
    recipient_account: &KeyedAccount,
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
) -> Result<(), InstructionError> {
    if authority_address.is_none() {
        ic_logger_msg!(log_collector, "Account is immutable");
        return Err(InstructionError::Immutable);
    }
    if *authority_address != Some(*authority_account.unsigned_key()) {
        ic_logger_msg!(log_collector, "Incorrect authority provided");
        return Err(InstructionError::IncorrectAuthority);
    }
    if authority_account.signer_key().is_none() {
        ic_logger_msg!(log_collector, "Authority did not sign");
        return Err(InstructionError::MissingRequiredSignature);
    }

    recipient_account
        .try_account_ref_mut()?
        .checked_add_lamports(close_account.lamports()?)?;
    close_account.try_account_ref_mut()?.set_lamports(0);
    close_account.set_state(&UpgradeableLoaderState::Uninitialized)?;
    Ok(())
}

fn process_loader_instruction(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let program_id = invoke_context.transaction_context.get_program_key()?;
    let keyed_accounts = invoke_context.get_keyed_accounts()?;
    let program = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
    if program.owner()? != *program_id {
        ic_msg!(
            invoke_context,
            "Executable account not owned by the BPF loader"
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    match limited_deserialize(instruction_data)? {
        LoaderInstruction::Write { offset, bytes } => {
            if program.signer_key().is_none() {
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
            if program.signer_key().is_none() {
                ic_msg!(invoke_context, "key[0] did not sign the transaction");
                return Err(InstructionError::MissingRequiredSignature);
            }

            let executor =
                create_executor(first_instruction_account, 0, invoke_context, use_jit, true)?;
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            let program = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            invoke_context.update_executor(program.unsigned_key(), executor);
            program.try_account_ref_mut()?.set_executable(true);
            ic_msg!(
                invoke_context,
                "Finalized account {:?}",
                program.unsigned_key()
            );
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
}

// Well, implement Debug for solana_rbpf::vm::Executable in solana-rbpf...
impl Debug for BpfExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BpfExecutor({:p})", self)
    }
}

impl Executor for BpfExecutor {
    fn execute<'a, 'b>(
        &self,
        _first_instruction_account: usize,
        _instruction_data: &[u8],
        invoke_context: &'a mut InvokeContext<'b>,
        use_jit: bool,
    ) -> Result<(), InstructionError> {
        let log_collector = invoke_context.get_log_collector();
        let compute_meter = invoke_context.get_compute_meter();
        let invoke_depth = invoke_context.invoke_depth();

        let mut serialize_time = Measure::start("serialize");
        let program_id = *invoke_context.transaction_context.get_program_key()?;
        let (mut parameter_bytes, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            invoke_context
                .transaction_context
                .get_current_instruction_context()?,
        )?;
        serialize_time.stop();
        let mut create_vm_time = Measure::start("create_vm");
        let mut execute_time;
        let execution_result = {
            let mut vm = match create_vm(
                &self.executable,
                parameter_bytes.as_slice_mut(),
                invoke_context,
                &account_lengths,
            ) {
                Ok(info) => info,
                Err(e) => {
                    ic_logger_msg!(log_collector, "Failed to create BPF VM: {}", e);
                    return Err(InstructionError::ProgramEnvironmentSetupFailure);
                }
            };
            create_vm_time.stop();

            execute_time = Measure::start("execute");
            stable_log::program_invoke(&log_collector, &program_id, invoke_depth);
            let mut instruction_meter = ThisInstructionMeter::new(compute_meter.clone());
            let before = compute_meter.borrow().get_remaining();
            let result = if use_jit {
                vm.execute_program_jit(&mut instruction_meter)
            } else {
                vm.execute_program_interpreted(&mut instruction_meter)
            };
            let after = compute_meter.borrow().get_remaining();
            ic_logger_msg!(
                log_collector,
                "Program {} consumed {} of {} compute units",
                &program_id,
                before - after,
                before
            );
            if log_enabled!(Trace) {
                let mut trace_buffer = Vec::<u8>::new();
                let analysis = Analysis::from_executable(&self.executable);
                vm.get_tracer().write(&mut trace_buffer, &analysis).unwrap();
                let trace_string = String::from_utf8(trace_buffer).unwrap();
                trace!("BPF Program Instruction Trace:\n{}", trace_string);
            }
            drop(vm);
            let (_returned_from_program_id, return_data) = &invoke_context.return_data;
            if !return_data.is_empty() {
                stable_log::program_return(&log_collector, &program_id, return_data);
            }
            match result {
                Ok(status) if status != SUCCESS => {
                    let error: InstructionError = if status == ACCOUNTS_DATA_BUDGET_EXCEEDED
                        && !invoke_context
                            .feature_set
                            .is_active(&cap_accounts_data_len::id())
                    {
                        // Until the cap_accounts_data_len feature is enabled, map the
                        // ACCOUNTS_DATA_BUDGET_EXCEEDED error to InvalidError
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
                &account_lengths,
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
        solana_runtime::{bank::Bank, bank_client::BankClient},
        solana_sdk::{
            account::{
                create_account_shared_data_for_test as create_account_for_test, AccountSharedData,
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
        let input = &mut [0x00];
        let mut bpf_functions = std::collections::BTreeMap::<u32, (usize, String)>::new();
        solana_rbpf::elf::register_bpf_function(&mut bpf_functions, 0, "entrypoint", false)
            .unwrap();
        let program = Executable::<BpfError, TestInstructionMeter>::from_text_bytes(
            program,
            None,
            Config::default(),
            SyscallRegistry::default(),
            bpf_functions,
        )
        .unwrap();
        let mut vm =
            EbpfVm::<BpfError, TestInstructionMeter>::new(&program, &mut [], input).unwrap();
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
        assert_eq!(&vec![0, 0, 0, 1, 2, 3], accounts[0].data());

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
            load_program_account_from_elf(&loader_id, "test_elfs/noop_aligned.so");
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
        assert!(accounts[0].executable());

        // Case: Finalize bad ELF
        program_account.data_as_mut_slice()[0] = 0;
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
            load_program_account_from_elf(&loader_id, "test_elfs/noop_aligned.so");
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
            Err(InstructionError::ProgramFailedToComplete),
            |first_instruction_account: usize,
             instruction_data: &[u8],
             invoke_context: &mut InvokeContext| {
                invoke_context
                    .get_compute_meter()
                    .borrow_mut()
                    .mock_set_remaining(0);
                super::process_instruction(
                    first_instruction_account,
                    instruction_data,
                    invoke_context,
                )
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
            load_program_account_from_elf(&loader_id, "test_elfs/noop_unaligned.so");
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
            load_program_account_from_elf(&loader_id, "test_elfs/noop_aligned.so");
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
        let buffer_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &loader_id,
        );
        let authority_address = Pubkey::new_unique();
        let authority_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &loader_id,
        );
        let instruction_data =
            bincode::serialize(&UpgradeableLoaderInstruction::InitializeBuffer).unwrap();
        let instruction_accounts = vec![
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
                (buffer_address, accounts[0].clone()),
                (authority_address, accounts[1].clone()),
            ],
            instruction_accounts,
            Err(InstructionError::AccountAlreadyInitialized),
        );
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
        let mut buffer_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &loader_id,
        );
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: buffer_address,
                is_signer: false,
                is_writable: false,
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &accounts[0].data()[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
            &[42; 9]
        );

        // Case: Write portion of the buffer
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 3,
            bytes: vec![42; 6],
        })
        .unwrap();
        let mut buffer_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &loader_id,
        );
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &accounts[0].data()[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
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
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        // Compute rent exempt balances
        let program_len = elf.len();
        let min_program_balance = bank
            .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::program_len().unwrap());
        let min_buffer_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::buffer_len(program_len).unwrap(),
        );
        let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(program_len).unwrap(),
        );

        // Setup accounts
        let buffer_account = {
            let mut account = AccountSharedData::new(
                min_buffer_balance,
                UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            account
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(upgrade_authority_keypair.pubkey()),
                })
                .unwrap();
            account.data_as_mut_slice()[UpgradeableLoaderState::buffer_data_offset().unwrap()..]
                .copy_from_slice(&elf);
            account
        };
        let program_account = AccountSharedData::new(
            min_programdata_balance,
            UpgradeableLoaderState::program_len().unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        let programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::programdata_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Test successful deploy
        let payer_base_balance = LAMPORTS_PER_SOL;
        let deploy_fees = {
            let fee_calculator = genesis_config.fee_rate_governor.create_fee_calculator();
            3 * fee_calculator.lamports_per_signature
        };
        let min_payer_balance =
            min_program_balance + min_programdata_balance - min_buffer_balance + deploy_fees;
        bank.store_account(
            &payer_keypair.pubkey(),
            &AccountSharedData::new(
                payer_base_balance + min_payer_balance,
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
            UpgradeableLoaderState::program_len().unwrap()
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
        for (i, byte) in post_programdata_account.data()
            [UpgradeableLoaderState::programdata_data_offset().unwrap()..]
            .iter()
            .enumerate()
        {
            assert_eq!(elf[i], *byte);
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
                min_program_balance - 1,
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
        instructions[0] = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            UpgradeableLoaderState::program_len().unwrap() as u64 + 1,
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
        instructions[0] = system_instruction::create_account(
            &mint_keypair.pubkey(),
            &program_keypair.pubkey(),
            min_program_balance,
            UpgradeableLoaderState::program_len().unwrap() as u64 - 1,
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
            &AccountSharedData::new(deploy_fees + min_program_balance, 0, &system_program::id()),
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
                elf.len() - 1,
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
        instructions[1].accounts[6] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
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
            UpgradeableLoaderState::buffer_len(1).unwrap(),
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
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        modified_buffer_account.data_as_mut_slice()
            [UpgradeableLoaderState::buffer_data_offset().unwrap()..]
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
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        modified_buffer_account.data_as_mut_slice()
            [UpgradeableLoaderState::buffer_data_offset().unwrap()..]
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
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        modified_buffer_account.data_as_mut_slice()
            [UpgradeableLoaderState::buffer_data_offset().unwrap()..]
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
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf_orig = Vec::new();
        file.read_to_end(&mut elf_orig).unwrap();
        let mut file = File::open("test_elfs/noop_unaligned.so").expect("file open failed");
        let mut elf_new = Vec::new();
        file.read_to_end(&mut elf_new).unwrap();
        assert_ne!(elf_orig.len(), elf_new.len());
        let slot = 42;
        let buffer_address = Pubkey::new_unique();
        let upgrade_authority_address = Pubkey::new_unique();

        fn get_accounts(
            buffer_address: &Pubkey,
            buffer_authority: &Pubkey,
            upgrade_authority_address: &Pubkey,
            slot: u64,
            elf_orig: &[u8],
            elf_new: &[u8],
        ) -> (Vec<(Pubkey, AccountSharedData)>, Vec<AccountMeta>) {
            let loader_id = bpf_loader_upgradeable::id();
            let program_address = Pubkey::new_unique();
            let spill_address = Pubkey::new_unique();
            let rent = Rent::default();
            let min_program_balance =
                1.max(rent.minimum_balance(UpgradeableLoaderState::program_len().unwrap()));
            let min_programdata_balance = 1.max(rent.minimum_balance(
                UpgradeableLoaderState::programdata_len(elf_orig.len().max(elf_new.len())).unwrap(),
            ));
            let (programdata_address, _) =
                Pubkey::find_program_address(&[program_address.as_ref()], &loader_id);
            let mut buffer_account = AccountSharedData::new(
                1,
                UpgradeableLoaderState::buffer_len(elf_new.len()).unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            buffer_account
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(*buffer_authority),
                })
                .unwrap();
            buffer_account.data_as_mut_slice()
                [UpgradeableLoaderState::buffer_data_offset().unwrap()..]
                .copy_from_slice(elf_new);
            let mut programdata_account = AccountSharedData::new(
                min_programdata_balance,
                UpgradeableLoaderState::programdata_len(elf_orig.len().max(elf_new.len())).unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            programdata_account
                .set_state(&UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address: Some(*upgrade_authority_address),
                })
                .unwrap();
            let mut program_account = AccountSharedData::new(
                min_program_balance,
                UpgradeableLoaderState::program_len().unwrap(),
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
                slot,
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
                expected_result,
                super::process_instruction,
            )
        }

        // Case: Success
        let (transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
        );
        let accounts = process_instruction(transaction_accounts, instruction_accounts, Ok(()));
        let min_programdata_balance = Rent::default().minimum_balance(
            UpgradeableLoaderState::programdata_len(elf_orig.len().max(elf_new.len())).unwrap(),
        );
        assert_eq!(min_programdata_balance, accounts[0].lamports());
        assert_eq!(0, accounts[2].lamports());
        assert_eq!(1, accounts[3].lamports());
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address)
            }
        );
        for (i, byte) in accounts[0].data()[UpgradeableLoaderState::programdata_data_offset()
            .unwrap()
            ..UpgradeableLoaderState::programdata_data_offset().unwrap() + elf_new.len()]
            .iter()
            .enumerate()
        {
            assert_eq!(elf_new[i], *byte);
        }

        // Case: not upgradable
        let (mut transaction_accounts, instruction_accounts) = get_accounts(
            &buffer_address,
            &upgrade_authority_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[0]
            .1
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
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
            slot,
            &elf_orig,
            &elf_new,
        );
        let invalid_upgrade_authority_address = Pubkey::new_unique();
        transaction_accounts[6].0 = invalid_upgrade_authority_address;
        instruction_accounts[6].pubkey = invalid_upgrade_authority_address;
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
            slot,
            &elf_orig,
            &elf_new,
        );
        instruction_accounts[6].is_signer = false;
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[1].1.set_executable(false);
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[1].1.set_owner(Pubkey::new_unique());
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
            slot,
            &elf_orig,
            &elf_new,
        );
        instruction_accounts[1].is_writable = false;
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[1]
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
            slot,
            &elf_orig,
            &elf_new,
        );
        let invalid_programdata_address = Pubkey::new_unique();
        transaction_accounts[0].0 = invalid_programdata_address;
        instruction_accounts[0].pubkey = invalid_programdata_address;
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[2]
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[2].1 = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(elf_orig.len().max(elf_new.len()) + 1).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        transaction_accounts[2]
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[2]
            .1
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        truncate_data(&mut transaction_accounts[2].1, 5);
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
            slot,
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[2]
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
            slot,
            &elf_orig,
            &elf_new,
        );
        transaction_accounts[0]
            .1
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            })
            .unwrap();
        transaction_accounts[2]
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
            UpgradeableLoaderState::programdata_len(0).unwrap(),
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
            is_writable: false,
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
        let mut buffer_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(0).unwrap(),
            &loader_id,
        );
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
            is_writable: false,
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
        transaction_accounts[0]
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
        transaction_accounts[0]
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
        let mut buffer_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::buffer_len(0).unwrap(),
            &loader_id,
        );
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        let uninitialized_address = Pubkey::new_unique();
        let mut uninitialized_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::programdata_len(0).unwrap(),
            &loader_id,
        );
        uninitialized_account
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        let programdata_address = Pubkey::new_unique();
        let mut programdata_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::programdata_len(0).unwrap(),
            &loader_id,
        );
        programdata_account
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot: 0,
                upgrade_authority_address: Some(authority_address),
            })
            .unwrap();
        let program_address = Pubkey::new_unique();
        let mut program_account = AccountSharedData::new(
            1,
            UpgradeableLoaderState::program_len().unwrap(),
            &loader_id,
        );
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
            is_writable: false,
        };
        let recipient_meta = AccountMeta {
            pubkey: recipient_address,
            is_signer: false,
            is_writable: false,
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
        assert_eq!(0, accounts[0].lamports());
        assert_eq!(2, accounts[1].lamports());
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
                    is_writable: false,
                },
                recipient_meta.clone(),
                authority_meta.clone(),
            ],
            Ok(()),
        );
        assert_eq!(0, accounts[0].lamports());
        assert_eq!(2, accounts[1].lamports());
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
                    is_writable: false,
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
        assert_eq!(0, accounts[0].lamports());
        assert_eq!(2, accounts[1].lamports());
        let state: UpgradeableLoaderState = accounts[0].state().unwrap();
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
                mangled_bytes[offset] = value;
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
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
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
