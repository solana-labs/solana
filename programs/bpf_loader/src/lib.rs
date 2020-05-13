pub mod alloc;
pub mod allocator_bump;
pub mod bpf_verifier;
pub mod syscalls;

use crate::{bpf_verifier::VerifierError, syscalls::SyscallError};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use solana_rbpf::{
    ebpf::{EbpfError, UserDefinedError},
    memory_region::MemoryRegion,
    EbpfVm,
};
use solana_sdk::{
    account::KeyedAccount,
    bpf_loader,
    entrypoint::SUCCESS,
    entrypoint_native::InvokeContext,
    instruction::InstructionError,
    loader_instruction::LoaderInstruction,
    program_utils::DecodeError,
    program_utils::{is_executable, limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
};
use std::{io::prelude::*, mem};
use thiserror::Error;

solana_sdk::declare_loader!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    process_instruction
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
    prog: &'a [u8],
    invoke_context: &'a mut dyn InvokeContext,
) -> Result<(EbpfVm<'a, BPFError>, MemoryRegion), EbpfError<BPFError>> {
    let mut vm = EbpfVm::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_max_instruction_count(100_000)?;
    vm.set_elf(&prog)?;

    let heap_region = syscalls::register_syscalls(&mut vm, invoke_context)?;

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

pub fn serialize_parameters(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<Vec<u8>, InstructionError> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, position) = is_dup(&keyed_accounts[..i], keyed_account);
        if is_dup {
            v.write_u8(position as u8).unwrap();
        } else {
            v.write_u8(std::u8::MAX).unwrap();
            v.write_u8(keyed_account.signer_key().is_some() as u8)
                .unwrap();
            v.write_u8(keyed_account.is_writable() as u8).unwrap();
            v.write_all(keyed_account.unsigned_key().as_ref()).unwrap();
            v.write_u64::<LittleEndian>(keyed_account.lamports()?)
                .unwrap();
            v.write_u64::<LittleEndian>(keyed_account.data_len()? as u64)
                .unwrap();
            v.write_all(&keyed_account.try_account_ref()?.data).unwrap();
            v.write_all(keyed_account.owner()?.as_ref()).unwrap();
            v.write_u8(keyed_account.executable()? as u8).unwrap();
            v.write_u64::<LittleEndian>(keyed_account.rent_epoch()? as u64)
                .unwrap();
        }
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    Ok(v)
}

pub fn deserialize_parameters(
    keyed_accounts: &[KeyedAccount],
    buffer: &[u8],
) -> Result<(), InstructionError> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut start = mem::size_of::<u64>(); // number of accounts
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, _) = is_dup(&keyed_accounts[..i], keyed_account);
        start += 1; // is_dup
        if !is_dup {
            start += mem::size_of::<u8>(); // is_signer
            start += mem::size_of::<u8>(); // is_writable
            start += mem::size_of::<Pubkey>(); // pubkey
            keyed_account.try_account_ref_mut()?.lamports =
                LittleEndian::read_u64(&buffer[start..]);
            start += mem::size_of::<u64>() // lamports
                + mem::size_of::<u64>(); // data length
            let end = start + keyed_account.data_len()?;
            keyed_account
                .try_account_ref_mut()?
                .data
                .clone_from_slice(&buffer[start..end]);
            start += keyed_account.data_len()? // data
                + mem::size_of::<Pubkey>() // owner
                + mem::size_of::<u8>() // executable
                + mem::size_of::<u64>(); // rent_epoch
        }
    }
    Ok(())
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    solana_logger::setup_with_default("solana=info");

    debug_assert!(bpf_loader::check_id(program_id));

    if keyed_accounts.is_empty() {
        warn!("No account keys");
        return Err(InstructionError::NotEnoughAccountKeys);
    }

    if is_executable(keyed_accounts)? {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let program = next_keyed_account(&mut keyed_accounts_iter)?;

        let parameter_accounts = keyed_accounts_iter.as_slice();
        let parameter_bytes = serialize_parameters(
            program.unsigned_key(),
            parameter_accounts,
            &instruction_data,
        )?;
        {
            let program_account = program.try_account_ref_mut()?;
            let (mut vm, heap_region) = match create_vm(&program_account.data, invoke_context) {
                Ok(info) => info,
                Err(e) => {
                    warn!("Failed to create BPF VM: {}", e);
                    return Err(BPFLoaderError::VirtualMachineCreationFailed.into());
                }
            };

            info!("Call BPF program {}", program.unsigned_key());
            match vm.execute_program(parameter_bytes.as_slice(), &[], &[heap_region]) {
                Ok(status) => {
                    if status != SUCCESS {
                        let error: InstructionError = status.into();
                        warn!("BPF program {} failed: {}", program.unsigned_key(), error);
                        return Err(error);
                    }
                }
                Err(error) => {
                    warn!("BPF program {} failed: {}", program.unsigned_key(), error);
                    return match error {
                        EbpfError::UserError(BPFError::SyscallError(
                            SyscallError::InstructionError(error),
                        )) => Err(error),
                        _ => Err(BPFLoaderError::VirtualMachineFailedToRunProgram.into()),
                    };
                }
            }
        }
        deserialize_parameters(parameter_accounts, &parameter_bytes)?;
        info!("BPF program {} success", program.unsigned_key());
    } else if !keyed_accounts.is_empty() {
        match limited_deserialize(instruction_data)? {
            LoaderInstruction::Write { offset, bytes } => {
                let mut keyed_accounts_iter = keyed_accounts.iter();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;
                if program.signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }
                let offset = offset as usize;
                let len = bytes.len();
                trace!("Write: offset={} length={}", offset, len);
                if program.data_len()? < offset + len {
                    warn!("Write overflow: {} < {}", program.data_len()?, offset + len);
                    return Err(InstructionError::AccountDataTooSmall);
                }
                program.try_account_ref_mut()?.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                let mut keyed_accounts_iter = keyed_accounts.iter();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;

                if program.signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }

                if let Err(e) = check_elf(&program.try_account_ref()?.data) {
                    warn!("{}", e);
                    return Err(InstructionError::InvalidAccountData);
                }

                program.try_account_ref_mut()?.executable = true;
                info!("Finalize: account {:?}", program.signer_key().unwrap());
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
        account::Account, instruction::CompiledInstruction, message::Message, rent::Rent,
    };
    use std::{cell::RefCell, fs::File, io::Read, ops::Range, rc::Rc};

    #[derive(Debug, Default)]
    pub struct MockInvokeContext {
        key: Pubkey,
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
            _signers: &[Pubkey],
            _accounts: &[Rc<RefCell<Account>>],
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
            Ok(&self.key)
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
        vm.set_max_instruction_count(10).unwrap();
        vm.set_program(program).unwrap();
        vm.execute_program(input, &[], &[]).unwrap();
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
        let mut file = File::open("test_elfs/noop.so").expect("file open failed");
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
        solana_logger::setup();

        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop.so").expect("file open failed");
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
        let mut file = File::open("test_elfs/noop.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        info!("mangle the whole file");
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
