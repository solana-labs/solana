pub mod alloc;
pub mod allocator_bump;
pub mod bpf_verifier;
pub mod helpers;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use log::*;
use solana_rbpf::{memory_region::MemoryRegion, EbpfVm};
use solana_sdk::{
    account::KeyedAccount,
    instruction::InstructionError,
    instruction_processor_utils::{limited_deserialize, next_keyed_account},
    loader_instruction::LoaderInstruction,
    pubkey::Pubkey,
    sysvar::rent,
};
use std::{
    convert::TryFrom,
    io::{prelude::*, Error},
    mem,
};

solana_sdk::declare_program!(
    solana_sdk::bpf_loader::ID,
    solana_bpf_loader_program,
    process_instruction
);

pub fn create_vm(prog: &[u8]) -> Result<(EbpfVm, MemoryRegion), Error> {
    let mut vm = EbpfVm::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_max_instruction_count(100_000)?;
    vm.set_elf(&prog)?;

    let heap_region = helpers::register_helpers(&mut vm)?;

    Ok((vm, heap_region))
}

pub fn check_elf(prog: &[u8]) -> Result<(), Error> {
    let mut vm = EbpfVm::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_elf(&prog)?;
    Ok(())
}

pub fn serialize_parameters(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Vec<u8> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for info in keyed_accounts.iter_mut() {
        v.write_u64::<LittleEndian>(info.signer_key().is_some() as u64)
            .unwrap();
        v.write_all(info.unsigned_key().as_ref()).unwrap();
        v.write_u64::<LittleEndian>(info.account.lamports).unwrap();
        v.write_u64::<LittleEndian>(info.account.data.len() as u64)
            .unwrap();
        v.write_all(&info.account.data).unwrap();
        v.write_all(info.account.owner.as_ref()).unwrap();
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    v
}

pub fn deserialize_parameters(keyed_accounts: &mut [KeyedAccount], buffer: &[u8]) {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut start = mem::size_of::<u64>();
    for info in keyed_accounts.iter_mut() {
        start += mem::size_of::<u64>(); // skip signer_key boolean
        start += mem::size_of::<Pubkey>(); // skip pubkey
        info.account.lamports = LittleEndian::read_u64(&buffer[start..]);

        start += mem::size_of::<u64>() // skip lamports
                  + mem::size_of::<u64>(); // skip length tag
        let end = start + info.account.data.len();
        info.account.data.clone_from_slice(&buffer[start..end]);

        start += info.account.data.len() // skip data
                  + mem::size_of::<Pubkey>(); // skip owner
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    ix_data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup_with_default("solana=info");

    if let Ok(instruction) = limited_deserialize(ix_data) {
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                let mut keyed_accounts_iter = keyed_accounts.iter_mut();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;
                if program.signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }
                let offset = offset as usize;
                let len = bytes.len();
                trace!("Write: offset={} length={}", offset, len);
                if program.account.data.len() < offset + len {
                    warn!(
                        "Write overflow: {} < {}",
                        program.account.data.len(),
                        offset + len
                    );
                    return Err(InstructionError::AccountDataTooSmall);
                }
                program.account.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                let mut keyed_accounts_iter = keyed_accounts.iter_mut();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;
                let rent = next_keyed_account(&mut keyed_accounts_iter)?;

                if program.signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }

                if let Err(e) = check_elf(&program.account.data) {
                    warn!("Invalid ELF: {}", e);
                    return Err(InstructionError::InvalidAccountData);
                }

                rent::verify_rent_exemption(&program, &rent)?;

                program.account.executable = true;
                info!("Finalize: account {:?}", program.signer_key().unwrap());
            }
            LoaderInstruction::InvokeMain { data } => {
                let mut keyed_accounts_iter = keyed_accounts.iter_mut();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;

                if !program.account.executable {
                    warn!("BPF program account not executable");
                    return Err(InstructionError::AccountNotExecutable);
                }
                let (mut vm, heap_region) = match create_vm(&program.account.data) {
                    Ok(info) => info,
                    Err(e) => {
                        warn!("Failed to create BPF VM: {}", e);
                        return Err(InstructionError::GenericError);
                    }
                };
                let parameter_accounts = keyed_accounts_iter.into_slice();
                let mut parameter_bytes =
                    serialize_parameters(program_id, parameter_accounts, &data);

                info!("Call BPF program");
                match vm.execute_program(parameter_bytes.as_mut_slice(), &[], &[heap_region]) {
                    Ok(status) => match u32::try_from(status) {
                        Ok(status) => {
                            if status > 0 {
                                warn!("BPF program failed: {}", status);
                                return Err(InstructionError::CustomError(status));
                            }
                        }
                        Err(e) => {
                            warn!("BPF VM encountered invalid status: {}", e);
                            return Err(InstructionError::GenericError);
                        }
                    },
                    Err(e) => {
                        warn!("BPF VM failed to run program: {}", e);
                        return Err(InstructionError::GenericError);
                    }
                }
                deserialize_parameters(parameter_accounts, &parameter_bytes);
                info!("BPF program success");
            }
        }
    } else {
        warn!("Invalid instruction data: {:?}", ix_data);
        return Err(InstructionError::InvalidInstructionData);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;
    use std::fs::File;
    use std::io::Read;

    #[test]
    #[should_panic(expected = "Error: Exceeded maximum number of instructions allowed")]
    fn test_bpf_loader_non_terminating_program() {
        #[rustfmt::skip]
        let program = &[
            0x07, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // r6 + 1
            0x05, 0x00, 0xfe, 0xff, 0x00, 0x00, 0x00, 0x00, // goto -2
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // exit
        ];
        let input = &mut [0x00];

        let mut vm = EbpfVm::new(None).unwrap();
        vm.set_verifier(bpf_verifier::check).unwrap();
        vm.set_max_instruction_count(10).unwrap();
        vm.set_program(program).unwrap();
        vm.execute_program(input, &[], &[]).unwrap();
    }

    #[test]
    fn test_bpf_loader_write() {
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();
        let mut program_account = Account::new(1, 0, &program_id);
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];
        let ix_data = bincode::serialize(&LoaderInstruction::Write {
            offset: 3,
            bytes: vec![1, 2, 3],
        })
        .unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &ix_data)
        );

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );

        // Case: Write bytes to an offset
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &mut program_account)];
        keyed_accounts[0].account.data = vec![0; 6];
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
        assert_eq!(vec![0, 0, 0, 1, 2, 3], keyed_accounts[0].account.data);

        // Case: Overflow
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &mut program_account)];
        keyed_accounts[0].account.data = vec![0; 5];
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
    }

    #[test]
    fn test_bpf_loader_finalize() {
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();
        let rent_key = rent::id();
        let mut file = File::open("test_elfs/noop.so").expect("file open failed");
        let mut elf = Vec::new();
        let rent = rent::Rent::default();
        file.read_to_end(&mut elf).unwrap();
        let mut program_account = Account::new(rent.minimum_balance(elf.len()), 0, &program_id);
        program_account.data = elf;
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];
        let ix_data = bincode::serialize(&LoaderInstruction::Finalize).unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &ix_data)
        );

        let mut rent_account = rent::create_account(1, &rent);
        keyed_accounts.push(KeyedAccount::new(&rent_key, false, &mut rent_account));

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );

        // Case: Finalize
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program_key, true, &mut program_account),
            KeyedAccount::new(&rent_key, false, &mut rent_account),
        ];
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
        assert!(keyed_accounts[0].account.executable);

        // Case: Finalize
        program_account.data[0] = 0; // bad elf
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program_key, true, &mut program_account),
            KeyedAccount::new(&rent_key, false, &mut rent_account),
        ];
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
    }

    #[test]
    fn test_bpf_loader_invoke_main() {
        let program_id = Pubkey::new_rand();
        let program_key = Pubkey::new_rand();

        // Create program account
        let mut file = File::open("test_elfs/noop.so").expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();
        let mut program_account = Account::new(1, 0, &program_id);
        program_account.data = elf;
        program_account.executable = true;

        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];
        let ix_data = bincode::serialize(&LoaderInstruction::InvokeMain { data: vec![] }).unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &ix_data)
        );

        // Case: Only a program account
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );

        // Case: Account not executable
        keyed_accounts[0].account.executable = false;
        assert_eq!(
            Err(InstructionError::AccountNotExecutable),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
        keyed_accounts[0].account.executable = true;

        // Case: With program and parameter account
        let mut parameter_account = Account::new(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(
            &program_key,
            false,
            &mut parameter_account,
        ));
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &ix_data)
        );
    }
}
