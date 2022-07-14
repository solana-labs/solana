#![allow(clippy::integer_arithmetic)]

use {
    byteorder::{ByteOrder, LittleEndian, WriteBytesExt},
    solana_rbpf::{aligned_memory::AlignedMemory, ebpf::HOST_ALIGN},
    solana_sdk::{
        bpf_loader_deprecated,
        entrypoint::{BPF_ALIGN_OF_U128, MAX_PERMITTED_DATA_INCREASE, NON_DUP_MARKER},
        instruction::InstructionError,
        pubkey::Pubkey,
        system_instruction::MAX_PERMITTED_DATA_LENGTH,
        transaction_context::{InstructionContext, TransactionContext},
    },
    std::{io::prelude::*, mem::size_of},
};

/// Maximum number of instruction accounts that can be serialized into the
/// BPF VM.
const MAX_INSTRUCTION_ACCOUNTS: u8 = NON_DUP_MARKER;

pub fn serialize_parameters(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    should_cap_ix_accounts: bool,
) -> Result<(AlignedMemory, Vec<usize>), InstructionError> {
    let num_ix_accounts = instruction_context.get_number_of_instruction_accounts();
    if should_cap_ix_accounts && num_ix_accounts > usize::from(MAX_INSTRUCTION_ACCOUNTS) {
        return Err(InstructionError::MaxAccountsExceeded);
    }

    let is_loader_deprecated = *instruction_context
        .try_borrow_last_program_account(transaction_context)?
        .get_owner()
        == bpf_loader_deprecated::id();
    if is_loader_deprecated {
        serialize_parameters_unaligned(transaction_context, instruction_context)
    } else {
        serialize_parameters_aligned(transaction_context, instruction_context)
    }
    .and_then(|buffer| {
        let account_lengths = (0..instruction_context.get_number_of_instruction_accounts())
            .map(|instruction_account_index| {
                Ok(instruction_context
                    .try_borrow_instruction_account(transaction_context, instruction_account_index)?
                    .get_data()
                    .len())
            })
            .collect::<Result<Vec<usize>, InstructionError>>()?;
        Ok((buffer, account_lengths))
    })
}

pub fn deserialize_parameters(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    buffer: &[u8],
    account_lengths: &[usize],
) -> Result<(), InstructionError> {
    let is_loader_deprecated = *instruction_context
        .try_borrow_last_program_account(transaction_context)?
        .get_owner()
        == bpf_loader_deprecated::id();
    if is_loader_deprecated {
        deserialize_parameters_unaligned(
            transaction_context,
            instruction_context,
            buffer,
            account_lengths,
        )
    } else {
        deserialize_parameters_aligned(
            transaction_context,
            instruction_context,
            buffer,
            account_lengths,
        )
    }
}

pub fn serialize_parameters_unaligned(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<AlignedMemory, InstructionError> {
    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for instruction_account_index in 0..instruction_context.get_number_of_instruction_accounts() {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        size += 1; // dup
        if duplicate.is_none() {
            let data_len = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?
                .get_data()
                .len();
            size += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<Pubkey>() // key
                + size_of::<u64>()  // lamports
                + size_of::<u64>()  // data len
                + data_len // data
                + size_of::<Pubkey>() // owner
                + size_of::<u8>() // executable
                + size_of::<u64>(); // rent_epoch
        }
    }
    size += size_of::<u64>() // instruction data len
         + instruction_context.get_instruction_data().len() // instruction data
         + size_of::<Pubkey>(); // program id
    let mut v = AlignedMemory::new(size, HOST_ALIGN);

    v.write_u64::<LittleEndian>(instruction_context.get_number_of_instruction_accounts() as u64)
        .map_err(|_| InstructionError::InvalidArgument)?;
    for instruction_account_index in 0..instruction_context.get_number_of_instruction_accounts() {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        if let Some(position) = duplicate {
            v.write_u8(position as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
        } else {
            let borrowed_account = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?;
            v.write_u8(NON_DUP_MARKER)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_signer() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_writable() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(borrowed_account.get_key().as_ref())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_lamports())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_data().len() as u64)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(borrowed_account.get_data())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(borrowed_account.get_owner().as_ref())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_executable() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_rent_epoch() as u64)
                .map_err(|_| InstructionError::InvalidArgument)?;
        }
    }
    v.write_u64::<LittleEndian>(instruction_context.get_instruction_data().len() as u64)
        .map_err(|_| InstructionError::InvalidArgument)?;
    v.write_all(instruction_context.get_instruction_data())
        .map_err(|_| InstructionError::InvalidArgument)?;
    v.write_all(
        instruction_context
            .try_borrow_last_program_account(transaction_context)?
            .get_key()
            .as_ref(),
    )
    .map_err(|_| InstructionError::InvalidArgument)?;
    Ok(v)
}

pub fn deserialize_parameters_unaligned(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    buffer: &[u8],
    account_lengths: &[usize],
) -> Result<(), InstructionError> {
    let mut start = size_of::<u64>(); // number of accounts
    for (instruction_account_index, pre_len) in
        (0..instruction_context.get_number_of_instruction_accounts()).zip(account_lengths.iter())
    {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        start += 1; // is_dup
        if duplicate.is_none() {
            let mut borrowed_account = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?;
            start += size_of::<u8>(); // is_signer
            start += size_of::<u8>(); // is_writable
            start += size_of::<Pubkey>(); // key
            let lamports = LittleEndian::read_u64(
                buffer
                    .get(start..)
                    .ok_or(InstructionError::InvalidArgument)?,
            );
            if borrowed_account.get_lamports() != lamports {
                borrowed_account.set_lamports(lamports)?;
            }
            start += size_of::<u64>() // lamports
                + size_of::<u64>(); // data length
            let data = buffer
                .get(start..start + pre_len)
                .ok_or(InstructionError::InvalidArgument)?;
            // The redundant check helps to avoid the expensive data comparison if we can
            match borrowed_account
                .can_data_be_resized(data.len())
                .and_then(|_| borrowed_account.can_data_be_changed())
            {
                Ok(()) => borrowed_account.set_data(data)?,
                Err(err) if borrowed_account.get_data() != data => return Err(err),
                _ => {}
            }
            start += pre_len // data
                + size_of::<Pubkey>() // owner
                + size_of::<u8>() // executable
                + size_of::<u64>(); // rent_epoch
        }
    }
    Ok(())
}

pub fn serialize_parameters_aligned(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<AlignedMemory, InstructionError> {
    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for instruction_account_index in 0..instruction_context.get_number_of_instruction_accounts() {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        size += 1; // dup
        if duplicate.is_some() {
            size += 7; // padding to 64-bit aligned
        } else {
            let data_len = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?
                .get_data()
                .len();
            size += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<u8>() // executable
                + size_of::<u32>() // original_data_len
                + size_of::<Pubkey>()  // key
                + size_of::<Pubkey>() // owner
                + size_of::<u64>()  // lamports
                + size_of::<u64>()  // data len
                + data_len
                + MAX_PERMITTED_DATA_INCREASE
                + (data_len as *const u8).align_offset(BPF_ALIGN_OF_U128)
                + size_of::<u64>(); // rent epoch
        }
    }
    size += size_of::<u64>() // data len
    + instruction_context.get_instruction_data().len()
    + size_of::<Pubkey>(); // program id;
    let mut v = AlignedMemory::new(size, HOST_ALIGN);

    // Serialize into the buffer
    v.write_u64::<LittleEndian>(instruction_context.get_number_of_instruction_accounts() as u64)
        .map_err(|_| InstructionError::InvalidArgument)?;
    for instruction_account_index in 0..instruction_context.get_number_of_instruction_accounts() {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        if let Some(position) = duplicate {
            v.write_u8(position as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(&[0u8, 0, 0, 0, 0, 0, 0])
                .map_err(|_| InstructionError::InvalidArgument)?; // 7 bytes of padding to make 64-bit aligned
        } else {
            let borrowed_account = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?;
            v.write_u8(NON_DUP_MARKER)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_signer() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_writable() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u8(borrowed_account.is_executable() as u8)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(&[0u8, 0, 0, 0])
                .map_err(|_| InstructionError::InvalidArgument)?; // 4 bytes of padding to make 128-bit aligned
            v.write_all(borrowed_account.get_key().as_ref())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(borrowed_account.get_owner().as_ref())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_lamports())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_data().len() as u64)
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_all(borrowed_account.get_data())
                .map_err(|_| InstructionError::InvalidArgument)?;
            v.resize(
                MAX_PERMITTED_DATA_INCREASE
                    + (v.write_index() as *const u8).align_offset(BPF_ALIGN_OF_U128),
                0,
            )
            .map_err(|_| InstructionError::InvalidArgument)?;
            v.write_u64::<LittleEndian>(borrowed_account.get_rent_epoch() as u64)
                .map_err(|_| InstructionError::InvalidArgument)?;
        }
    }
    v.write_u64::<LittleEndian>(instruction_context.get_instruction_data().len() as u64)
        .map_err(|_| InstructionError::InvalidArgument)?;
    v.write_all(instruction_context.get_instruction_data())
        .map_err(|_| InstructionError::InvalidArgument)?;
    v.write_all(
        instruction_context
            .try_borrow_last_program_account(transaction_context)?
            .get_key()
            .as_ref(),
    )
    .map_err(|_| InstructionError::InvalidArgument)?;
    Ok(v)
}

pub fn deserialize_parameters_aligned(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    buffer: &[u8],
    account_lengths: &[usize],
) -> Result<(), InstructionError> {
    let mut start = size_of::<u64>(); // number of accounts
    for (instruction_account_index, pre_len) in
        (0..instruction_context.get_number_of_instruction_accounts()).zip(account_lengths.iter())
    {
        let duplicate =
            instruction_context.is_instruction_account_duplicate(instruction_account_index)?;
        start += size_of::<u8>(); // position
        if duplicate.is_some() {
            start += 7; // padding to 64-bit aligned
        } else {
            let mut borrowed_account = instruction_context
                .try_borrow_instruction_account(transaction_context, instruction_account_index)?;
            start += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<u8>() // executable
                + size_of::<u32>() // original_data_len
                + size_of::<Pubkey>(); // key
            let owner = buffer
                .get(start..start + size_of::<Pubkey>())
                .ok_or(InstructionError::InvalidArgument)?;
            start += size_of::<Pubkey>(); // owner
            let lamports = LittleEndian::read_u64(
                buffer
                    .get(start..)
                    .ok_or(InstructionError::InvalidArgument)?,
            );
            if borrowed_account.get_lamports() != lamports {
                borrowed_account.set_lamports(lamports)?;
            }
            start += size_of::<u64>(); // lamports
            let post_len = LittleEndian::read_u64(
                buffer
                    .get(start..)
                    .ok_or(InstructionError::InvalidArgument)?,
            ) as usize;
            start += size_of::<u64>(); // data length
            if post_len.saturating_sub(*pre_len) > MAX_PERMITTED_DATA_INCREASE
                || post_len > MAX_PERMITTED_DATA_LENGTH as usize
            {
                return Err(InstructionError::InvalidRealloc);
            }
            let data_end = start + post_len;
            let data = buffer
                .get(start..data_end)
                .ok_or(InstructionError::InvalidArgument)?;
            // The redundant check helps to avoid the expensive data comparison if we can
            match borrowed_account
                .can_data_be_resized(data.len())
                .and_then(|_| borrowed_account.can_data_be_changed())
            {
                Ok(()) => borrowed_account.set_data(data)?,
                Err(err) if borrowed_account.get_data() != data => return Err(err),
                _ => {}
            }
            start += *pre_len + MAX_PERMITTED_DATA_INCREASE; // data
            start += (start as *const u8).align_offset(BPF_ALIGN_OF_U128);
            start += size_of::<u64>(); // rent_epoch
            if borrowed_account.get_owner().to_bytes() != owner {
                // Change the owner at the end so that we are allowed to change the lamports and data before
                borrowed_account.set_owner(owner)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program_runtime::invoke_context::{prepare_mock_invoke_context, InvokeContext},
        solana_sdk::{
            account::{Account, AccountSharedData, ReadableAccount, WritableAccount},
            account_info::AccountInfo,
            bpf_loader,
            entrypoint::deserialize,
            instruction::AccountMeta,
            sysvar::rent::Rent,
        },
        std::{
            cell::RefCell,
            rc::Rc,
            slice::{from_raw_parts, from_raw_parts_mut},
        },
    };

    #[test]
    fn test_serialize_parameters_with_many_accounts() {
        struct TestCase {
            num_ix_accounts: usize,
            append_dup_account: bool,
            should_cap_ix_accounts: bool,
            expected_err: Option<InstructionError>,
            name: &'static str,
        }

        for TestCase {
            num_ix_accounts,
            append_dup_account,
            should_cap_ix_accounts,
            expected_err,
            name,
        } in [
            TestCase {
                name: "serialize max accounts without cap",
                num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS),
                should_cap_ix_accounts: false,
                append_dup_account: false,
                expected_err: None,
            },
            TestCase {
                name: "serialize max accounts and append dup without cap",
                num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS),
                should_cap_ix_accounts: false,
                append_dup_account: true,
                expected_err: None,
            },
            TestCase {
                name: "serialize max accounts with cap",
                num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS),
                should_cap_ix_accounts: true,
                append_dup_account: false,
                expected_err: None,
            },
            TestCase {
                name: "serialize too many accounts with cap",
                num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS) + 1,
                should_cap_ix_accounts: true,
                append_dup_account: false,
                expected_err: Some(InstructionError::MaxAccountsExceeded),
            },
            TestCase {
                name: "serialize too many accounts and append dup with cap",
                num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS),
                should_cap_ix_accounts: true,
                append_dup_account: true,
                expected_err: Some(InstructionError::MaxAccountsExceeded),
            },
            // This test case breaks parameter deserialization and can be cleaned up
            // when should_cap_ix_accounts is enabled.
            //
            // TestCase {
            //     name: "serialize too many accounts and append dup without cap",
            //     num_ix_accounts: usize::from(MAX_INSTRUCTION_ACCOUNTS) + 1,
            //     should_cap_ix_accounts: false,
            //     append_dup_account: true,
            //     expected_err: None,
            // },
        ] {
            let program_id = solana_sdk::pubkey::new_rand();
            let mut transaction_accounts = vec![(
                program_id,
                AccountSharedData::from(Account {
                    lamports: 0,
                    data: vec![],
                    owner: bpf_loader::id(),
                    executable: true,
                    rent_epoch: 0,
                }),
            )];

            let instruction_account_keys: Vec<Pubkey> =
                (0..num_ix_accounts).map(|_| Pubkey::new_unique()).collect();

            for key in &instruction_account_keys {
                transaction_accounts.push((
                    *key,
                    AccountSharedData::from(Account {
                        lamports: 0,
                        data: vec![],
                        owner: program_id,
                        executable: false,
                        rent_epoch: 0,
                    }),
                ));
            }

            let mut instruction_account_metas: Vec<_> = instruction_account_keys
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false))
                .collect();
            if append_dup_account {
                instruction_account_metas.push(instruction_account_metas.last().cloned().unwrap());
            }

            let program_indices = [0];
            let instruction_accounts = prepare_mock_invoke_context(
                transaction_accounts.clone(),
                instruction_account_metas,
                &program_indices,
            )
            .instruction_accounts;

            let transaction_context =
                TransactionContext::new(transaction_accounts, Some(Rent::default()), 1, 1);
            let instruction_data = vec![];
            let instruction_context = InstructionContext::new(
                0,
                0,
                &program_indices,
                &instruction_accounts,
                &instruction_data,
            );

            let serialization_result = serialize_parameters(
                &transaction_context,
                &instruction_context,
                should_cap_ix_accounts,
            );
            assert_eq!(
                serialization_result.as_ref().err(),
                expected_err.as_ref(),
                "{} test case failed",
                name
            );
            if expected_err.is_some() {
                continue;
            }

            let (mut serialized, _account_lengths) = serialization_result.unwrap();
            let (de_program_id, de_accounts, de_instruction_data) =
                unsafe { deserialize(serialized.as_slice_mut().first_mut().unwrap() as *mut u8) };
            assert_eq!(de_program_id, &program_id);
            assert_eq!(de_instruction_data, &instruction_data);
            for (index, account_info) in de_accounts.into_iter().enumerate() {
                let ix_account = &instruction_accounts.get(index).unwrap();
                assert_eq!(
                    account_info.key,
                    transaction_context
                        .get_key_of_account_at_index(ix_account.index_in_transaction)
                        .unwrap()
                );
                assert_eq!(account_info.owner, &program_id);
                assert!(!account_info.executable);
                assert!(account_info.data_is_empty());
                assert!(!account_info.is_writable);
                assert!(!account_info.is_signer);
            }
        }
    }

    #[test]
    fn test_serialize_parameters() {
        let program_id = solana_sdk::pubkey::new_rand();
        let transaction_accounts = vec![
            (
                program_id,
                AccountSharedData::from(Account {
                    lamports: 0,
                    data: vec![],
                    owner: bpf_loader::id(),
                    executable: true,
                    rent_epoch: 0,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 1,
                    data: vec![1u8, 2, 3, 4, 5],
                    owner: bpf_loader::id(),
                    executable: false,
                    rent_epoch: 100,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 2,
                    data: vec![11u8, 12, 13, 14, 15, 16, 17, 18, 19],
                    owner: bpf_loader::id(),
                    executable: true,
                    rent_epoch: 200,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 3,
                    data: vec![],
                    owner: bpf_loader::id(),
                    executable: false,
                    rent_epoch: 3100,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 4,
                    data: vec![1u8, 2, 3, 4, 5],
                    owner: bpf_loader::id(),
                    executable: false,
                    rent_epoch: 100,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 5,
                    data: vec![11u8, 12, 13, 14, 15, 16, 17, 18, 19],
                    owner: bpf_loader::id(),
                    executable: true,
                    rent_epoch: 200,
                }),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::from(Account {
                    lamports: 6,
                    data: vec![],
                    owner: bpf_loader::id(),
                    executable: false,
                    rent_epoch: 3100,
                }),
            ),
        ];
        let instruction_accounts = [1, 1, 2, 3, 4, 4, 5, 6]
            .into_iter()
            .enumerate()
            .map(
                |(instruction_account_index, index_in_transaction)| AccountMeta {
                    pubkey: transaction_accounts.get(index_in_transaction).unwrap().0,
                    is_signer: false,
                    is_writable: instruction_account_index >= 4,
                },
            )
            .collect();
        let instruction_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let program_indices = [0];
        let mut original_accounts = transaction_accounts.clone();
        let preparation = prepare_mock_invoke_context(
            transaction_accounts,
            instruction_accounts,
            &program_indices,
        );
        let mut transaction_context = TransactionContext::new(
            preparation.transaction_accounts,
            Some(Rent::default()),
            1,
            1,
        );
        let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        invoke_context
            .push(
                &preparation.instruction_accounts,
                &program_indices,
                &instruction_data,
            )
            .unwrap();
        let instruction_context = invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap();

        // check serialize_parameters_aligned

        let (mut serialized, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            instruction_context,
            true,
        )
        .unwrap();

        let (de_program_id, de_accounts, de_instruction_data) =
            unsafe { deserialize(serialized.as_slice_mut().first_mut().unwrap() as *mut u8) };

        assert_eq!(&program_id, de_program_id);
        assert_eq!(instruction_data, de_instruction_data);
        assert_eq!(
            (de_instruction_data.first().unwrap() as *const u8).align_offset(BPF_ALIGN_OF_U128),
            0
        );
        for account_info in de_accounts {
            let index_in_transaction = invoke_context
                .transaction_context
                .find_index_of_account(account_info.key)
                .unwrap();
            let account = invoke_context
                .transaction_context
                .get_account_at_index(index_in_transaction)
                .unwrap()
                .borrow();
            assert_eq!(account.lamports(), account_info.lamports());
            assert_eq!(account.data(), &account_info.data.borrow()[..]);
            assert_eq!(account.owner(), account_info.owner);
            assert_eq!(account.executable(), account_info.executable);
            assert_eq!(account.rent_epoch(), account_info.rent_epoch);

            assert_eq!(
                (*account_info.lamports.borrow() as *const u64).align_offset(BPF_ALIGN_OF_U128),
                0
            );
            assert_eq!(
                account_info
                    .data
                    .borrow()
                    .as_ptr()
                    .align_offset(BPF_ALIGN_OF_U128),
                0
            );
        }

        deserialize_parameters(
            invoke_context.transaction_context,
            instruction_context,
            serialized.as_slice(),
            &account_lengths,
        )
        .unwrap();
        for (index_in_transaction, (_key, original_account)) in original_accounts.iter().enumerate()
        {
            let account = invoke_context
                .transaction_context
                .get_account_at_index(index_in_transaction)
                .unwrap()
                .borrow();
            assert_eq!(&*account, original_account);
        }

        // check serialize_parameters_unaligned
        original_accounts
            .first_mut()
            .unwrap()
            .1
            .set_owner(bpf_loader_deprecated::id());
        invoke_context
            .transaction_context
            .get_account_at_index(0)
            .unwrap()
            .borrow_mut()
            .set_owner(bpf_loader_deprecated::id());

        let (mut serialized, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            instruction_context,
            true,
        )
        .unwrap();

        let (de_program_id, de_accounts, de_instruction_data) = unsafe {
            deserialize_unaligned(serialized.as_slice_mut().first_mut().unwrap() as *mut u8)
        };
        assert_eq!(&program_id, de_program_id);
        assert_eq!(instruction_data, de_instruction_data);
        for account_info in de_accounts {
            let index_in_transaction = invoke_context
                .transaction_context
                .find_index_of_account(account_info.key)
                .unwrap();
            let account = invoke_context
                .transaction_context
                .get_account_at_index(index_in_transaction)
                .unwrap()
                .borrow();
            assert_eq!(account.lamports(), account_info.lamports());
            assert_eq!(account.data(), &account_info.data.borrow()[..]);
            assert_eq!(account.owner(), account_info.owner);
            assert_eq!(account.executable(), account_info.executable);
            assert_eq!(account.rent_epoch(), account_info.rent_epoch);
        }

        deserialize_parameters(
            invoke_context.transaction_context,
            instruction_context,
            serialized.as_slice(),
            &account_lengths,
        )
        .unwrap();
        for (index_in_transaction, (_key, original_account)) in original_accounts.iter().enumerate()
        {
            let account = invoke_context
                .transaction_context
                .get_account_at_index(index_in_transaction)
                .unwrap()
                .borrow();
            assert_eq!(&*account, original_account);
        }
    }

    // the old bpf_loader in-program deserializer bpf_loader::id()
    #[allow(clippy::type_complexity)]
    pub unsafe fn deserialize_unaligned<'a>(
        input: *mut u8,
    ) -> (&'a Pubkey, Vec<AccountInfo<'a>>, &'a [u8]) {
        let mut offset: usize = 0;

        // number of accounts present

        #[allow(clippy::cast_ptr_alignment)]
        let num_accounts = *(input.add(offset) as *const u64) as usize;
        offset += size_of::<u64>();

        // account Infos

        let mut accounts = Vec::with_capacity(num_accounts);
        for _ in 0..num_accounts {
            let dup_info = *(input.add(offset) as *const u8);
            offset += size_of::<u8>();
            if dup_info == NON_DUP_MARKER {
                #[allow(clippy::cast_ptr_alignment)]
                let is_signer = *(input.add(offset) as *const u8) != 0;
                offset += size_of::<u8>();

                #[allow(clippy::cast_ptr_alignment)]
                let is_writable = *(input.add(offset) as *const u8) != 0;
                offset += size_of::<u8>();

                let key: &Pubkey = &*(input.add(offset) as *const Pubkey);
                offset += size_of::<Pubkey>();

                #[allow(clippy::cast_ptr_alignment)]
                let lamports = Rc::new(RefCell::new(&mut *(input.add(offset) as *mut u64)));
                offset += size_of::<u64>();

                #[allow(clippy::cast_ptr_alignment)]
                let data_len = *(input.add(offset) as *const u64) as usize;
                offset += size_of::<u64>();

                let data = Rc::new(RefCell::new({
                    from_raw_parts_mut(input.add(offset), data_len)
                }));
                offset += data_len;

                let owner: &Pubkey = &*(input.add(offset) as *const Pubkey);
                offset += size_of::<Pubkey>();

                #[allow(clippy::cast_ptr_alignment)]
                let executable = *(input.add(offset) as *const u8) != 0;
                offset += size_of::<u8>();

                #[allow(clippy::cast_ptr_alignment)]
                let rent_epoch = *(input.add(offset) as *const u64);
                offset += size_of::<u64>();

                accounts.push(AccountInfo {
                    key,
                    is_signer,
                    is_writable,
                    lamports,
                    data,
                    owner,
                    executable,
                    rent_epoch,
                });
            } else {
                // duplicate account, clone the original
                accounts.push(accounts.get(dup_info as usize).unwrap().clone());
            }
        }

        // instruction data

        #[allow(clippy::cast_ptr_alignment)]
        let instruction_data_len = *(input.add(offset) as *const u64) as usize;
        offset += size_of::<u64>();

        let instruction_data = { from_raw_parts(input.add(offset), instruction_data_len) };
        offset += instruction_data_len;

        // program Id

        let program_id: &Pubkey = &*(input.add(offset) as *const Pubkey);

        (program_id, accounts, instruction_data)
    }
}
