use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use solana_sdk::{
    account::KeyedAccount, bpf_loader_deprecated, entrypoint::MAX_PERMITTED_DATA_INCREASE,
    instruction::InstructionError, pubkey::Pubkey,
};
use std::{
    io::prelude::*,
    mem::{align_of, size_of},
};

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
    loader_id: &Pubkey,
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<Vec<u8>, InstructionError> {
    if *loader_id == bpf_loader_deprecated::id() {
        serialize_parameters_unaligned(program_id, keyed_accounts, data)
    } else {
        serialize_parameters_aligned(program_id, keyed_accounts, data)
    }
}

pub fn deserialize_parameters(
    loader_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    buffer: &[u8],
) -> Result<(), InstructionError> {
    if *loader_id == bpf_loader_deprecated::id() {
        deserialize_parameters_unaligned(keyed_accounts, buffer)
    } else {
        deserialize_parameters_aligned(keyed_accounts, buffer)
    }
}

pub fn serialize_parameters_unaligned(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<Vec<u8>, InstructionError> {
    assert_eq!(32, size_of::<Pubkey>());

    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, _) = is_dup(&keyed_accounts[..i], keyed_account);
        size += 1; // dup, signer, writable, executable
        if !is_dup {
            let data_len = keyed_account.data_len()?;
            size += size_of::<Pubkey>()  // key
                    + size_of::<Pubkey>() // owner
                    + size_of::<u64>()  // lamports
                    + size_of::<u64>()  // data len
                    + data_len
                    + MAX_PERMITTED_DATA_INCREASE
                    + (data_len as *const u8).align_offset(align_of::<u128>())
                    + size_of::<u64>(); // rent epoch;
        }
    }
    size += size_of::<u64>() // data len
        + instruction_data.len()
        + size_of::<Pubkey>(); // program id;
    let mut v: Vec<u8> = Vec::with_capacity(size);

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
    v.write_u64::<LittleEndian>(instruction_data.len() as u64)
        .unwrap();
    v.write_all(instruction_data).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    Ok(v)
}

pub fn deserialize_parameters_unaligned(
    keyed_accounts: &[KeyedAccount],
    buffer: &[u8],
) -> Result<(), InstructionError> {
    assert_eq!(32, size_of::<Pubkey>());

    let mut start = size_of::<u64>(); // number of accounts
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, _) = is_dup(&keyed_accounts[..i], keyed_account);
        start += 1; // is_dup
        if !is_dup {
            start += size_of::<u8>(); // is_signer
            start += size_of::<u8>(); // is_writable
            start += size_of::<Pubkey>(); // pubkey
            keyed_account.try_account_ref_mut()?.lamports =
                LittleEndian::read_u64(&buffer[start..]);
            start += size_of::<u64>() // lamports
                + size_of::<u64>(); // data length
            let end = start + keyed_account.data_len()?;
            keyed_account
                .try_account_ref_mut()?
                .data
                .clone_from_slice(&buffer[start..end]);
            start += keyed_account.data_len()? // data
                + size_of::<Pubkey>() // owner
                + size_of::<u8>() // executable
                + size_of::<u64>(); // rent_epoch
        }
    }
    Ok(())
}

pub fn serialize_parameters_aligned(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<Vec<u8>, InstructionError> {
    assert_eq!(32, size_of::<Pubkey>());

    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, _) = is_dup(&keyed_accounts[..i], keyed_account);
        size += 8; // dup, signer, writable, executable
        if !is_dup {
            let data_len = keyed_account.data_len()?;
            size += size_of::<Pubkey>()  // key
                + size_of::<Pubkey>() // owner
                + size_of::<u64>()  // lamports
                + size_of::<u64>()  // data len
                + data_len
                + MAX_PERMITTED_DATA_INCREASE
                + (data_len as *const u8).align_offset(align_of::<u128>())
                + size_of::<u64>(); // rent epoch;
        }
    }
    size += size_of::<u64>() // data len
    + instruction_data.len()
    + size_of::<Pubkey>(); // program id;
    let mut v: Vec<u8> = Vec::with_capacity(size);

    // Serialize into the buffer
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    if v.as_ptr().align_offset(align_of::<u128>()) != 0 {
        panic!();
    }
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, position) = is_dup(&keyed_accounts[..i], keyed_account);
        if is_dup {
            v.write_u8(position as u8).unwrap();
            v.write_all(&[0u8, 0, 0, 0, 0, 0, 0]).unwrap(); // 7 bytes of padding to make 64-bit aligned
        } else {
            v.write_u8(std::u8::MAX).unwrap();
            v.write_u8(keyed_account.signer_key().is_some() as u8)
                .unwrap();
            v.write_u8(keyed_account.is_writable() as u8).unwrap();
            v.write_u8(keyed_account.executable()? as u8).unwrap();
            v.write_all(&[0u8, 0, 0, 0]).unwrap(); // 4 bytes of padding to make 128-bit aligned
            v.write_all(keyed_account.unsigned_key().as_ref()).unwrap();
            v.write_all(keyed_account.owner()?.as_ref()).unwrap();
            v.write_u64::<LittleEndian>(keyed_account.lamports()?)
                .unwrap();
            v.write_u64::<LittleEndian>(keyed_account.data_len()? as u64)
                .unwrap();
            v.write_all(&keyed_account.try_account_ref()?.data).unwrap();
            v.resize(
                v.len()
                    + MAX_PERMITTED_DATA_INCREASE
                    + (v.len() as *const u8).align_offset(align_of::<u128>()),
                0,
            );
            v.write_u64::<LittleEndian>(keyed_account.rent_epoch()? as u64)
                .unwrap();
        }
    }
    v.write_u64::<LittleEndian>(instruction_data.len() as u64)
        .unwrap();
    v.write_all(instruction_data).unwrap();
    v.write_all(program_id.as_ref()).unwrap();
    Ok(v)
}

pub fn deserialize_parameters_aligned(
    keyed_accounts: &[KeyedAccount],
    buffer: &[u8],
) -> Result<(), InstructionError> {
    assert_eq!(32, size_of::<Pubkey>());

    let mut start = size_of::<u64>(); // number of accounts
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let (is_dup, _) = is_dup(&keyed_accounts[..i], keyed_account);
        start += size_of::<u8>(); // position
        if is_dup {
            start += 7; // padding to 64-bit aligned
        } else {
            let mut account = keyed_account.try_account_ref_mut()?;
            start += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<u8>() // executable
                + 4 // padding to 128-bit aligned
                + size_of::<Pubkey>(); // key
            account.owner = Pubkey::new(&buffer[start..start + size_of::<Pubkey>()]);
            start += size_of::<Pubkey>(); // owner
            account.lamports = LittleEndian::read_u64(&buffer[start..]);
            start += size_of::<u64>(); // lamports
            let pre_len = account.data.len();
            let post_len = LittleEndian::read_u64(&buffer[start..]) as usize;
            start += size_of::<u64>(); // data length
            let mut data_end = start + pre_len;
            if post_len != pre_len
                && (post_len.saturating_sub(pre_len)) <= MAX_PERMITTED_DATA_INCREASE
            {
                account.data.resize(post_len, 0);
                data_end = start + post_len;
            }
            account.data.clone_from_slice(&buffer[start..data_end]);
            start += pre_len + MAX_PERMITTED_DATA_INCREASE; // data
            start += (start as *const u8).align_offset(align_of::<u128>());
            start += size_of::<u64>(); // rent_epoch
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        account::Account, account_info::AccountInfo, bpf_loader, entrypoint::deserialize,
    };
    use std::{
        cell::RefCell,
        rc::Rc,
        // Hide Result from bindgen gets confused about generics in non-generic type declarations
        slice::{from_raw_parts, from_raw_parts_mut},
    };

    #[test]
    fn test_serialize_parameters() {
        let program_id = solana_sdk::pubkey::new_rand();
        let dup_key = solana_sdk::pubkey::new_rand();
        let keys = vec![dup_key, dup_key, solana_sdk::pubkey::new_rand(), solana_sdk::pubkey::new_rand()];
        let accounts = [
            RefCell::new(Account {
                lamports: 1,
                data: vec![1u8, 2, 3, 4, 5],
                owner: bpf_loader::id(),
                executable: false,
                rent_epoch: 100,
            }),
            // dup of first
            RefCell::new(Account {
                lamports: 1,
                data: vec![1u8, 2, 3, 4, 5],
                owner: bpf_loader::id(),
                executable: false,
                rent_epoch: 100,
            }),
            RefCell::new(Account {
                lamports: 2,
                data: vec![11u8, 12, 13, 14, 15, 16, 17, 18, 19],
                owner: bpf_loader::id(),
                executable: true,
                rent_epoch: 200,
            }),
            RefCell::new(Account {
                lamports: 3,
                data: vec![],
                owner: bpf_loader::id(),
                executable: false,
                rent_epoch: 3100,
            }),
        ];

        let keyed_accounts: Vec<_> = keys
            .iter()
            .zip(&accounts)
            .map(|(key, account)| KeyedAccount::new(&key, false, &account))
            .collect();
        let instruction_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

        // check serialize_parameters_aligned

        let mut serialized = serialize_parameters(
            &bpf_loader::id(),
            &program_id,
            &keyed_accounts,
            &instruction_data,
        )
        .unwrap();
        let (de_program_id, de_accounts, de_instruction_data) =
            unsafe { deserialize(&mut serialized[0] as *mut u8) };

        assert_eq!(&program_id, de_program_id);
        assert_eq!(instruction_data, de_instruction_data);
        assert_eq!(
            (&de_instruction_data[0] as *const u8).align_offset(align_of::<u128>()),
            0
        );
        for ((account, account_info), key) in accounts.iter().zip(de_accounts).zip(keys.clone()) {
            assert_eq!(key, *account_info.key);
            let account = account.borrow();
            assert_eq!(account.lamports, account_info.lamports());
            assert_eq!(&account.data[..], &account_info.data.borrow()[..]);
            assert_eq!(&account.owner, account_info.owner);
            assert_eq!(account.executable, account_info.executable);
            assert_eq!(account.rent_epoch, account_info.rent_epoch);

            assert_eq!(
                (*account_info.lamports.borrow() as *const u64).align_offset(align_of::<u64>()),
                0
            );
            assert_eq!(
                account_info
                    .data
                    .borrow()
                    .as_ptr()
                    .align_offset(align_of::<u128>()),
                0
            );
        }

        // check serialize_parameters_unaligned

        let mut serialized = serialize_parameters(
            &bpf_loader_deprecated::id(),
            &program_id,
            &keyed_accounts,
            &instruction_data,
        )
        .unwrap();
        let (de_program_id, de_accounts, de_instruction_data) =
            unsafe { deserialize_unaligned(&mut serialized[0] as *mut u8) };

        assert_eq!(&program_id, de_program_id);
        assert_eq!(instruction_data, de_instruction_data);
        for ((account, account_info), key) in accounts.iter().zip(de_accounts).zip(keys) {
            assert_eq!(key, *account_info.key);
            let account = account.borrow();
            assert_eq!(account.lamports, account_info.lamports());
            assert_eq!(&account.data[..], &account_info.data.borrow()[..]);
            assert_eq!(&account.owner, account_info.owner);
            assert_eq!(account.executable, account_info.executable);
            assert_eq!(account.rent_epoch, account_info.rent_epoch);
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
            if dup_info == std::u8::MAX {
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
                    is_signer,
                    is_writable,
                    key,
                    lamports,
                    data,
                    owner,
                    executable,
                    rent_epoch,
                });
            } else {
                // duplicate account, clone the original
                accounts.push(accounts[dup_info as usize].clone());
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
