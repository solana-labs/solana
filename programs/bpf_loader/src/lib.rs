pub mod alloc;
pub mod allocator_bump;
pub mod bpf_verifier;
pub mod deprecated;
pub mod serialization;
pub mod syscalls;
pub mod upgradeable;
pub mod upgradeable_with_jit;
pub mod with_jit;

use crate::{
    bpf_verifier::VerifierError,
    serialization::{deserialize_parameters, serialize_parameters},
    syscalls::SyscallError,
};
use solana_rbpf::{
    ebpf::MM_HEAP_START,
    error::{EbpfError, UserDefinedError},
    memory_region::MemoryRegion,
    vm::{Config, EbpfVm, Executable, InstructionMeter},
};
use solana_runtime::message_processor::MessageProcessor;
use solana_sdk::{
    account_utils::State,
    bpf_loader, bpf_loader_deprecated,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    clock::Clock,
    entrypoint::SUCCESS,
    feature_set::{
        bpf_compute_budget_balancing, matching_buffer_upgrade_authorities,
        prevent_upgrade_and_invoke,
    },
    ic_logger_msg, ic_msg,
    instruction::InstructionError,
    keyed_account::{from_keyed_account, next_keyed_account, KeyedAccount},
    loader_instruction::LoaderInstruction,
    loader_upgradeable_instruction::UpgradeableLoaderInstruction,
    process_instruction::{stable_log, ComputeMeter, Executor, InvokeContext},
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
};
use std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};
use thiserror::Error;

solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    solana_bpf_loader_program::process_instruction
);

/// Errors returned by functions the BPF Loader registers with the VM
#[derive(Debug, Error, PartialEq)]
pub enum BPFError {
    #[error("{0}")]
    VerifierError(#[from] VerifierError),
    #[error("{0}")]
    SyscallError(#[from] SyscallError),
}
impl UserDefinedError for BPFError {}

fn map_ebpf_error(
    invoke_context: &mut dyn InvokeContext,
    e: EbpfError<BPFError>,
) -> InstructionError {
    ic_msg!(invoke_context, "{}", e);
    InstructionError::InvalidAccountData
}

pub fn create_and_cache_executor(
    key: &Pubkey,
    data: &[u8],
    invoke_context: &mut dyn InvokeContext,
    use_jit: bool,
) -> Result<Arc<BPFExecutor>, InstructionError> {
    let bpf_compute_budget = invoke_context.get_bpf_compute_budget();
    let mut program = Executable::<BPFError, ThisInstructionMeter>::from_elf(
        data,
        None,
        Config {
            max_call_depth: bpf_compute_budget.max_call_depth,
            stack_frame_size: bpf_compute_budget.stack_frame_size,
            enable_instruction_meter: true,
            enable_instruction_tracing: false,
        },
    )
    .map_err(|e| map_ebpf_error(invoke_context, e))?;
    let (_, elf_bytes) = program
        .get_text_bytes()
        .map_err(|e| map_ebpf_error(invoke_context, e))?;
    bpf_verifier::check(
        elf_bytes,
        !invoke_context.is_feature_active(&bpf_compute_budget_balancing::id()),
    )
    .map_err(|e| map_ebpf_error(invoke_context, EbpfError::UserError(e)))?;
    let syscall_registry = syscalls::register_syscalls(invoke_context).map_err(|e| {
        ic_msg!(invoke_context, "Failed to register syscalls: {}", e);
        InstructionError::ProgramEnvironmentSetupFailure
    })?;
    program.set_syscall_registry(syscall_registry);
    if use_jit {
        if let Err(err) = program.jit_compile() {
            ic_msg!(invoke_context, "Failed to compile program {:?}", err);
            return Err(InstructionError::ProgramFailedToCompile);
        }
    }
    let executor = Arc::new(BPFExecutor { program });
    invoke_context.add_executor(key, executor.clone());
    Ok(executor)
}

fn write_program_data(
    data: &mut [u8],
    offset: usize,
    bytes: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let len = bytes.len();
    if data.len() < offset + len {
        ic_msg!(
            invoke_context,
            "Write overflow: {} < {}",
            data.len(),
            offset + len
        );
        return Err(InstructionError::AccountDataTooSmall);
    }
    data[offset..offset + len].copy_from_slice(&bytes);
    Ok(())
}

fn check_loader_id(id: &Pubkey) -> bool {
    bpf_loader::check_id(id)
        || bpf_loader_deprecated::check_id(id)
        || bpf_loader_upgradeable::check_id(id)
}

/// Default program heap size, allocators
/// are expected to enforce this
const DEFAULT_HEAP_SIZE: usize = 32 * 1024;

/// Create the BPF virtual machine
pub fn create_vm<'a>(
    loader_id: &'a Pubkey,
    program: &'a dyn Executable<BPFError, ThisInstructionMeter>,
    parameter_bytes: &mut [u8],
    parameter_accounts: &'a [KeyedAccount<'a>],
    invoke_context: &'a mut dyn InvokeContext,
) -> Result<EbpfVm<'a, BPFError, ThisInstructionMeter>, EbpfError<BPFError>> {
    let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
    let heap_region = MemoryRegion::new_from_slice(&heap, MM_HEAP_START, 0, true);
    let mut vm = EbpfVm::new(program, parameter_bytes, &[heap_region])?;
    syscalls::bind_syscall_context_objects(
        loader_id,
        &mut vm,
        parameter_accounts,
        invoke_context,
        heap,
    )?;
    Ok(vm)
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(
        program_id,
        keyed_accounts,
        instruction_data,
        invoke_context,
        false,
    )
}

pub fn process_instruction_jit(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    process_instruction_common(
        program_id,
        keyed_accounts,
        instruction_data,
        invoke_context,
        true,
    )
}

fn process_instruction_common(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let logger = invoke_context.get_logger();

    let account_iter = &mut keyed_accounts.iter();
    let first_account = next_keyed_account(account_iter)?;
    if first_account.executable()? {
        if first_account.unsigned_key() != program_id {
            ic_logger_msg!(logger, "Program id mismatch");
            return Err(InstructionError::IncorrectProgramId);
        }

        let (program, keyed_accounts, offset) =
            if bpf_loader_upgradeable::check_id(&first_account.owner()?) {
                if let UpgradeableLoaderState::Program {
                    programdata_address,
                } = first_account.state()?
                {
                    let programdata = next_keyed_account(account_iter)?;
                    if programdata_address != *programdata.unsigned_key() {
                        ic_logger_msg!(
                            logger,
                            "Wrong ProgramData account for this Program account"
                        );
                        return Err(InstructionError::InvalidArgument);
                    }
                    (
                        programdata,
                        &keyed_accounts[1..],
                        UpgradeableLoaderState::programdata_data_offset()?,
                    )
                } else {
                    ic_logger_msg!(logger, "Invalid Program account");
                    return Err(InstructionError::InvalidAccountData);
                }
            } else {
                (first_account, keyed_accounts, 0)
            };

        let loader_id = &program.owner()?;

        if !check_loader_id(loader_id) {
            ic_logger_msg!(logger, "Executable account not owned by the BPF loader");
            return Err(InstructionError::IncorrectProgramId);
        }

        let executor = match invoke_context.get_executor(program_id) {
            Some(executor) => executor,
            None => create_and_cache_executor(
                program_id,
                &program.try_account_ref()?.data[offset..],
                invoke_context,
                use_jit,
            )?,
        };
        executor.execute(
            loader_id,
            program_id,
            keyed_accounts,
            instruction_data,
            invoke_context,
            use_jit,
        )?
    } else {
        if !check_loader_id(program_id) {
            ic_logger_msg!(logger, "Invalid BPF loader id");
            return Err(InstructionError::IncorrectProgramId);
        }

        if bpf_loader_upgradeable::check_id(program_id) {
            process_loader_upgradeable_instruction(
                program_id,
                keyed_accounts,
                instruction_data,
                invoke_context,
                use_jit,
            )?;
        } else {
            process_loader_instruction(
                program_id,
                keyed_accounts,
                instruction_data,
                invoke_context,
                use_jit,
            )?;
        }
    }
    Ok(())
}

fn process_loader_upgradeable_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let logger = invoke_context.get_logger();
    let account_iter = &mut keyed_accounts.iter();

    match limited_deserialize(instruction_data)? {
        UpgradeableLoaderInstruction::InitializeBuffer => {
            let buffer = next_keyed_account(account_iter)?;

            if UpgradeableLoaderState::Uninitialized != buffer.state()? {
                ic_logger_msg!(logger, "Buffer account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }

            if invoke_context.is_feature_active(&matching_buffer_upgrade_authorities::id()) {
                let authority = next_keyed_account(account_iter)?;

                buffer.set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(*authority.unsigned_key()),
                })?;
            } else {
                let authority = next_keyed_account(account_iter)
                    .ok()
                    .map(|account| account.unsigned_key());
                buffer.set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: authority.cloned(),
                })?;
            }
        }
        UpgradeableLoaderInstruction::Write { offset, bytes } => {
            let buffer = next_keyed_account(account_iter)?;
            let authority = next_keyed_account(account_iter)?;

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if authority_address == None {
                    ic_logger_msg!(logger, "Buffer is immutable");
                    return Err(InstructionError::Immutable); // TODO better error code
                }
                if authority_address != Some(*authority.unsigned_key()) {
                    ic_logger_msg!(logger, "Incorrect buffer authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority.signer_key().is_none() {
                    ic_logger_msg!(logger, "Buffer authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(logger, "Invalid Buffer account");
                return Err(InstructionError::InvalidAccountData);
            }
            write_program_data(
                &mut buffer.try_account_ref_mut()?.data,
                UpgradeableLoaderState::buffer_data_offset()? + offset as usize,
                &bytes,
                invoke_context,
            )?;
        }
        UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
            let payer = next_keyed_account(account_iter)?;
            let programdata = next_keyed_account(account_iter)?;
            let program = next_keyed_account(account_iter)?;
            let buffer = next_keyed_account(account_iter)?;
            let rent = from_keyed_account::<Rent>(next_keyed_account(account_iter)?)?;
            let clock = from_keyed_account::<Clock>(next_keyed_account(account_iter)?)?;
            let system = next_keyed_account(account_iter)?;
            let (upgrade_authority_address, upgrade_authority_signer) =
                if invoke_context.is_feature_active(&matching_buffer_upgrade_authorities::id()) {
                    let authority = next_keyed_account(account_iter)?;
                    (
                        Some(*authority.unsigned_key()),
                        authority.signer_key().is_none(),
                    )
                } else {
                    let authority = next_keyed_account(account_iter)
                        .ok()
                        .map(|account| account.unsigned_key());
                    (authority.cloned(), false)
                };

            // Verify Program account

            if UpgradeableLoaderState::Uninitialized != program.state()? {
                ic_logger_msg!(logger, "Program account already initialized");
                return Err(InstructionError::AccountAlreadyInitialized);
            }
            if program.data_len()? < UpgradeableLoaderState::program_len()? {
                ic_logger_msg!(logger, "Program account too small");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if program.lamports()? < rent.minimum_balance(program.data_len()?) {
                ic_logger_msg!(logger, "Program account not rent-exempt");
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }

            // Verify Buffer account

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if invoke_context.is_feature_active(&matching_buffer_upgrade_authorities::id()) {
                    if authority_address != upgrade_authority_address {
                        ic_logger_msg!(logger, "Buffer and upgrade authority don't match");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if upgrade_authority_signer {
                        ic_logger_msg!(logger, "Upgrade authority did not sign");
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                }
            } else {
                ic_logger_msg!(logger, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }

            let buffer_data_offset = UpgradeableLoaderState::buffer_data_offset()?;
            let buffer_data_len = buffer.data_len()?.saturating_sub(buffer_data_offset);
            let programdata_data_offset = UpgradeableLoaderState::programdata_data_offset()?;

            if buffer.data_len()? < UpgradeableLoaderState::buffer_data_offset()?
                || buffer_data_len == 0
            {
                ic_logger_msg!(logger, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }
            if max_data_len < buffer_data_len {
                ic_logger_msg!(logger, "Max data length is too small to hold Buffer data");
                return Err(InstructionError::AccountDataTooSmall);
            }

            // Create ProgramData account

            let (derived_address, bump_seed) =
                Pubkey::find_program_address(&[program.unsigned_key().as_ref()], &program_id);
            if derived_address != *programdata.unsigned_key() {
                ic_logger_msg!(logger, "ProgramData address is not derived");
                return Err(InstructionError::InvalidArgument);
            }

            MessageProcessor::native_invoke(
                invoke_context,
                system_instruction::create_account(
                    payer.unsigned_key(),
                    programdata.unsigned_key(),
                    1.max(
                        rent.minimum_balance(UpgradeableLoaderState::programdata_len(
                            max_data_len,
                        )?),
                    ),
                    UpgradeableLoaderState::programdata_len(max_data_len)? as u64,
                    program_id,
                ),
                &[payer, programdata, system],
                &[&[program.unsigned_key().as_ref(), &[bump_seed]]],
            )?;

            // Load and verify the program bits
            let _ = create_and_cache_executor(
                program_id,
                &buffer.try_account_ref()?.data[buffer_data_offset..],
                invoke_context,
                use_jit,
            )?;

            // Update the ProgramData account and record the program bits
            programdata.set_state(&UpgradeableLoaderState::ProgramData {
                slot: clock.slot,
                upgrade_authority_address,
            })?;
            programdata.try_account_ref_mut()?.data
                [programdata_data_offset..programdata_data_offset + buffer_data_len]
                .copy_from_slice(&buffer.try_account_ref()?.data[buffer_data_offset..]);
            // Update the Program account

            program.set_state(&UpgradeableLoaderState::Program {
                programdata_address: *programdata.unsigned_key(),
            })?;
            program.try_account_ref_mut()?.executable = true;

            // Drain the Buffer account back to the payer

            payer.try_account_ref_mut()?.lamports += buffer.lamports()?;
            buffer.try_account_ref_mut()?.lamports = 0;

            ic_logger_msg!(logger, "Deployed program {:?}", program.unsigned_key());
        }
        UpgradeableLoaderInstruction::Upgrade => {
            let programdata = next_keyed_account(account_iter)?;
            let program = next_keyed_account(account_iter)?;
            let buffer = next_keyed_account(account_iter)?;
            let spill = next_keyed_account(account_iter)?;
            let rent = from_keyed_account::<Rent>(next_keyed_account(account_iter)?)?;
            let clock = from_keyed_account::<Clock>(next_keyed_account(account_iter)?)?;
            let authority = next_keyed_account(account_iter)?;

            // Verify Program account

            if !program.executable()? {
                ic_logger_msg!(logger, "Program account not executable");
                return Err(InstructionError::AccountNotExecutable);
            }
            if !program.is_writable()
                && invoke_context.is_feature_active(&prevent_upgrade_and_invoke::id())
            {
                ic_logger_msg!(logger, "Program account not writeable");
                return Err(InstructionError::InvalidArgument);
            }
            if &program.owner()? != program_id {
                ic_logger_msg!(logger, "Program account not owned by loader");
                return Err(InstructionError::IncorrectProgramId);
            }
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program.state()?
            {
                if programdata_address != *programdata.unsigned_key() {
                    ic_logger_msg!(logger, "Program and ProgramData account mismatch");
                    return Err(InstructionError::InvalidArgument);
                }
            } else {
                ic_logger_msg!(logger, "Invalid Program account");
                return Err(InstructionError::InvalidAccountData);
            }

            // Verify Buffer account

            if let UpgradeableLoaderState::Buffer { authority_address } = buffer.state()? {
                if invoke_context.is_feature_active(&matching_buffer_upgrade_authorities::id()) {
                    if authority_address != Some(*authority.unsigned_key()) {
                        ic_logger_msg!(logger, "Buffer and upgrade authority don't match");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if authority.signer_key().is_none() {
                        ic_logger_msg!(logger, "Upgrade authority did not sign");
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                }
            } else {
                ic_logger_msg!(logger, "Invalid Buffer account");
                return Err(InstructionError::InvalidArgument);
            }

            let buffer_data_offset = UpgradeableLoaderState::buffer_data_offset()?;
            let buffer_data_len = buffer.data_len()?.saturating_sub(buffer_data_offset);
            let programdata_data_offset = UpgradeableLoaderState::programdata_data_offset()?;
            let programdata_balance_required = 1.max(rent.minimum_balance(programdata.data_len()?));

            if buffer.data_len()? < UpgradeableLoaderState::buffer_data_offset()?
                || buffer_data_len == 0
            {
                ic_logger_msg!(logger, "Buffer account too small");
                return Err(InstructionError::InvalidAccountData);
            }

            // Verify ProgramData account

            if programdata.data_len()? < UpgradeableLoaderState::programdata_len(buffer_data_len)? {
                ic_logger_msg!(logger, "ProgramData account not large enough");
                return Err(InstructionError::AccountDataTooSmall);
            }
            if programdata.lamports()? + buffer.lamports()? < programdata_balance_required {
                ic_logger_msg!(logger, "Buffer account balance too low to fund upgrade");
                return Err(InstructionError::InsufficientFunds);
            }
            if let UpgradeableLoaderState::ProgramData {
                slot: _,
                upgrade_authority_address,
            } = programdata.state()?
            {
                if upgrade_authority_address == None {
                    ic_logger_msg!(logger, "Program not upgradeable");
                    return Err(InstructionError::Immutable);
                }
                if upgrade_authority_address != Some(*authority.unsigned_key()) {
                    ic_logger_msg!(logger, "Incorrect upgrade authority provided");
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority.signer_key().is_none() {
                    ic_logger_msg!(logger, "Upgrade authority did not sign");
                    return Err(InstructionError::MissingRequiredSignature);
                }
            } else {
                ic_logger_msg!(logger, "Invalid ProgramData account");
                return Err(InstructionError::InvalidAccountData);
            }

            // Load and verify the program bits

            let _ = create_and_cache_executor(
                program.unsigned_key(),
                &buffer.try_account_ref()?.data[buffer_data_offset..],
                invoke_context,
                use_jit,
            )?;

            // Update the ProgramData account, record the upgraded data, and zero
            // the rest

            programdata.set_state(&UpgradeableLoaderState::ProgramData {
                slot: clock.slot,
                upgrade_authority_address: Some(*authority.unsigned_key()),
            })?;
            programdata.try_account_ref_mut()?.data
                [programdata_data_offset..programdata_data_offset + buffer_data_len]
                .copy_from_slice(&buffer.try_account_ref()?.data[buffer_data_offset..]);
            for i in &mut programdata.try_account_ref_mut()?.data
                [programdata_data_offset + buffer_data_len..]
            {
                *i = 0
            }

            // Fund ProgramData to rent-exemption, spill the rest

            spill.try_account_ref_mut()?.lamports += (programdata.lamports()?
                + buffer.lamports()?)
            .saturating_sub(programdata_balance_required);
            buffer.try_account_ref_mut()?.lamports = 0;
            programdata.try_account_ref_mut()?.lamports = programdata_balance_required;

            ic_logger_msg!(logger, "Upgraded program {:?}", program.unsigned_key());
        }
        UpgradeableLoaderInstruction::SetAuthority => {
            let account = next_keyed_account(account_iter)?;
            let present_authority = next_keyed_account(account_iter)?;
            let new_authority = next_keyed_account(account_iter)
                .ok()
                .map(|account| account.unsigned_key());

            match account.state()? {
                UpgradeableLoaderState::Buffer { authority_address } => {
                    if invoke_context.is_feature_active(&matching_buffer_upgrade_authorities::id())
                        && new_authority == None
                    {
                        ic_logger_msg!(logger, "Buffer authority is not optional");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if authority_address == None {
                        ic_logger_msg!(logger, "Buffer is immutable");
                        return Err(InstructionError::Immutable);
                    }
                    if authority_address != Some(*present_authority.unsigned_key()) {
                        ic_logger_msg!(logger, "Incorrect buffer authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if present_authority.signer_key().is_none() {
                        ic_logger_msg!(logger, "Buffer authority did not sign");
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
                    if upgrade_authority_address == None {
                        ic_logger_msg!(logger, "Program not upgradeable");
                        return Err(InstructionError::Immutable);
                    }
                    if upgrade_authority_address != Some(*present_authority.unsigned_key()) {
                        ic_logger_msg!(logger, "Incorrect upgrade authority provided");
                        return Err(InstructionError::IncorrectAuthority);
                    }
                    if present_authority.signer_key().is_none() {
                        ic_logger_msg!(logger, "Upgrade authority did not sign");
                        return Err(InstructionError::MissingRequiredSignature);
                    }
                    account.set_state(&UpgradeableLoaderState::ProgramData {
                        slot,
                        upgrade_authority_address: new_authority.cloned(),
                    })?;
                }
                _ => {
                    ic_logger_msg!(logger, "Account does not support authorities");
                    return Err(InstructionError::InvalidAccountData);
                }
            }

            ic_logger_msg!(logger, "New authority {:?}", new_authority);
        }
    }

    Ok(())
}

fn process_loader_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
    use_jit: bool,
) -> Result<(), InstructionError> {
    let account_iter = &mut keyed_accounts.iter();

    let program = next_keyed_account(account_iter)?;
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
                &mut program.try_account_ref_mut()?.data,
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

            let _ = create_and_cache_executor(
                program.unsigned_key(),
                &program.try_account_ref()?.data,
                invoke_context,
                use_jit,
            )?;
            program.try_account_ref_mut()?.executable = true;
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
    pub compute_meter: Rc<RefCell<dyn ComputeMeter>>,
}
impl ThisInstructionMeter {
    fn new(compute_meter: Rc<RefCell<dyn ComputeMeter>>) -> Self {
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
pub struct BPFExecutor {
    program: Box<dyn Executable<BPFError, ThisInstructionMeter>>,
}

// Well, implement Debug for solana_rbpf::vm::Executable in solana-rbpf...
impl Debug for BPFExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BPFExecutor({:p})", self)
    }
}

impl Executor for BPFExecutor {
    fn execute(
        &self,
        loader_id: &Pubkey,
        program_id: &Pubkey,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
        use_jit: bool,
    ) -> Result<(), InstructionError> {
        let logger = invoke_context.get_logger();
        let invoke_depth = invoke_context.invoke_depth();

        let mut keyed_accounts_iter = keyed_accounts.iter();
        let _ = next_keyed_account(&mut keyed_accounts_iter)?;
        let parameter_accounts = keyed_accounts_iter.as_slice();
        let mut parameter_bytes =
            serialize_parameters(loader_id, program_id, parameter_accounts, &instruction_data)?;
        {
            let compute_meter = invoke_context.get_compute_meter();
            let mut vm = match create_vm(
                loader_id,
                self.program.as_ref(),
                &mut parameter_bytes,
                &parameter_accounts,
                invoke_context,
            ) {
                Ok(info) => info,
                Err(e) => {
                    ic_logger_msg!(logger, "Failed to create BPF VM: {}", e);
                    return Err(InstructionError::ProgramEnvironmentSetupFailure);
                }
            };

            stable_log::program_invoke(&logger, program_id, invoke_depth);
            let mut instruction_meter = ThisInstructionMeter::new(compute_meter.clone());
            let before = compute_meter.borrow().get_remaining();
            let result = if use_jit {
                vm.execute_program_jit(&mut instruction_meter)
            } else {
                vm.execute_program_interpreted(&mut instruction_meter)
            };
            let after = compute_meter.borrow().get_remaining();
            ic_logger_msg!(
                logger,
                "Program {} consumed {} of {} compute units",
                program_id,
                before - after,
                before
            );
            match result {
                Ok(status) => {
                    if status != SUCCESS {
                        let error: InstructionError = status.into();
                        stable_log::program_failure(&logger, program_id, &error);
                        return Err(error);
                    }
                }
                Err(error) => {
                    let error = match error {
                        EbpfError::UserError(BPFError::SyscallError(
                            SyscallError::InstructionError(error),
                        )) => error,
                        err => {
                            ic_logger_msg!(logger, "Program failed to complete: {}", err);
                            InstructionError::ProgramFailedToComplete
                        }
                    };
                    stable_log::program_failure(&logger, program_id, &error);
                    return Err(error);
                }
            }
        }
        deserialize_parameters(loader_id, parameter_accounts, &parameter_bytes)?;
        stable_log::program_success(&logger, program_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        message_processor::{Executors, ThisInvokeContext},
    };
    use solana_sdk::{
        account::{create_account, Account},
        account_utils::StateMut,
        client::SyncClient,
        clock::Clock,
        feature_set::FeatureSet,
        genesis_config::create_genesis_config,
        instruction::Instruction,
        instruction::{AccountMeta, InstructionError},
        message::Message,
        process_instruction::{BpfComputeBudget, MockInvokeContext},
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
        system_program, sysvar,
        transaction::TransactionError,
    };
    use std::{cell::RefCell, fs::File, io::Read, ops::Range, rc::Rc, sync::Arc};

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

        let program = Executable::<BPFError, TestInstructionMeter>::from_text_bytes(
            program,
            None,
            Config::default(),
        )
        .unwrap();
        let mut vm =
            EbpfVm::<BPFError, TestInstructionMeter>::new(program.as_ref(), input, &[]).unwrap();
        let mut instruction_meter = TestInstructionMeter { remaining: 10 };
        vm.execute_program_interpreted(&mut instruction_meter)
            .unwrap();
    }

    #[test]
    #[should_panic(expected = "VerifierError(LDDWCannotBeLast)")]
    fn test_bpf_loader_check_load_dw() {
        let prog = &[
            0x18, 0x00, 0x00, 0x00, 0x88, 0x77, 0x66, 0x55, // first half of lddw
        ];
        bpf_verifier::check(prog, true).unwrap();
    }

    #[test]
    fn test_bpf_loader_write() {
        let program_id = bpf_loader::id();
        let program_key = solana_sdk::pubkey::new_rand();
        let program_account = Account::new_ref(1, 0, &program_id);
        let keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];
        let instruction_data = bincode::serialize(&LoaderInstruction::Write {
            offset: 3,
            bytes: vec![1, 2, 3],
        })
        .unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(
                &bpf_loader::id(),
                &[],
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Write bytes to an offset
        #[allow(unused_mut)]
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &program_account)];
        keyed_accounts[0].account.borrow_mut().data = vec![0; 6];
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );
        assert_eq!(
            vec![0, 0, 0, 1, 2, 3],
            keyed_accounts[0].account.borrow().data
        );

        // Case: Overflow
        #[allow(unused_mut)]
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &program_account)];
        keyed_accounts[0].account.borrow_mut().data = vec![0; 5];
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_finalize() {
        let program_id = bpf_loader::id();
        let program_key = solana_sdk::pubkey::new_rand();
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        let rent = Rent::default();
        file.read_to_end(&mut elf).unwrap();
        let program_account = Account::new_ref(rent.minimum_balance(elf.len()), 0, &program_id);
        program_account.borrow_mut().data = elf;
        let keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];
        let instruction_data = bincode::serialize(&LoaderInstruction::Finalize).unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(
                &bpf_loader::id(),
                &[],
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Finalize
        let keyed_accounts = vec![KeyedAccount::new(&program_key, true, &program_account)];
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );
        assert!(keyed_accounts[0].account.borrow().executable);

        program_account.borrow_mut().executable = false; // Un-finalize the account

        // Case: Finalize
        program_account.borrow_mut().data[0] = 0; // bad elf
        let keyed_accounts = vec![KeyedAccount::new(&program_key, true, &program_account)];
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &instruction_data,
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_invoke_main() {
        let program_id = bpf_loader::id();
        let program_key = solana_sdk::pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let program_account = Account::new_ref(1, 0, &program_id);
        program_account.borrow_mut().data = elf;
        program_account.borrow_mut().executable = true;

        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];

        let mut invoke_context = MockInvokeContext::default();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &[], &[], &mut invoke_context)
        );

        // Case: Only a program account
        assert_eq!(
            Ok(()),
            process_instruction(&program_key, &keyed_accounts, &[], &mut invoke_context)
        );

        // Case: Account not a program
        keyed_accounts[0].account.borrow_mut().executable = false;
        assert_eq!(
            Err(InstructionError::InvalidInstructionData),
            process_instruction(&program_id, &keyed_accounts, &[], &mut invoke_context)
        );
        keyed_accounts[0].account.borrow_mut().executable = true;

        // Case: With program and parameter account
        let parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(&program_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(&program_key, &keyed_accounts, &[], &mut invoke_context)
        );

        // Case: limited budget
        let program_id = Pubkey::default();
        let mut invoke_context = ThisInvokeContext::new(
            &program_id,
            Rent::default(),
            vec![],
            &[],
            &[],
            None,
            BpfComputeBudget {
                max_units: 1,
                log_units: 100,
                log_64_units: 100,
                create_program_address_units: 1500,
                invoke_units: 1000,
                max_invoke_depth: 2,
                sha256_base_cost: 85,
                sha256_byte_cost: 1,
                max_call_depth: 20,
                stack_frame_size: 4096,
                log_pubkey_units: 100,
                max_cpi_instruction_size: usize::MAX,
            },
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(FeatureSet::default()),
        );
        assert_eq!(
            Err(InstructionError::ProgramFailedToComplete),
            process_instruction(&program_key, &keyed_accounts, &[], &mut invoke_context)
        );

        // Case: With duplicate accounts
        let duplicate_key = solana_sdk::pubkey::new_rand();
        let parameter_account = Account::new_ref(1, 0, &program_id);
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(
                &program_key,
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_serialize_unaligned() {
        let program_id = bpf_loader_deprecated::id();
        let program_key = solana_sdk::pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop_unaligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let program_account = Account::new_ref(1, 0, &program_id);
        program_account.borrow_mut().data = elf;
        program_account.borrow_mut().executable = true;
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];

        // Case: With program and parameter account
        let parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(&program_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(
                &program_key,
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );

        // Case: With duplicate accounts
        let duplicate_key = solana_sdk::pubkey::new_rand();
        let parameter_account = Account::new_ref(1, 0, &program_id);
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(
                &program_key,
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_serialize_aligned() {
        let program_id = bpf_loader::id();
        let program_key = solana_sdk::pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let program_account = Account::new_ref(1, 0, &program_id);
        program_account.borrow_mut().data = elf;
        program_account.borrow_mut().executable = true;
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];

        // Case: With program and parameter account
        let parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(&program_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(
                &program_key,
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );

        // Case: With duplicate accounts
        let duplicate_key = solana_sdk::pubkey::new_rand();
        let parameter_account = Account::new_ref(1, 0, &program_id);
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(
                &program_key,
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_initialize_buffer() {
        let instruction =
            bincode::serialize(&UpgradeableLoaderInstruction::InitializeBuffer).unwrap();
        let buffer_address = Pubkey::new_unique();
        let buffer_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        let authority_address = Pubkey::new_unique();
        let authority_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Case: Success
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&authority_address, false, &authority_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address)
            }
        );

        // Case: Already initialized
        assert_eq!(
            Err(InstructionError::AccountAlreadyInitialized),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&authority_address, false, &authority_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address)
            }
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_write() {
        let buffer_address = Pubkey::new_unique();
        let buffer_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Case: Not initialized
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Write entire buffer
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &buffer_account.borrow().data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
            &[42; 9]
        );

        // Case: Write portion of the buffer
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 3,
            bytes: vec![42; 6],
        })
        .unwrap();
        let buffer_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(9).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address)
            }
        );
        assert_eq!(
            &buffer_account.borrow().data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
            &[0, 0, 0, 42, 42, 42, 42, 42, 42]
        );

        // Case: Not signed
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: overflow size
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 0,
            bytes: vec![42; 10],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: overflow offset
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&buffer_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: wrong authority
        let authority_address = Pubkey::new_unique();
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&authority_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: None authority
        let authority_address = Pubkey::new_unique();
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Write {
            offset: 1,
            bytes: vec![42; 9],
        })
        .unwrap();
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::Immutable),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&authority_address, true, &buffer_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_deploy_with_max_len() {
        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000_000);
        let mut bank = Bank::new(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.add_builtin(
            "solana_bpf_loader_upgradeable_program",
            bpf_loader_upgradeable::id(),
            process_instruction,
        );
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        // Setup initial accounts
        let program_keypair = Keypair::new();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_keypair.pubkey().as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let upgrade_authority_keypair = Keypair::new();
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let min_program_balance = bank
            .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::program_len().unwrap());
        let min_programdata_balance = bank.get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(elf.len()).unwrap(),
        );
        let buffer_address = Pubkey::new_unique();
        let mut buffer_account = Account::new(
            min_programdata_balance,
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..]
            .copy_from_slice(&elf);
        let program_account = Account::new(
            min_programdata_balance,
            UpgradeableLoaderState::program_len().unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        let programdata_account = Account::new(
            1,
            UpgradeableLoaderState::programdata_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Test successful deploy
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
        let before = bank.get_balance(&mint_keypair.pubkey());
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
        assert!(bank_client
            .send_and_confirm_message(
                &[&mint_keypair, &program_keypair, &upgrade_authority_keypair],
                message
            )
            .is_ok());
        assert_eq!(
            bank.get_balance(&mint_keypair.pubkey()),
            before - min_program_balance
        );
        assert_eq!(bank.get_balance(&buffer_address), 0);
        assert_eq!(None, bank.get_account(&buffer_address));
        let post_program_account = bank.get_account(&program_keypair.pubkey()).unwrap();
        assert_eq!(post_program_account.lamports, min_program_balance);
        assert_eq!(post_program_account.owner, bpf_loader_upgradeable::id());
        assert_eq!(
            post_program_account.data.len(),
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
        assert_eq!(post_programdata_account.lamports, min_programdata_balance);
        assert_eq!(post_programdata_account.owner, bpf_loader_upgradeable::id());
        let state: UpgradeableLoaderState = post_programdata_account.state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot: bank_client.get_slot().unwrap(),
                upgrade_authority_address: Some(upgrade_authority_keypair.pubkey())
            }
        );
        for (i, byte) in post_programdata_account.data
            [UpgradeableLoaderState::programdata_data_offset().unwrap()..]
            .iter()
            .enumerate()
        {
            assert_eq!(elf[i], *byte);
        }

        // Test initialized program account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        let message = Message::new(
            &[Instruction::new(
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
        bank.store_account(&program_keypair.pubkey(), &Account::default());
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
            &[Instruction::new(
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
            &[Instruction::new(
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
        bank.store_account(&buffer_address, &Account::default());
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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

        // Test Insufficient payer funds
        bank.clear_signatures();
        bank.store_account(
            &mint_keypair.pubkey(),
            &Account::new(min_program_balance, 0, &system_program::id()),
        );
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
            &Account::new(1_000_000_000, 0, &system_program::id()),
        );

        // Test max_data_len
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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

        // Test not the system account
        bank.clear_signatures();
        bank.store_account(&buffer_address, &buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        modified_buffer_account
            .data
            .truncate(UpgradeableLoaderState::buffer_len(1).unwrap());
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        let mut modified_buffer_account = Account::new(
            min_programdata_balance,
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_keypair.pubkey()),
            })
            .unwrap();
        modified_buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..]
            .copy_from_slice(&elf);
        modified_buffer_account.data.truncate(5);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        let mut modified_buffer_account = Account::new(
            min_programdata_balance,
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(buffer_address),
            })
            .unwrap();
        modified_buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..]
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        let mut modified_buffer_account = Account::new(
            min_programdata_balance,
            UpgradeableLoaderState::buffer_len(elf.len()).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        modified_buffer_account
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        modified_buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..]
            .copy_from_slice(&elf);
        bank.store_account(&buffer_address, &modified_buffer_account);
        bank.store_account(&program_keypair.pubkey(), &Account::default());
        bank.store_account(&programdata_address, &Account::default());
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
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::Upgrade).unwrap();
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf_orig = Vec::new();
        file.read_to_end(&mut elf_orig).unwrap();
        let mut file = File::open("test_elfs/noop_unaligned.so").expect("file open failed");
        let mut elf_new = Vec::new();
        file.read_to_end(&mut elf_new).unwrap();
        assert_ne!(elf_orig.len(), elf_new.len());
        let rent = Rent::default();
        let rent_account = RefCell::new(create_account(&Rent::default(), 1));
        let slot = 42;
        let clock_account = RefCell::new(create_account(
            &Clock {
                slot,
                ..Clock::default()
            },
            1,
        ));
        let min_program_balance =
            1.max(rent.minimum_balance(UpgradeableLoaderState::program_len().unwrap()));
        let min_programdata_balance = 1.max(rent.minimum_balance(
            UpgradeableLoaderState::programdata_len(elf_orig.len().max(elf_new.len())).unwrap(),
        ));
        let upgrade_authority_address = Pubkey::new_unique();
        let buffer_address = Pubkey::new_unique();
        let program_address = Pubkey::new_unique();
        let (programdata_address, _) =
            Pubkey::find_program_address(&[program_address.as_ref()], &id());
        let spill_address = Pubkey::new_unique();
        let upgrade_authority_account = Account::new_ref(1, 0, &Pubkey::new_unique());

        #[allow(clippy::type_complexity)]
        fn get_accounts(
            buffer_authority: &Pubkey,
            programdata_address: &Pubkey,
            upgrade_authority_address: &Pubkey,
            slot: u64,
            elf_orig: &[u8],
            elf_new: &[u8],
            min_program_balance: u64,
            min_programdata_balance: u64,
        ) -> (
            Rc<RefCell<Account>>,
            Rc<RefCell<Account>>,
            Rc<RefCell<Account>>,
            Rc<RefCell<Account>>,
        ) {
            let buffer_account = Account::new_ref(
                1,
                UpgradeableLoaderState::buffer_len(elf_new.len()).unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            buffer_account
                .borrow_mut()
                .set_state(&UpgradeableLoaderState::Buffer {
                    authority_address: Some(*buffer_authority),
                })
                .unwrap();
            buffer_account.borrow_mut().data
                [UpgradeableLoaderState::buffer_data_offset().unwrap()..]
                .copy_from_slice(&elf_new);
            let programdata_account = Account::new_ref(
                min_programdata_balance,
                UpgradeableLoaderState::programdata_len(elf_orig.len().max(elf_new.len())).unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            programdata_account
                .borrow_mut()
                .set_state(&UpgradeableLoaderState::ProgramData {
                    slot,
                    upgrade_authority_address: Some(*upgrade_authority_address),
                })
                .unwrap();
            let program_account = Account::new_ref(
                min_program_balance,
                UpgradeableLoaderState::program_len().unwrap(),
                &bpf_loader_upgradeable::id(),
            );
            program_account.borrow_mut().executable = true;
            program_account
                .borrow_mut()
                .set_state(&UpgradeableLoaderState::Program {
                    programdata_address: *programdata_address,
                })
                .unwrap();
            let spill_account = Account::new_ref(0, 0, &Pubkey::new_unique());

            (
                buffer_account,
                program_account,
                programdata_account,
                spill_account,
            )
        }

        // Case: Success
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        assert_eq!(0, buffer_account.borrow().lamports);
        assert_eq!(
            min_programdata_balance,
            programdata_account.borrow().lamports
        );
        assert_eq!(1, spill_account.borrow().lamports);
        let state: UpgradeableLoaderState = programdata_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address)
            }
        );
        for (i, byte) in programdata_account.borrow().data
            [UpgradeableLoaderState::programdata_data_offset().unwrap()
                ..UpgradeableLoaderState::programdata_data_offset().unwrap() + elf_new.len()]
            .iter()
            .enumerate()
        {
            assert_eq!(elf_new[i], *byte);
        }

        // Case: not upgradable
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::Immutable),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: wrong authority
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &Pubkey::new_unique(),
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: authority did not sign
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        false,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Program account not executable
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        program_account.borrow_mut().executable = false;
        assert_eq!(
            Err(InstructionError::AccountNotExecutable),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Program account now owned by loader
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        program_account.borrow_mut().owner = Pubkey::new_unique();
        assert_eq!(
            Err(InstructionError::IncorrectProgramId),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Program account not writable
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Err(InstructionError::InvalidArgument),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Program account not initialized
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        program_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Program ProgramData account mismatch
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Err(InstructionError::InvalidArgument),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&Pubkey::new_unique(), false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Buffer account not initialized
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Uninitialized)
            .unwrap();
        assert_eq!(
            Err(InstructionError::InvalidArgument),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Buffer account too big
        let (_, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        let buffer_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(elf_orig.len().max(elf_new.len()) + 1).unwrap(),
            &bpf_loader_upgradeable::id(),
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Test small buffer account
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &upgrade_authority_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        buffer_account.borrow_mut().data.truncate(5);
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: Mismatched buffer and program authority
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &buffer_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: None buffer authority
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &buffer_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: None buffer and program authority
        let (buffer_account, program_account, programdata_account, spill_account) = get_accounts(
            &buffer_address,
            &programdata_address,
            &upgrade_authority_address,
            slot,
            &elf_orig,
            &elf_new,
            min_program_balance,
            min_programdata_balance,
        );
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new(&program_address, false, &program_account),
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new(&spill_address, false, &spill_account),
                    KeyedAccount::new_readonly(&sysvar::rent::id(), false, &rent_account),
                    KeyedAccount::new_readonly(&sysvar::clock::id(), false, &clock_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_set_upgrade_authority() {
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap();
        let slot = 0;
        let upgrade_authority_address = Pubkey::new_unique();
        let upgrade_authority_account = Account::new_ref(1, 0, &Pubkey::new_unique());
        let new_upgrade_authority_address = Pubkey::new_unique();
        let new_upgrade_authority_account = Account::new_ref(1, 0, &Pubkey::new_unique());
        let program_address = Pubkey::new_unique();
        let (programdata_address, _) =
            Pubkey::find_program_address(&[program_address.as_ref()], &id());
        let programdata_account = Account::new_ref(
            1,
            UpgradeableLoaderState::programdata_len(0).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Case: Set to new authority
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    ),
                    KeyedAccount::new_readonly(
                        &new_upgrade_authority_address,
                        false,
                        &new_upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = programdata_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(new_upgrade_authority_address),
            }
        );

        // Case: Not upgradeable
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        true,
                        &upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = programdata_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            }
        );

        // Case: Authority did not sign
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &upgrade_authority_address,
                        false,
                        &upgrade_authority_account
                    ),
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: wrong authority
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: Some(upgrade_authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &Pubkey::new_unique(),
                        true,
                        &upgrade_authority_account
                    ),
                    KeyedAccount::new_readonly(
                        &new_upgrade_authority_address,
                        false,
                        &new_upgrade_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: No authority
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::ProgramData {
                slot,
                upgrade_authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::Immutable),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &Pubkey::new_unique(),
                        true,
                        &upgrade_authority_account
                    ),
                ],
                &bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap(),
                &mut MockInvokeContext::default()
            )
        );

        // Case: Not a ProgramData account
        programdata_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&programdata_address, false, &programdata_account),
                    KeyedAccount::new_readonly(
                        &Pubkey::new_unique(),
                        true,
                        &upgrade_authority_account
                    ),
                ],
                &bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap(),
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_upgradeable_set_buffer_authority() {
        let instruction = bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap();
        let authority_address = Pubkey::new_unique();
        let authority_account = Account::new_ref(1, 0, &Pubkey::new_unique());
        let new_authority_address = Pubkey::new_unique();
        let new_authority_account = Account::new_ref(1, 0, &Pubkey::new_unique());
        let buffer_address = Pubkey::new_unique();
        let buffer_account = Account::new_ref(
            1,
            UpgradeableLoaderState::buffer_len(0).unwrap(),
            &bpf_loader_upgradeable::id(),
        );

        // Case: Set to new authority
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&authority_address, true, &authority_account),
                    KeyedAccount::new_readonly(
                        &new_authority_address,
                        false,
                        &new_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(new_authority_address),
            }
        );

        // Case: New authority required
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&authority_address, true, &authority_account)
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );
        let state: UpgradeableLoaderState = buffer_account.borrow().state().unwrap();
        assert_eq!(
            state,
            UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            }
        );

        // Case: Authority did not sign
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&authority_address, false, &authority_account),
                    KeyedAccount::new_readonly(
                        &new_authority_address,
                        false,
                        &new_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: wrong authority
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&Pubkey::new_unique(), true, &authority_account),
                    KeyedAccount::new_readonly(
                        &new_authority_address,
                        false,
                        &new_authority_account
                    )
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
        );

        // Case: No authority
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: None,
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::Immutable),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&Pubkey::new_unique(), true, &authority_account),
                    KeyedAccount::new_readonly(
                        &new_authority_address,
                        false,
                        &new_authority_account
                    )
                ],
                &bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap(),
                &mut MockInvokeContext::default()
            )
        );

        // Case: Not a Buffer account
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Program {
                programdata_address: Pubkey::new_unique(),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&Pubkey::new_unique(), true, &authority_account),
                ],
                &bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap(),
                &mut MockInvokeContext::default()
            )
        );

        // Case: Set to no authority
        buffer_account
            .borrow_mut()
            .set_state(&UpgradeableLoaderState::Buffer {
                authority_address: Some(authority_address),
            })
            .unwrap();
        assert_eq!(
            Err(InstructionError::IncorrectAuthority),
            process_instruction(
                &bpf_loader_upgradeable::id(),
                &[
                    KeyedAccount::new(&buffer_address, false, &buffer_account),
                    KeyedAccount::new_readonly(&authority_address, true, &authority_account),
                ],
                &instruction,
                &mut MockInvokeContext::default()
            )
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
        let program_id = solana_sdk::pubkey::new_rand();
        let program_key = solana_sdk::pubkey::new_rand();

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
                let program_account = Account::new_ref(1, 0, &program_id);
                program_account.borrow_mut().data = bytes.to_vec();
                program_account.borrow_mut().executable = true;

                let parameter_account = Account::new_ref(1, 0, &program_id);
                let keyed_accounts = vec![
                    KeyedAccount::new(&program_key, false, &program_account),
                    KeyedAccount::new(&program_key, false, &parameter_account),
                ];

                let _result = process_instruction(
                    &bpf_loader::id(),
                    &keyed_accounts,
                    &[],
                    &mut MockInvokeContext::default(),
                );
            },
        );
    }
}
