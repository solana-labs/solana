pub mod alloc;
pub mod allocator_bump;
pub mod bpf_verifier;
pub mod helpers;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use log::*;
use solana_rbpf::{memory_region::MemoryRegion, EbpfVm};
use solana_sdk::{
    account::KeyedAccount,
    hash::{Hash, Hasher},
    instruction::InstructionError,
    instruction_processor_utils::{is_executable, limited_deserialize, next_keyed_account},
    loader_instruction::LoaderInstruction,
    pubkey::Pubkey,
    sysvar::rent,
};
use std::{
    collections::HashMap,
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
) -> Result<Vec<u8>, InstructionError> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for keyed_account in keyed_accounts.iter_mut() {
        v.write_u64::<LittleEndian>(keyed_account.signer_key().is_some() as u64)
            .unwrap();
        v.write_all(keyed_account.unsigned_key().as_ref()).unwrap();
        v.write_u64::<LittleEndian>(keyed_account.lamports()?)
            .unwrap();
        v.write_u64::<LittleEndian>(keyed_account.data_len()? as u64)
            .unwrap();
        v.write_all(&keyed_account.try_account_ref()?.data).unwrap();
        v.write_all(keyed_account.owner()?.as_ref()).unwrap();
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    Ok(v)
}

pub fn deserialize_parameters(
    keyed_accounts: &mut [KeyedAccount],
    buffer: &[u8],
) -> Result<(), InstructionError> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let calculate_hash = |lamports: u64, data: &[u8]| -> Hash {
        let mut hasher = Hasher::default();
        let mut buf = [0u8; 8];
        LittleEndian::write_u64(&mut buf[..], lamports);
        hasher.hash(&buf);
        hasher.hash(data);
        hasher.result()
    };

    // remember any duplicate accounts
    let mut map: HashMap<Pubkey, (Hash, bool)> = HashMap::new();
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        if keyed_accounts[i + 1..].contains(keyed_account)
            && !map.contains_key(keyed_account.unsigned_key())
        {
            let hash = calculate_hash(
                keyed_account.lamports()?,
                &keyed_account.try_account_ref()?.data,
            );
            map.insert(*keyed_account.unsigned_key(), (hash, false));
        }
    }

    let mut start = mem::size_of::<u64>();
    for keyed_account in keyed_accounts.iter_mut() {
        start += mem::size_of::<u64>() // signer_key boolean
            + mem::size_of::<Pubkey>(); // pubkey
        let lamports = LittleEndian::read_u64(&buffer[start..]);
        start += mem::size_of::<u64>() // lamports
            + mem::size_of::<u64>(); // length tag
        let end = start + keyed_account.data_len()?;
        let data_start = start;
        let data_end = end;

        // if duplicate, modified, and dirty, then bail
        let mut do_update = true;
        if let Some((hash, is_dirty)) = map.get_mut(keyed_account.unsigned_key()) {
            let new_hash = calculate_hash(lamports, &buffer[data_start..data_end]);
            if *hash != new_hash {
                if *is_dirty {
                    return Err(InstructionError::DuplicateAccountOutOfSync);
                }
                *is_dirty = true; // fail if modified again
            } else {
                do_update = false; // no changes, don't need to update account
            }
        }
        if do_update {
            keyed_account.try_account_ref_mut()?.lamports = lamports;
            keyed_account
                .try_account_ref_mut()?
                .data
                .clone_from_slice(&buffer[data_start..data_end]);
        }

        start += keyed_account.data_len()? // data
            + mem::size_of::<Pubkey>(); // owner
    }
    Ok(())
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup_with_default("solana=info");

    if keyed_accounts.is_empty() {
        warn!("No account keys");
        return Err(InstructionError::NotEnoughAccountKeys);
    }

    if is_executable(keyed_accounts)? {
        let mut keyed_accounts_iter = keyed_accounts.iter_mut();
        let program = next_keyed_account(&mut keyed_accounts_iter)?;
        let program_account = program.try_account_ref_mut()?;
        let (mut vm, heap_region) = match create_vm(&program_account.data) {
            Ok(info) => info,
            Err(e) => {
                warn!("Failed to create BPF VM: {}", e);
                return Err(InstructionError::GenericError);
            }
        };
        let parameter_accounts = keyed_accounts_iter.into_slice();
        let mut parameter_bytes =
            serialize_parameters(program_id, parameter_accounts, &instruction_data)?;

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
        deserialize_parameters(parameter_accounts, &parameter_bytes)?;
        info!("BPF program success");
    } else if !keyed_accounts.is_empty() {
        match limited_deserialize(instruction_data)? {
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
                if program.data_len()? < offset + len {
                    warn!("Write overflow: {} < {}", program.data_len()?, offset + len);
                    return Err(InstructionError::AccountDataTooSmall);
                }
                program.try_account_ref_mut()?.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                let mut keyed_accounts_iter = keyed_accounts.iter_mut();
                let program = next_keyed_account(&mut keyed_accounts_iter)?;
                let rent = next_keyed_account(&mut keyed_accounts_iter)?;

                if program.signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::MissingRequiredSignature);
                }

                if let Err(e) = check_elf(&program.try_account_ref()?.data) {
                    warn!("Invalid ELF: {}", e);
                    return Err(InstructionError::InvalidAccountData);
                }

                rent::verify_rent_exemption(&program, &rent)?;

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
    use solana_sdk::account::Account;
    use std::{cell::RefCell, fs::File, io::Read};

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
        let mut program_account = Account::new_ref(1, 0, &program_id);
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];
        let instruction_data = bincode::serialize(&LoaderInstruction::Write {
            offset: 3,
            bytes: vec![1, 2, 3],
        })
        .unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &instruction_data)
        );

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
        );

        // Case: Write bytes to an offset
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &mut program_account)];
        keyed_accounts[0].account.borrow_mut().data = vec![0; 6];
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
        );
        assert_eq!(
            vec![0, 0, 0, 1, 2, 3],
            keyed_accounts[0].account.borrow().data
        );

        // Case: Overflow
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, true, &mut program_account)];
        keyed_accounts[0].account.borrow_mut().data = vec![0; 5];
        assert_eq!(
            Err(InstructionError::AccountDataTooSmall),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
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
        let mut program_account = Account::new_ref(rent.minimum_balance(elf.len()), 0, &program_id);
        program_account.borrow_mut().data = elf;
        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];
        let instruction_data = bincode::serialize(&LoaderInstruction::Finalize).unwrap();

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &instruction_data)
        );

        let mut rent_account = RefCell::new(rent::create_account(1, &rent));
        keyed_accounts.push(KeyedAccount::new(&rent_key, false, &mut rent_account));

        // Case: Not signed
        assert_eq!(
            Err(InstructionError::MissingRequiredSignature),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
        );

        // Case: Finalize
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program_key, true, &mut program_account),
            KeyedAccount::new(&rent_key, false, &mut rent_account),
        ];
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
        );
        assert!(keyed_accounts[0].account.borrow().executable);

        program_account.borrow_mut().executable = false; // Un-finalize the account

        // Case: Finalize
        program_account.borrow_mut().data[0] = 0; // bad elf
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program_key, true, &mut program_account),
            KeyedAccount::new(&rent_key, false, &mut rent_account),
        ];
        assert_eq!(
            Err(InstructionError::InvalidAccountData),
            process_instruction(&program_id, &mut keyed_accounts, &instruction_data)
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
        let mut program_account = Account::new_ref(1, 0, &program_id);
        program_account.borrow_mut().data = elf;
        program_account.borrow_mut().executable = true;

        let mut keyed_accounts = vec![KeyedAccount::new(&program_key, false, &mut program_account)];

        // Case: Empty keyed accounts
        assert_eq!(
            Err(InstructionError::NotEnoughAccountKeys),
            process_instruction(&program_id, &mut vec![], &vec![])
        );

        // Case: Only a program account
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &vec![])
        );

        // Case: Account not executable
        keyed_accounts[0].account.borrow_mut().executable = false;
        assert_eq!(
            Err(InstructionError::InvalidInstructionData),
            process_instruction(&program_id, &mut keyed_accounts, &vec![])
        );
        keyed_accounts[0].account.borrow_mut().executable = true;

        // Case: With program and parameter account
        let mut parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(
            &program_key,
            false,
            &mut parameter_account,
        ));
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &vec![])
        );

        // Case: With duplicate accounts
        let duplicate_key = Pubkey::new_rand();
        let parameter_account = Account::new_ref(1, 0, &program_id);
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        keyed_accounts.push(KeyedAccount::new(&duplicate_key, false, &parameter_account));
        assert_eq!(
            Ok(()),
            process_instruction(&program_id, &mut keyed_accounts, &vec![])
        );
    }
}
