pub mod alloc;
pub mod allocator_bump;
pub mod bpf_verifier;
pub mod deprecated;
pub mod serialization;
pub mod syscalls;

use crate::{
    bpf_verifier::VerifierError,
    serialization::{deserialize_parameters, serialize_parameters},
    syscalls::SyscallError,
};
use num_derive::{FromPrimitive, ToPrimitive};
use solana_rbpf::{
    ebpf::MM_HEAP_START,
    error::{EbpfError, UserDefinedError},
    memory_region::MemoryRegion,
    vm::{Config, EbpfVm, Executable, InstructionMeter},
};
use solana_sdk::{
    bpf_loader, bpf_loader_deprecated,
    decode_error::DecodeError,
    entrypoint::SUCCESS,
    feature_set::bpf_compute_budget_balancing,
    instruction::InstructionError,
    keyed_account::{is_executable, next_keyed_account, KeyedAccount},
    loader_instruction::LoaderInstruction,
    process_instruction::{stable_log, ComputeMeter, Executor, InvokeContext},
    program_utils::limited_deserialize,
    pubkey::Pubkey,
};
use std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};
use thiserror::Error;

solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    solana_bpf_loader_program::process_instruction
);

/// Errors returned by the BPFLoader if the VM fails to run the program
#[derive(Error, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum BPFLoaderError {
    #[error("failed to create virtual machine")]
    VirtualMachineCreationFailed = 0x0b9f_0001,
    #[error("virtual machine failed to run the program to completion")]
    VirtualMachineFailedToRunProgram = 0x0b9f_0002,
}
impl<E> DecodeError<E> for BPFLoaderError {
    fn type_of() -> &'static str {
        "BPFLoaderError"
    }
}

/// Errors returned by functions the BPF Loader registers with the VM
#[derive(Debug, Error, PartialEq)]
pub enum BPFError {
    #[error("{0}")]
    VerifierError(#[from] VerifierError),
    #[error("{0}")]
    SyscallError(#[from] SyscallError),
}
impl UserDefinedError for BPFError {}

/// Point all log messages to the log collector
macro_rules! log {
    ($logger:ident, $message:expr) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log($message);
            }
        }
    };
    ($logger:ident, $fmt:expr, $($arg:tt)*) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log(&format!($fmt, $($arg)*));
            }
        }
    };
}

fn map_ebpf_error(
    invoke_context: &mut dyn InvokeContext,
    e: EbpfError<BPFError>,
) -> InstructionError {
    let logger = invoke_context.get_logger();
    log!(logger, "{}", e);
    InstructionError::InvalidAccountData
}

pub fn create_and_cache_executor(
    program: &KeyedAccount,
    invoke_context: &mut dyn InvokeContext,
) -> Result<Arc<BPFExecutor>, InstructionError> {
    let executable = Executable::from_elf(&program.try_account_ref()?.data, None)
        .map_err(|e| map_ebpf_error(invoke_context, e))?;
    let (_, elf_bytes) = executable
        .get_text_bytes()
        .map_err(|e| map_ebpf_error(invoke_context, e))?;
    bpf_verifier::check(
        elf_bytes,
        !invoke_context.is_feature_active(&bpf_compute_budget_balancing::id()),
    )
    .map_err(|e| map_ebpf_error(invoke_context, EbpfError::UserError(e)))?;
    let executor = Arc::new(BPFExecutor { executable });
    invoke_context.add_executor(program.unsigned_key(), executor.clone());
    Ok(executor)
}

/// Default program heap size, allocators
/// are expected to enforce this
const DEFAULT_HEAP_SIZE: usize = 32 * 1024;

/// Create the BPF virtual machine
pub fn create_vm<'a>(
    loader_id: &'a Pubkey,
    executable: &'a dyn Executable<BPFError>,
    parameter_bytes: &[u8],
    parameter_accounts: &'a [KeyedAccount<'a>],
    invoke_context: &'a mut dyn InvokeContext,
) -> Result<EbpfVm<'a, BPFError, ThisInstructionMeter>, EbpfError<BPFError>> {
    let heap = vec![0_u8; DEFAULT_HEAP_SIZE];
    let heap_region = MemoryRegion::new_from_slice(&heap, MM_HEAP_START, true);
    let bpf_compute_budget = invoke_context.get_bpf_compute_budget();
    let mut vm = EbpfVm::new(
        executable,
        Config {
            max_call_depth: bpf_compute_budget.max_call_depth,
            stack_frame_size: bpf_compute_budget.stack_frame_size,
        },
        parameter_bytes,
        &[heap_region],
    )?;
    syscalls::register_syscalls(loader_id, &mut vm, parameter_accounts, invoke_context, heap)?;
    Ok(vm)
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    debug_assert!(bpf_loader::check_id(program_id) || bpf_loader_deprecated::check_id(program_id));

    let logger = invoke_context.get_logger();

    if keyed_accounts.is_empty() {
        log!(logger, "No account keys");
        return Err(InstructionError::NotEnoughAccountKeys);
    }
    let program = &keyed_accounts[0];

    if is_executable(keyed_accounts)? {
        let executor = match invoke_context.get_executor(program.unsigned_key()) {
            Some(executor) => executor,
            None => create_and_cache_executor(program, invoke_context)?,
        };
        executor.execute(program_id, keyed_accounts, instruction_data, invoke_context)?
    } else if !keyed_accounts.is_empty() {
        match limited_deserialize(instruction_data)? {
            LoaderInstruction::Write { offset, bytes } => {
                if program.signer_key().is_none() {
                    log!(logger, "key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }
                let offset = offset as usize;
                let len = bytes.len();
                if program.data_len()? < offset + len {
                    log!(
                        logger,
                        "Write overflow: {} < {}",
                        program.data_len()?,
                        offset + len
                    );
                    return Err(InstructionError::AccountDataTooSmall);
                }
                program.try_account_ref_mut()?.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                if program.signer_key().is_none() {
                    log!(logger, "key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }

                let _ = create_and_cache_executor(program, invoke_context)?;
                program.try_account_ref_mut()?.executable = true;
                log!(
                    logger,
                    "Finalized account {:?}",
                    program.signer_key().unwrap()
                );
            }
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
    executable: Box<dyn Executable<BPFError>>,
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
        program_id: &Pubkey,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        let logger = invoke_context.get_logger();
        let invoke_depth = invoke_context.invoke_depth();

        let mut keyed_accounts_iter = keyed_accounts.iter();
        let program = next_keyed_account(&mut keyed_accounts_iter)?;

        let parameter_accounts = keyed_accounts_iter.as_slice();
        let parameter_bytes = serialize_parameters(
            program_id,
            program.unsigned_key(),
            parameter_accounts,
            &instruction_data,
        )?;
        {
            let compute_meter = invoke_context.get_compute_meter();
            let mut vm = match create_vm(
                program_id,
                self.executable.as_ref(),
                parameter_bytes.as_slice(),
                &parameter_accounts,
                invoke_context,
            ) {
                Ok(info) => info,
                Err(e) => {
                    log!(logger, "Failed to create BPF VM: {}", e);
                    return Err(BPFLoaderError::VirtualMachineCreationFailed.into());
                }
            };

            stable_log::program_invoke(&logger, program.unsigned_key(), invoke_depth);
            let mut instruction_meter = ThisInstructionMeter::new(compute_meter.clone());
            let before = compute_meter.borrow().get_remaining();
            const IS_JIT_ENABLED: bool = false;
            let result = if IS_JIT_ENABLED {
                if vm.jit_compile().is_err() {
                    return Err(BPFLoaderError::VirtualMachineCreationFailed.into());
                }
                unsafe { vm.execute_program_jit(&mut instruction_meter) }
            } else {
                vm.execute_program_interpreted(&mut instruction_meter)
            };
            let after = compute_meter.borrow().get_remaining();
            log!(
                logger,
                "Program {} consumed {} of {} compute units",
                program.unsigned_key(),
                before - after,
                before
            );
            match result {
                Ok(status) => {
                    if status != SUCCESS {
                        let error: InstructionError = status.into();
                        stable_log::program_failure(&logger, program.unsigned_key(), &error);
                        return Err(error);
                    }
                }
                Err(error) => {
                    log!(
                        logger,
                        "Program {} BPF VM error: {}",
                        program.unsigned_key(),
                        error
                    );
                    let error = match error {
                        EbpfError::UserError(BPFError::SyscallError(
                            SyscallError::InstructionError(error),
                        )) => error,
                        _ => BPFLoaderError::VirtualMachineFailedToRunProgram.into(),
                    };

                    stable_log::program_failure(&logger, program.unsigned_key(), &error);
                    return Err(error);
                }
            }
        }
        deserialize_parameters(program_id, parameter_accounts, &parameter_bytes)?;
        stable_log::program_success(&logger, program.unsigned_key());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use solana_runtime::message_processor::{Executors, ThisInvokeContext};
    use solana_sdk::{
        account::Account,
        feature_set::FeatureSet,
        instruction::InstructionError,
        process_instruction::{BpfComputeBudget, MockInvokeContext},
        pubkey::Pubkey,
        rent::Rent,
    };
    use std::{cell::RefCell, fs::File, io::Read, ops::Range, rc::Rc};

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

        let executable = Executable::<BPFError>::from_text_bytes(program, None).unwrap();
        let mut vm = EbpfVm::<BPFError, TestInstructionMeter>::new(
            executable.as_ref(),
            Config::default(),
            input,
            &[],
        )
        .unwrap();
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
        let program_id = solana_sdk::pubkey::new_rand();
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
        let program_id = solana_sdk::pubkey::new_rand();
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
        let program_id = solana_sdk::pubkey::new_rand();
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
            process_instruction(&bpf_loader::id(), &[], &[], &mut invoke_context)
        );

        // Case: Only a program account
        assert_eq!(
            Ok(()),
            process_instruction(&bpf_loader::id(), &keyed_accounts, &[], &mut invoke_context)
        );

        // Case: Account not executable
        keyed_accounts[0].account.borrow_mut().executable = false;
        assert_eq!(
            Err(InstructionError::InvalidInstructionData),
            process_instruction(&bpf_loader::id(), &keyed_accounts, &[], &mut invoke_context)
        );
        keyed_accounts[0].account.borrow_mut().executable = true;

        // Case: With program and parameter account
        let parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(&program_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(&bpf_loader::id(), &keyed_accounts, &[], &mut invoke_context)
        );

        // Case: limited budget
        let program_id = Pubkey::default();
        let mut invoke_context = ThisInvokeContext::new(
            &program_id,
            Rent::default(),
            vec![],
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
            },
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(FeatureSet::default()),
        );
        assert_eq!(
            Err(InstructionError::Custom(194969602)),
            process_instruction(&bpf_loader::id(), &keyed_accounts, &[], &mut invoke_context)
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
                &bpf_loader::id(),
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_serialize_unaligned() {
        let program_id = solana_sdk::pubkey::new_rand();
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
                &bpf_loader_deprecated::id(),
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
                &bpf_loader_deprecated::id(),
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
    }

    #[test]
    fn test_bpf_loader_serialize_aligned() {
        let program_id = solana_sdk::pubkey::new_rand();
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
                &bpf_loader::id(),
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
                &bpf_loader::id(),
                &keyed_accounts,
                &[],
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
