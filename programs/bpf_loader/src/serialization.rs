#![allow(clippy::arithmetic_side_effects)]

use {
    byteorder::{ByteOrder, LittleEndian},
    solana_program_runtime::invoke_context::SerializedAccountMetadata,
    solana_rbpf::{
        aligned_memory::{AlignedMemory, Pod},
        ebpf::{HOST_ALIGN, MM_INPUT_START},
        memory_region::{MemoryRegion, MemoryState},
    },
    solana_sdk::{
        bpf_loader_deprecated,
        entrypoint::{BPF_ALIGN_OF_U128, MAX_PERMITTED_DATA_INCREASE, NON_DUP_MARKER},
        instruction::InstructionError,
        pubkey::Pubkey,
        system_instruction::MAX_PERMITTED_DATA_LENGTH,
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    std::mem::{self, size_of},
};

/// Maximum number of instruction accounts that can be serialized into the
/// SBF VM.
const MAX_INSTRUCTION_ACCOUNTS: u8 = NON_DUP_MARKER;

enum SerializeAccount<'a> {
    Account(IndexOfAccount, BorrowedAccount<'a>),
    Duplicate(IndexOfAccount),
}

struct Serializer {
    pub buffer: AlignedMemory<HOST_ALIGN>,
    regions: Vec<MemoryRegion>,
    vaddr: u64,
    region_start: usize,
    aligned: bool,
    copy_account_data: bool,
}

impl Serializer {
    fn new(size: usize, start_addr: u64, aligned: bool, copy_account_data: bool) -> Serializer {
        Serializer {
            buffer: AlignedMemory::with_capacity(size),
            regions: Vec::new(),
            region_start: 0,
            vaddr: start_addr,
            aligned,
            copy_account_data,
        }
    }

    fn fill_write(&mut self, num: usize, value: u8) -> std::io::Result<()> {
        self.buffer.fill_write(num, value)
    }

    pub fn write<T: Pod>(&mut self, value: T) -> u64 {
        self.debug_assert_alignment::<T>();
        let vaddr = self
            .vaddr
            .saturating_add(self.buffer.len() as u64)
            .saturating_sub(self.region_start as u64);
        // Safety:
        // in serialize_parameters_(aligned|unaligned) first we compute the
        // required size then we write into the newly allocated buffer. There's
        // no need to check bounds at every write.
        //
        // AlignedMemory::write_unchecked _does_ debug_assert!() that the capacity
        // is enough, so in the unlikely case we introduce a bug in the size
        // computation, tests will abort.
        unsafe {
            self.buffer.write_unchecked(value);
        }

        vaddr
    }

    fn write_all(&mut self, value: &[u8]) -> u64 {
        let vaddr = self
            .vaddr
            .saturating_add(self.buffer.len() as u64)
            .saturating_sub(self.region_start as u64);
        // Safety:
        // see write() - the buffer is guaranteed to be large enough
        unsafe {
            self.buffer.write_all_unchecked(value);
        }

        vaddr
    }

    fn write_account(
        &mut self,
        account: &mut BorrowedAccount<'_>,
    ) -> Result<u64, InstructionError> {
        let vm_data_addr = if self.copy_account_data {
            let vm_data_addr = self.vaddr.saturating_add(self.buffer.len() as u64);
            self.write_all(account.get_data());
            vm_data_addr
        } else {
            self.push_region(true);
            let vaddr = self.vaddr;
            self.push_account_data_region(account)?;
            vaddr
        };

        if self.aligned {
            let align_offset =
                (account.get_data().len() as *const u8).align_offset(BPF_ALIGN_OF_U128);
            if self.copy_account_data {
                self.fill_write(MAX_PERMITTED_DATA_INCREASE + align_offset, 0)
                    .map_err(|_| InstructionError::InvalidArgument)?;
            } else {
                // The deserialization code is going to align the vm_addr to
                // BPF_ALIGN_OF_U128. Always add one BPF_ALIGN_OF_U128 worth of
                // padding and shift the start of the next region, so that once
                // vm_addr is aligned, the corresponding host_addr is aligned
                // too.
                self.fill_write(MAX_PERMITTED_DATA_INCREASE + BPF_ALIGN_OF_U128, 0)
                    .map_err(|_| InstructionError::InvalidArgument)?;
                self.region_start += BPF_ALIGN_OF_U128.saturating_sub(align_offset);
                // put the realloc padding in its own region
                self.push_region(account.can_data_be_changed().is_ok());
            }
        }

        Ok(vm_data_addr)
    }

    fn push_account_data_region(
        &mut self,
        account: &mut BorrowedAccount<'_>,
    ) -> Result<(), InstructionError> {
        if !account.get_data().is_empty() {
            let region = match account_data_region_memory_state(account) {
                MemoryState::Readable => MemoryRegion::new_readonly(account.get_data(), self.vaddr),
                MemoryState::Writable => {
                    MemoryRegion::new_writable(account.get_data_mut()?, self.vaddr)
                }
                MemoryState::Cow(index_in_transaction) => {
                    MemoryRegion::new_cow(account.get_data(), self.vaddr, index_in_transaction)
                }
            };
            self.vaddr += region.len;
            self.regions.push(region);
        }

        Ok(())
    }

    fn push_region(&mut self, writable: bool) {
        let range = self.region_start..self.buffer.len();
        let region = if writable {
            MemoryRegion::new_writable(
                self.buffer.as_slice_mut().get_mut(range.clone()).unwrap(),
                self.vaddr,
            )
        } else {
            MemoryRegion::new_readonly(
                self.buffer.as_slice().get(range.clone()).unwrap(),
                self.vaddr,
            )
        };
        self.regions.push(region);
        self.region_start = range.end;
        self.vaddr += range.len() as u64;
    }

    fn finish(mut self) -> (AlignedMemory<HOST_ALIGN>, Vec<MemoryRegion>) {
        self.push_region(true);
        debug_assert_eq!(self.region_start, self.buffer.len());
        (self.buffer, self.regions)
    }

    fn debug_assert_alignment<T>(&self) {
        debug_assert!(
            !self.aligned
                || self
                    .buffer
                    .as_slice()
                    .as_ptr_range()
                    .end
                    .align_offset(mem::align_of::<T>())
                    == 0
        );
    }
}

pub fn serialize_parameters(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    should_cap_ix_accounts: bool,
    copy_account_data: bool,
) -> Result<
    (
        AlignedMemory<HOST_ALIGN>,
        Vec<MemoryRegion>,
        Vec<SerializedAccountMetadata>,
    ),
    InstructionError,
> {
    let num_ix_accounts = instruction_context.get_number_of_instruction_accounts();
    if should_cap_ix_accounts && num_ix_accounts > MAX_INSTRUCTION_ACCOUNTS as IndexOfAccount {
        return Err(InstructionError::MaxAccountsExceeded);
    }

    let (program_id, is_loader_deprecated) = {
        let program_account =
            instruction_context.try_borrow_last_program_account(transaction_context)?;
        (
            *program_account.get_key(),
            *program_account.get_owner() == bpf_loader_deprecated::id(),
        )
    };

    let accounts = (0..instruction_context.get_number_of_instruction_accounts())
        .map(|instruction_account_index| {
            if let Some(index) = instruction_context
                .is_instruction_account_duplicate(instruction_account_index)
                .unwrap()
            {
                SerializeAccount::Duplicate(index)
            } else {
                let account = instruction_context
                    .try_borrow_instruction_account(transaction_context, instruction_account_index)
                    .unwrap();
                SerializeAccount::Account(instruction_account_index, account)
            }
        })
        // fun fact: jemalloc is good at caching tiny allocations like this one,
        // so collecting here is actually faster than passing the iterator
        // around, since the iterator does the work to produce its items each
        // time it's iterated on.
        .collect::<Vec<_>>();

    if is_loader_deprecated {
        serialize_parameters_unaligned(
            accounts,
            instruction_context.get_instruction_data(),
            &program_id,
            copy_account_data,
        )
    } else {
        serialize_parameters_aligned(
            accounts,
            instruction_context.get_instruction_data(),
            &program_id,
            copy_account_data,
        )
    }
}

pub fn deserialize_parameters(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    copy_account_data: bool,
    buffer: &[u8],
    accounts_metadata: &[SerializedAccountMetadata],
) -> Result<(), InstructionError> {
    let is_loader_deprecated = *instruction_context
        .try_borrow_last_program_account(transaction_context)?
        .get_owner()
        == bpf_loader_deprecated::id();
    let account_lengths = accounts_metadata.iter().map(|a| a.original_data_len);
    if is_loader_deprecated {
        deserialize_parameters_unaligned(
            transaction_context,
            instruction_context,
            copy_account_data,
            buffer,
            account_lengths,
        )
    } else {
        deserialize_parameters_aligned(
            transaction_context,
            instruction_context,
            copy_account_data,
            buffer,
            account_lengths,
        )
    }
}

fn serialize_parameters_unaligned(
    accounts: Vec<SerializeAccount>,
    instruction_data: &[u8],
    program_id: &Pubkey,
    copy_account_data: bool,
) -> Result<
    (
        AlignedMemory<HOST_ALIGN>,
        Vec<MemoryRegion>,
        Vec<SerializedAccountMetadata>,
    ),
    InstructionError,
> {
    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for account in &accounts {
        size += 1; // dup
        match account {
            SerializeAccount::Duplicate(_) => {}
            SerializeAccount::Account(_, account) => {
                size += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<Pubkey>() // key
                + size_of::<u64>()  // lamports
                + size_of::<u64>()  // data len
                + size_of::<Pubkey>() // owner
                + size_of::<u8>() // executable
                + size_of::<u64>(); // rent_epoch
                if copy_account_data {
                    size += account.get_data().len();
                }
            }
        }
    }
    size += size_of::<u64>() // instruction data len
         + instruction_data.len() // instruction data
         + size_of::<Pubkey>(); // program id

    let mut s = Serializer::new(size, MM_INPUT_START, false, copy_account_data);

    let mut accounts_metadata: Vec<SerializedAccountMetadata> = Vec::with_capacity(accounts.len());
    s.write::<u64>((accounts.len() as u64).to_le());
    for account in accounts {
        match account {
            SerializeAccount::Duplicate(position) => {
                accounts_metadata.push(accounts_metadata.get(position as usize).unwrap().clone());
                s.write(position as u8);
            }
            SerializeAccount::Account(_, mut account) => {
                s.write::<u8>(NON_DUP_MARKER);
                s.write::<u8>(account.is_signer() as u8);
                s.write::<u8>(account.is_writable() as u8);
                let vm_key_addr = s.write_all(account.get_key().as_ref());
                let vm_lamports_addr = s.write::<u64>(account.get_lamports().to_le());
                s.write::<u64>((account.get_data().len() as u64).to_le());
                let vm_data_addr = s.write_account(&mut account)?;
                let vm_owner_addr = s.write_all(account.get_owner().as_ref());
                s.write::<u8>(account.is_executable() as u8);
                s.write::<u64>((account.get_rent_epoch()).to_le());
                accounts_metadata.push(SerializedAccountMetadata {
                    original_data_len: account.get_data().len(),
                    vm_key_addr,
                    vm_lamports_addr,
                    vm_owner_addr,
                    vm_data_addr,
                });
            }
        };
    }
    s.write::<u64>((instruction_data.len() as u64).to_le());
    s.write_all(instruction_data);
    s.write_all(program_id.as_ref());

    let (mem, regions) = s.finish();
    Ok((mem, regions, accounts_metadata))
}

pub fn deserialize_parameters_unaligned<I: IntoIterator<Item = usize>>(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    copy_account_data: bool,
    buffer: &[u8],
    account_lengths: I,
) -> Result<(), InstructionError> {
    let mut start = size_of::<u64>(); // number of accounts
    for (instruction_account_index, pre_len) in (0..instruction_context
        .get_number_of_instruction_accounts())
        .zip(account_lengths.into_iter())
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
            if copy_account_data {
                let data = buffer
                    .get(start..start + pre_len)
                    .ok_or(InstructionError::InvalidArgument)?;
                // The redundant check helps to avoid the expensive data comparison if we can
                match borrowed_account
                    .can_data_be_resized(data.len())
                    .and_then(|_| borrowed_account.can_data_be_changed())
                {
                    Ok(()) => borrowed_account.set_data_from_slice(data)?,
                    Err(err) if borrowed_account.get_data() != data => return Err(err),
                    _ => {}
                }
                start += pre_len; // data
            }
            start += size_of::<Pubkey>() // owner
                + size_of::<u8>() // executable
                + size_of::<u64>(); // rent_epoch
        }
    }
    Ok(())
}

fn serialize_parameters_aligned(
    accounts: Vec<SerializeAccount>,
    instruction_data: &[u8],
    program_id: &Pubkey,
    copy_account_data: bool,
) -> Result<
    (
        AlignedMemory<HOST_ALIGN>,
        Vec<MemoryRegion>,
        Vec<SerializedAccountMetadata>,
    ),
    InstructionError,
> {
    let mut accounts_metadata = Vec::with_capacity(accounts.len());
    // Calculate size in order to alloc once
    let mut size = size_of::<u64>();
    for account in &accounts {
        size += 1; // dup
        match account {
            SerializeAccount::Duplicate(_) => size += 7, // padding to 64-bit aligned
            SerializeAccount::Account(_, account) => {
                let data_len = account.get_data().len();
                size += size_of::<u8>() // is_signer
                + size_of::<u8>() // is_writable
                + size_of::<u8>() // executable
                + size_of::<u32>() // original_data_len
                + size_of::<Pubkey>()  // key
                + size_of::<Pubkey>() // owner
                + size_of::<u64>()  // lamports
                + size_of::<u64>()  // data len
                + MAX_PERMITTED_DATA_INCREASE
                + size_of::<u64>(); // rent epoch
                if copy_account_data {
                    size += data_len + (data_len as *const u8).align_offset(BPF_ALIGN_OF_U128);
                } else {
                    size += BPF_ALIGN_OF_U128;
                }
            }
        }
    }
    size += size_of::<u64>() // data len
    + instruction_data.len()
    + size_of::<Pubkey>(); // program id;

    let mut s = Serializer::new(size, MM_INPUT_START, true, copy_account_data);

    // Serialize into the buffer
    s.write::<u64>((accounts.len() as u64).to_le());
    for account in accounts {
        match account {
            SerializeAccount::Account(_, mut borrowed_account) => {
                s.write::<u8>(NON_DUP_MARKER);
                s.write::<u8>(borrowed_account.is_signer() as u8);
                s.write::<u8>(borrowed_account.is_writable() as u8);
                s.write::<u8>(borrowed_account.is_executable() as u8);
                s.write_all(&[0u8, 0, 0, 0]);
                let vm_key_addr = s.write_all(borrowed_account.get_key().as_ref());
                let vm_owner_addr = s.write_all(borrowed_account.get_owner().as_ref());
                let vm_lamports_addr = s.write::<u64>(borrowed_account.get_lamports().to_le());
                s.write::<u64>((borrowed_account.get_data().len() as u64).to_le());
                let vm_data_addr = s.write_account(&mut borrowed_account)?;
                s.write::<u64>((borrowed_account.get_rent_epoch()).to_le());
                accounts_metadata.push(SerializedAccountMetadata {
                    original_data_len: borrowed_account.get_data().len(),
                    vm_key_addr,
                    vm_owner_addr,
                    vm_lamports_addr,
                    vm_data_addr,
                });
            }
            SerializeAccount::Duplicate(position) => {
                accounts_metadata.push(accounts_metadata.get(position as usize).unwrap().clone());
                s.write::<u8>(position as u8);
                s.write_all(&[0u8, 0, 0, 0, 0, 0, 0]);
            }
        };
    }
    s.write::<u64>((instruction_data.len() as u64).to_le());
    s.write_all(instruction_data);
    s.write_all(program_id.as_ref());

    let (mem, regions) = s.finish();
    Ok((mem, regions, accounts_metadata))
}

pub fn deserialize_parameters_aligned<I: IntoIterator<Item = usize>>(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    copy_account_data: bool,
    buffer: &[u8],
    account_lengths: I,
) -> Result<(), InstructionError> {
    let mut start = size_of::<u64>(); // number of accounts
    for (instruction_account_index, pre_len) in (0..instruction_context
        .get_number_of_instruction_accounts())
        .zip(account_lengths.into_iter())
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
            if post_len.saturating_sub(pre_len) > MAX_PERMITTED_DATA_INCREASE
                || post_len > MAX_PERMITTED_DATA_LENGTH as usize
            {
                return Err(InstructionError::InvalidRealloc);
            }
            // The redundant check helps to avoid the expensive data comparison if we can
            let alignment_offset = (pre_len as *const u8).align_offset(BPF_ALIGN_OF_U128);
            if copy_account_data {
                let data = buffer
                    .get(start..start + post_len)
                    .ok_or(InstructionError::InvalidArgument)?;
                match borrowed_account
                    .can_data_be_resized(post_len)
                    .and_then(|_| borrowed_account.can_data_be_changed())
                {
                    Ok(()) => borrowed_account.set_data_from_slice(data)?,
                    Err(err) if borrowed_account.get_data() != data => return Err(err),
                    _ => {}
                }
                start += pre_len; // data
            } else {
                // See Serializer::write_account() as to why we have this
                // padding before the realloc region here.
                start += BPF_ALIGN_OF_U128.saturating_sub(alignment_offset);
                let data = buffer
                    .get(start..start + MAX_PERMITTED_DATA_INCREASE)
                    .ok_or(InstructionError::InvalidArgument)?;
                match borrowed_account
                    .can_data_be_resized(post_len)
                    .and_then(|_| borrowed_account.can_data_be_changed())
                {
                    Ok(()) => {
                        borrowed_account.set_data_length(post_len)?;
                        let allocated_bytes = post_len.saturating_sub(pre_len);
                        if allocated_bytes > 0 {
                            borrowed_account
                                .get_data_mut()?
                                .get_mut(pre_len..pre_len.saturating_add(allocated_bytes))
                                .ok_or(InstructionError::InvalidArgument)?
                                .copy_from_slice(
                                    data.get(0..allocated_bytes)
                                        .ok_or(InstructionError::InvalidArgument)?,
                                );
                        }
                    }
                    Err(err) if borrowed_account.get_data().len() != post_len => return Err(err),
                    _ => {}
                }
            }
            start += MAX_PERMITTED_DATA_INCREASE;
            start += alignment_offset;
            start += size_of::<u64>(); // rent_epoch
            if borrowed_account.get_owner().to_bytes() != owner {
                // Change the owner at the end so that we are allowed to change the lamports and data before
                borrowed_account.set_owner(owner)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn account_data_region_memory_state(account: &BorrowedAccount<'_>) -> MemoryState {
    if account.can_data_be_changed().is_ok() {
        if account.is_shared() {
            MemoryState::Cow(account.get_index_in_transaction() as u64)
        } else {
            MemoryState::Writable
        }
    } else {
        MemoryState::Readable
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use {
        super::*,
        solana_program_runtime::with_mock_invoke_context,
        solana_sdk::{
            account::{Account, AccountSharedData, WritableAccount},
            account_info::AccountInfo,
            bpf_loader,
            entrypoint::deserialize,
            transaction_context::InstructionAccount,
        },
        std::{
            cell::RefCell,
            mem::transmute,
            rc::Rc,
            slice::{self, from_raw_parts, from_raw_parts_mut},
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

        for copy_account_data in [true] {
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
                for _ in 0..num_ix_accounts {
                    transaction_accounts.push((
                        Pubkey::new_unique(),
                        AccountSharedData::from(Account {
                            lamports: 0,
                            data: vec![],
                            owner: program_id,
                            executable: false,
                            rent_epoch: 0,
                        }),
                    ));
                }
                let mut instruction_accounts: Vec<_> = (0..num_ix_accounts as IndexOfAccount)
                    .map(|index_in_callee| InstructionAccount {
                        index_in_transaction: index_in_callee + 1,
                        index_in_caller: index_in_callee + 1,
                        index_in_callee,
                        is_signer: false,
                        is_writable: false,
                    })
                    .collect();
                if append_dup_account {
                    instruction_accounts.push(instruction_accounts.last().cloned().unwrap());
                }
                let program_indices = [0];
                let instruction_data = vec![];

                with_mock_invoke_context!(
                    invoke_context,
                    transaction_context,
                    transaction_accounts
                );
                invoke_context
                    .transaction_context
                    .get_next_instruction_context()
                    .unwrap()
                    .configure(&program_indices, &instruction_accounts, &instruction_data);
                invoke_context.push().unwrap();
                let instruction_context = invoke_context
                    .transaction_context
                    .get_current_instruction_context()
                    .unwrap();

                let serialization_result = serialize_parameters(
                    invoke_context.transaction_context,
                    instruction_context,
                    should_cap_ix_accounts,
                    copy_account_data,
                );
                assert_eq!(
                    serialization_result.as_ref().err(),
                    expected_err.as_ref(),
                    "{name} test case failed",
                );
                if expected_err.is_some() {
                    continue;
                }

                let (mut serialized, regions, _account_lengths) = serialization_result.unwrap();
                let mut serialized_regions = concat_regions(&regions);
                let (de_program_id, de_accounts, de_instruction_data) = unsafe {
                    deserialize(
                        if copy_account_data {
                            serialized.as_slice_mut()
                        } else {
                            serialized_regions.as_slice_mut()
                        }
                        .first_mut()
                        .unwrap() as *mut u8,
                    )
                };
                assert_eq!(de_program_id, &program_id);
                assert_eq!(de_instruction_data, &instruction_data);
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
            }
        }
    }

    #[test]
    fn test_serialize_parameters() {
        for copy_account_data in [false, true] {
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
            let instruction_accounts: Vec<InstructionAccount> = [1, 1, 2, 3, 4, 4, 5, 6]
                .into_iter()
                .enumerate()
                .map(
                    |(index_in_instruction, index_in_transaction)| InstructionAccount {
                        index_in_transaction,
                        index_in_caller: index_in_transaction,
                        index_in_callee: index_in_transaction - 1,
                        is_signer: false,
                        is_writable: index_in_instruction >= 4,
                    },
                )
                .collect();
            let instruction_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
            let program_indices = [0];
            let mut original_accounts = transaction_accounts.clone();
            with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
            invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(&program_indices, &instruction_accounts, &instruction_data);
            invoke_context.push().unwrap();
            let instruction_context = invoke_context
                .transaction_context
                .get_current_instruction_context()
                .unwrap();

            // check serialize_parameters_aligned
            let (mut serialized, regions, accounts_metadata) = serialize_parameters(
                invoke_context.transaction_context,
                instruction_context,
                true,
                copy_account_data,
            )
            .unwrap();

            let mut serialized_regions = concat_regions(&regions);
            if copy_account_data {
                assert_eq!(serialized.as_slice(), serialized_regions.as_slice());
            }
            let (de_program_id, de_accounts, de_instruction_data) = unsafe {
                deserialize(
                    if copy_account_data {
                        serialized.as_slice_mut()
                    } else {
                        serialized_regions.as_slice_mut()
                    }
                    .first_mut()
                    .unwrap() as *mut u8,
                )
            };

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
                copy_account_data,
                serialized.as_slice(),
                &accounts_metadata,
            )
            .unwrap();
            for (index_in_transaction, (_key, original_account)) in
                original_accounts.iter().enumerate()
            {
                let account = invoke_context
                    .transaction_context
                    .get_account_at_index(index_in_transaction as IndexOfAccount)
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

            let (mut serialized, regions, account_lengths) = serialize_parameters(
                invoke_context.transaction_context,
                instruction_context,
                true,
                copy_account_data,
            )
            .unwrap();
            let mut serialized_regions = concat_regions(&regions);

            let (de_program_id, de_accounts, de_instruction_data) = unsafe {
                deserialize_unaligned(
                    if copy_account_data {
                        serialized.as_slice_mut()
                    } else {
                        serialized_regions.as_slice_mut()
                    }
                    .first_mut()
                    .unwrap() as *mut u8,
                )
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
                copy_account_data,
                serialized.as_slice(),
                &account_lengths,
            )
            .unwrap();
            for (index_in_transaction, (_key, original_account)) in
                original_accounts.iter().enumerate()
            {
                let account = invoke_context
                    .transaction_context
                    .get_account_at_index(index_in_transaction as IndexOfAccount)
                    .unwrap()
                    .borrow();
                assert_eq!(&*account, original_account);
            }
        }
    }

    // the old bpf_loader in-program deserializer bpf_loader::id()
    #[deny(unsafe_op_in_unsafe_fn)]
    pub unsafe fn deserialize_unaligned<'a>(
        input: *mut u8,
    ) -> (&'a Pubkey, Vec<AccountInfo<'a>>, &'a [u8]) {
        // this boring boilerplate struct is needed until inline const...
        struct Ptr<T>(std::marker::PhantomData<T>);
        impl<T> Ptr<T> {
            const COULD_BE_UNALIGNED: bool = std::mem::align_of::<T>() > 1;

            #[inline(always)]
            fn read_possibly_unaligned(input: *mut u8, offset: usize) -> T {
                unsafe {
                    let src = input.add(offset) as *const T;
                    if Self::COULD_BE_UNALIGNED {
                        src.read_unaligned()
                    } else {
                        src.read()
                    }
                }
            }

            // rustc inserts debug_assert! for misaligned pointer dereferences when
            // deserializing, starting from [1]. so, use std::mem::transmute as the last resort
            // while preventing clippy from complaining to suggest not to use it.
            // [1]: https://github.com/rust-lang/rust/commit/22a7a19f9333bc1fcba97ce444a3515cb5fb33e6
            // as for the ub nature of the misaligned pointer dereference, this is
            // acceptable in this code, given that this is cfg(test) and it's cared only with
            // x86-64 and the target only incurs some performance penalty, not like segfaults
            // in other targets.
            #[inline(always)]
            fn ref_possibly_unaligned<'a>(input: *mut u8, offset: usize) -> &'a T {
                #[allow(clippy::transmute_ptr_to_ref)]
                unsafe {
                    transmute(input.add(offset) as *const T)
                }
            }

            // See ref_possibly_unaligned's comment
            #[inline(always)]
            fn mut_possibly_unaligned<'a>(input: *mut u8, offset: usize) -> &'a mut T {
                #[allow(clippy::transmute_ptr_to_ref)]
                unsafe {
                    transmute(input.add(offset) as *mut T)
                }
            }
        }

        let mut offset: usize = 0;

        // number of accounts present

        let num_accounts = Ptr::<u64>::read_possibly_unaligned(input, offset) as usize;
        offset += size_of::<u64>();

        // account Infos

        let mut accounts = Vec::with_capacity(num_accounts);
        for _ in 0..num_accounts {
            let dup_info = Ptr::<u8>::read_possibly_unaligned(input, offset);
            offset += size_of::<u8>();
            if dup_info == NON_DUP_MARKER {
                let is_signer = Ptr::<u8>::read_possibly_unaligned(input, offset) != 0;
                offset += size_of::<u8>();

                let is_writable = Ptr::<u8>::read_possibly_unaligned(input, offset) != 0;
                offset += size_of::<u8>();

                let key = Ptr::<Pubkey>::ref_possibly_unaligned(input, offset);
                offset += size_of::<Pubkey>();

                let lamports = Rc::new(RefCell::new(Ptr::mut_possibly_unaligned(input, offset)));
                offset += size_of::<u64>();

                let data_len = Ptr::<u64>::read_possibly_unaligned(input, offset) as usize;
                offset += size_of::<u64>();

                let data = Rc::new(RefCell::new(unsafe {
                    from_raw_parts_mut(input.add(offset), data_len)
                }));
                offset += data_len;

                let owner: &Pubkey = Ptr::<Pubkey>::ref_possibly_unaligned(input, offset);
                offset += size_of::<Pubkey>();

                let executable = Ptr::<u8>::read_possibly_unaligned(input, offset) != 0;
                offset += size_of::<u8>();

                let rent_epoch = Ptr::<u64>::read_possibly_unaligned(input, offset);
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

        let instruction_data_len = Ptr::<u64>::read_possibly_unaligned(input, offset) as usize;
        offset += size_of::<u64>();

        let instruction_data = unsafe { from_raw_parts(input.add(offset), instruction_data_len) };
        offset += instruction_data_len;

        // program Id

        let program_id = Ptr::<Pubkey>::ref_possibly_unaligned(input, offset);

        (program_id, accounts, instruction_data)
    }

    fn concat_regions(regions: &[MemoryRegion]) -> AlignedMemory<HOST_ALIGN> {
        let len = regions.iter().fold(0, |len, region| len + region.len) as usize;
        let mut mem = AlignedMemory::zero_filled(len);
        for region in regions {
            let host_slice = unsafe {
                slice::from_raw_parts(region.host_addr.get() as *const u8, region.len as usize)
            };
            mem.as_slice_mut()[(region.vm_addr - MM_INPUT_START) as usize..][..region.len as usize]
                .copy_from_slice(host_slice)
        }
        mem
    }
}
