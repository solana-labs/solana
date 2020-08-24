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
    ebpf::{EbpfError, UserDefinedError},
    memory_region::MemoryRegion,
    EbpfVm, InstructionMeter,
};
use solana_sdk::{
    account::{is_executable, next_keyed_account, KeyedAccount},
    bpf_loader, bpf_loader_deprecated,
    decode_error::DecodeError,
    entrypoint::SUCCESS,
    entrypoint_native::{ComputeMeter, InvokeContext},
    instruction::InstructionError,
    loader_instruction::LoaderInstruction,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
};
use std::{cell::RefCell, rc::Rc};
use thiserror::Error;

solana_sdk::declare_builtin!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    solana_bpf_loader_program::process_instruction
);

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

/// Errors returned by functions the BPF Loader registers with the vM
#[derive(Debug, Error)]
pub enum BPFError {
    #[error("{0}")]
    VerifierError(#[from] VerifierError),
    #[error("{0}")]
    SyscallError(#[from] SyscallError),
}
impl UserDefinedError for BPFError {}

pub fn create_vm<'a>(
    loader_id: &Pubkey,
    prog: &'a [u8],
    parameter_accounts: &'a [KeyedAccount<'a>],
    invoke_context: &'a mut dyn InvokeContext,
) -> Result<(EbpfVm<'a, BPFError>, MemoryRegion), EbpfError<BPFError>> {
    let mut vm = EbpfVm::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_elf(&prog)?;

    let heap_region =
        syscalls::register_syscalls(loader_id, &mut vm, parameter_accounts, invoke_context)?;

    Ok((vm, heap_region))
}

pub fn check_elf(prog: &[u8]) -> Result<(), EbpfError<BPFError>> {
    let mut vm = EbpfVm::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_elf(&prog)?;
    Ok(())
}

/// Look for a duplicate account and return its position if found
pub fn is_dup(accounts: &[KeyedAccount], keyed_account: &KeyedAccount) -> (bool, usize) {
    for (i, account) in accounts.iter().enumerate() {
        if account == keyed_account {
            return (true, i);
        }
    }
    (false, 0)
}

macro_rules! log{
    ($logger:ident, $message:expr) => {
        if let Ok(mut logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log($message);
            }
        }
    };
    ($logger:ident, $fmt:expr, $($arg:tt)*) => {
        if let Ok(mut logger) = $logger.try_borrow_mut() {
            logger.log(&format!($fmt, $($arg)*));
        }
    };
}

struct ThisInstructionMeter {
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
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

    if is_executable(keyed_accounts)? {
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
            let program_account = program.try_account_ref_mut()?;
            let (mut vm, heap_region) = match create_vm(
                program_id,
                &program_account.data,
                &parameter_accounts,
                invoke_context,
            ) {
                Ok(info) => info,
                Err(e) => {
                    log!(logger, "Failed to create BPF VM: {}", e);
                    return Err(BPFLoaderError::VirtualMachineCreationFailed.into());
                }
            };

            log!(logger, "Call BPF program {}", program.unsigned_key());
            let instruction_meter = ThisInstructionMeter { compute_meter };
            match vm.execute_program_metered(
                parameter_bytes.as_slice(),
                &[],
                &[heap_region],
                instruction_meter,
            ) {
                Ok(status) => {
                    if status != SUCCESS {
                        let error: InstructionError = status.into();
                        log!(
                            logger,
                            "BPF program {} failed: {}",
                            program.unsigned_key(),
                            error
                        );
                        return Err(error);
                    }
                }
                Err(error) => {
                    log!(
                        logger,
                        "BPF program {} failed: {}",
                        program.unsigned_key(),
                        error
                    );
                    return match error {
                        EbpfError::UserError(BPFError::SyscallError(
                            SyscallError::InstructionError(error),
                        )) => Err(error),
                        _ => Err(BPFLoaderError::VirtualMachineFailedToRunProgram.into()),
                    };
                }
            }
        }
        deserialize_parameters(program_id, parameter_accounts, &parameter_bytes)?;
        log!(logger, "BPF program {} success", program.unsigned_key());
    } else if !keyed_accounts.is_empty() {
        match limited_deserialize(instruction_data)? {
            LoaderInstruction::Write { offset, bytes } => {
                let mut keyed_accounts_iter = keyed_accounts.iter();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;
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
                let mut keyed_accounts_iter = keyed_accounts.iter();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;

                if program.signer_key().is_none() {
                    log!(logger, "key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }

                if let Err(e) = check_elf(&program.try_account_ref()?.data) {
                    log!(logger, "{}", e);
                    return Err(InstructionError::InvalidAccountData);
                }

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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use solana_sdk::{
        account::Account,
        entrypoint_native::{ComputeMeter, Logger, ProcessInstruction},
        instruction::CompiledInstruction,
        message::Message,
        rent::Rent,
    };
    use std::{cell::RefCell, fs::File, io::Read, ops::Range, rc::Rc};

    #[derive(Debug, Default, Clone)]
    pub struct MockComputeMeter {
        pub remaining: u64,
    }
    impl ComputeMeter for MockComputeMeter {
        fn consume(&mut self, amount: u64) -> Result<(), InstructionError> {
            self.remaining = self.remaining.saturating_sub(amount);
            if self.remaining == 0 {
                return Err(InstructionError::ComputationalBudgetExceeded);
            }
            Ok(())
        }
        fn get_remaining(&self) -> u64 {
            self.remaining
        }
    }
    #[derive(Debug, Default, Clone)]
    pub struct MockLogger {
        pub log: Rc<RefCell<Vec<String>>>,
    }
    impl Logger for MockLogger {
        fn log_enabled(&self) -> bool {
            true
        }
        fn log(&mut self, message: &str) {
            self.log.borrow_mut().push(message.to_string());
        }
    }
    #[derive(Debug)]
    pub struct MockInvokeContext {
        pub key: Pubkey,
        pub logger: MockLogger,
        pub compute_meter: MockComputeMeter,
    }
    impl Default for MockInvokeContext {
        fn default() -> Self {
            MockInvokeContext {
                key: Pubkey::default(),
                logger: MockLogger::default(),
                compute_meter: MockComputeMeter {
                    remaining: std::u64::MAX,
                },
            }
        }
    }
    impl InvokeContext for MockInvokeContext {
        fn push(&mut self, _key: &Pubkey) -> Result<(), InstructionError> {
            Ok(())
        }
        fn pop(&mut self) {}
        fn verify_and_update(
            &mut self,
            _message: &Message,
            _instruction: &CompiledInstruction,
            _accounts: &[Rc<RefCell<Account>>],
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
            Ok(&self.key)
        }
        fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)] {
            &[]
        }
        fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
            Rc::new(RefCell::new(self.logger.clone()))
        }
        fn is_cross_program_supported(&self) -> bool {
            true
        }
        fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
            Rc::new(RefCell::new(self.compute_meter.clone()))
        }
    }

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
    #[should_panic(expected = "ExceededMaxInstructions(10)")]
    fn test_bpf_loader_non_terminating_program() {
        #[rustfmt::skip]
        let program = &[
            0x07, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // r6 + 1
            0x05, 0x00, 0xfe, 0xff, 0x00, 0x00, 0x00, 0x00, // goto -2
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // exit
        ];
        let input = &mut [0x00];

        let mut vm = EbpfVm::<BPFError>::new(None).unwrap();
        vm.set_verifier(bpf_verifier::check).unwrap();
        let instruction_meter = TestInstructionMeter { remaining: 10 };
        vm.set_program(program).unwrap();
        vm.execute_program_metered(input, &[], &[], instruction_meter)
            .unwrap();
    }

    #[test]
    fn test_bpf_loader_write() {
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();
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
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();
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
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop_aligned.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let program_account = Account::new_ref(1, 0, &program_id);
        program_account.borrow_mut().data = elf;
        program_account.borrow_mut().executable = true;

        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &program_account)];

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(
                &bpf_loader::id(),
                &[],
                &[],
                &mut MockInvokeContext::default()
            )
        );

        // Case: Only a program account
        assert_eq!(
            Ok(()),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );

        // Case: Account not executable
        keyed_accounts[0].account.borrow_mut().executable = false;
        assert_eq!(
            Err(InstructionError::InvalidInstructionData),
            process_instruction(
                &bpf_loader::id(),
                &keyed_accounts,
                &[],
                &mut MockInvokeContext::default()
            )
        );
        keyed_accounts[0].account.borrow_mut().executable = true;

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
        let duplicate_key = Pubkey::new_rand();
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
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

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
        let duplicate_key = Pubkey::new_rand();
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
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

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
        let duplicate_key = Pubkey::new_rand();
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
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

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
