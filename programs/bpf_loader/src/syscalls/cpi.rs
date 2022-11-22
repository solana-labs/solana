use {
    super::*,
    crate::declare_syscall,
    solana_sdk::{
        feature_set::enable_bpf_loader_set_authority_checked_ix,
        syscalls::{
            MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_ACCOUNTS, MAX_CPI_INSTRUCTION_DATA_LEN,
        },
        transaction_context::BorrowedAccount,
    },
    std::mem,
};

/// Host side representation of AccountInfo or SolAccountInfo passed to the CPI syscall.
///
/// At the start of a CPI, this can be different from the data stored in the
/// corresponding BorrowedAccount, and needs to be synched.
struct CallerAccount<'a> {
    lamports: &'a mut u64,
    owner: &'a mut Pubkey,
    original_data_len: usize,
    data: &'a mut [u8],
    // Given the corresponding input AccountInfo::data, vm_data_addr points to
    // the pointer field and ref_to_len_in_vm points to the length field.
    vm_data_addr: u64,
    ref_to_len_in_vm: &'a mut u64,
    executable: bool,
    rent_epoch: u64,
}

impl<'a> CallerAccount<'a> {
    // Create a CallerAccount given an AccountInfo.
    fn from_account_info(
        invoke_context: &InvokeContext,
        memory_mapping: &MemoryMapping,
        _vm_addr: u64,
        account_info: &AccountInfo,
        original_data_len: usize,
    ) -> Result<CallerAccount<'a>, EbpfError> {
        // account_info points to host memory. The addresses used internally are
        // in vm space so they need to be translated.

        let lamports = {
            // Double translate lamports out of RefCell
            let ptr = translate_type::<u64>(
                memory_mapping,
                account_info.lamports.as_ptr() as u64,
                invoke_context.get_check_aligned(),
            )?;
            translate_type_mut::<u64>(memory_mapping, *ptr, invoke_context.get_check_aligned())?
        };
        let owner = translate_type_mut::<Pubkey>(
            memory_mapping,
            account_info.owner as *const _ as u64,
            invoke_context.get_check_aligned(),
        )?;

        let (data, vm_data_addr, ref_to_len_in_vm) = {
            // Double translate data out of RefCell
            let data = *translate_type::<&[u8]>(
                memory_mapping,
                account_info.data.as_ptr() as *const _ as u64,
                invoke_context.get_check_aligned(),
            )?;

            consume_compute_meter(
                invoke_context,
                (data.len() as u64)
                    .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
            )?;

            let translated = translate(
                memory_mapping,
                AccessType::Store,
                (account_info.data.as_ptr() as *const u64 as u64)
                    .saturating_add(size_of::<u64>() as u64),
                8,
            )? as *mut u64;
            let ref_to_len_in_vm = unsafe { &mut *translated };
            let vm_data_addr = data.as_ptr() as u64;
            (
                translate_slice_mut::<u8>(
                    memory_mapping,
                    vm_data_addr,
                    data.len() as u64,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?,
                vm_data_addr,
                ref_to_len_in_vm,
            )
        };

        Ok(CallerAccount {
            lamports,
            owner,
            original_data_len,
            data,
            vm_data_addr,
            ref_to_len_in_vm,
            executable: account_info.executable,
            rent_epoch: account_info.rent_epoch,
        })
    }

    // Create a CallerAccount given a SolAccountInfo.
    fn from_sol_account_info(
        invoke_context: &InvokeContext,
        memory_mapping: &MemoryMapping,
        vm_addr: u64,
        account_info: &SolAccountInfo,
        original_data_len: usize,
    ) -> Result<CallerAccount<'a>, EbpfError> {
        // account_info points to host memory. The addresses used internally are
        // in vm space so they need to be translated.

        let lamports = translate_type_mut::<u64>(
            memory_mapping,
            account_info.lamports_addr,
            invoke_context.get_check_aligned(),
        )?;
        let owner = translate_type_mut::<Pubkey>(
            memory_mapping,
            account_info.owner_addr,
            invoke_context.get_check_aligned(),
        )?;
        let vm_data_addr = account_info.data_addr;

        consume_compute_meter(
            invoke_context,
            account_info
                .data_len
                .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
        )?;

        let data = translate_slice_mut::<u8>(
            memory_mapping,
            vm_data_addr,
            account_info.data_len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        // we already have the host addr we want: &mut account_info.data_len.
        // The account info might be read only in the vm though, so we translate
        // to ensure we can write. This is tested by programs/sbf/rust/ro_modify
        // which puts SolAccountInfo in rodata.
        let data_len_vm_addr = vm_addr
            .saturating_add(&account_info.data_len as *const u64 as u64)
            .saturating_sub(account_info as *const _ as *const u64 as u64);
        let data_len_addr = translate(
            memory_mapping,
            AccessType::Store,
            data_len_vm_addr,
            size_of::<u64>() as u64,
        )?;
        let ref_to_len_in_vm = unsafe { &mut *(data_len_addr as *mut u64) };

        Ok(CallerAccount {
            lamports,
            owner,
            original_data_len,
            data,
            vm_data_addr,
            ref_to_len_in_vm,
            executable: account_info.executable,
            rent_epoch: account_info.rent_epoch,
        })
    }
}

type TranslatedAccounts<'a> = Vec<(IndexOfAccount, Option<CallerAccount<'a>>)>;

/// Implemented by language specific data structure translators
trait SyscallInvokeSigned {
    fn translate_instruction(
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError>;
    fn translate_accounts<'a>(
        instruction_accounts: &[InstructionAccount],
        program_indices: &[IndexOfAccount],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'a>, EbpfError>;
    fn translate_signers(
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError>;
}

declare_syscall!(
    /// Cross-program invocation called from Rust
    SyscallInvokeSignedRust,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        cpi_common::<Self>(
            invoke_context,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        )
    }
);

impl SyscallInvokeSigned for SyscallInvokeSignedRust {
    fn translate_instruction(
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError> {
        let ix = translate_type::<Instruction>(
            memory_mapping,
            addr,
            invoke_context.get_check_aligned(),
        )?;

        check_instruction_size(ix.accounts.len(), ix.data.len(), invoke_context)?;

        let accounts = translate_slice::<AccountMeta>(
            memory_mapping,
            ix.accounts.as_ptr() as u64,
            ix.accounts.len() as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?
        .to_vec();

        let ix_data_len = ix.data.len() as u64;
        if invoke_context
            .feature_set
            .is_active(&feature_set::loosen_cpi_size_restriction::id())
        {
            consume_compute_meter(
                invoke_context,
                (ix_data_len)
                    .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
            )?;
        }

        let data = translate_slice::<u8>(
            memory_mapping,
            ix.data.as_ptr() as u64,
            ix_data_len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?
        .to_vec();
        Ok(Instruction {
            program_id: ix.program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts<'a>(
        instruction_accounts: &[InstructionAccount],
        program_indices: &[IndexOfAccount],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'a>, EbpfError> {
        let (account_infos, account_info_keys) = translate_account_infos(
            account_infos_addr,
            account_infos_len,
            |account_info: &AccountInfo| account_info.key as *const _ as u64,
            memory_mapping,
            invoke_context,
        )?;

        translate_and_update_accounts(
            instruction_accounts,
            program_indices,
            &account_info_keys,
            account_infos,
            account_infos_addr,
            invoke_context,
            memory_mapping,
            CallerAccount::from_account_info,
        )
    }

    fn translate_signers(
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError> {
        let mut signers = Vec::new();
        if signers_seeds_len > 0 {
            let signers_seeds = translate_slice::<&[&[u8]]>(
                memory_mapping,
                signers_seeds_addr,
                signers_seeds_len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            if signers_seeds.len() > MAX_SIGNERS {
                return Err(SyscallError::TooManySigners.into());
            }
            for signer_seeds in signers_seeds.iter() {
                let untranslated_seeds = translate_slice::<&[u8]>(
                    memory_mapping,
                    signer_seeds.as_ptr() as *const _ as u64,
                    signer_seeds.len() as u64,
                    invoke_context.get_check_aligned(),
                    invoke_context.get_check_size(),
                )?;
                if untranslated_seeds.len() > MAX_SEEDS {
                    return Err(SyscallError::InstructionError(
                        InstructionError::MaxSeedLengthExceeded,
                    )
                    .into());
                }
                let seeds = untranslated_seeds
                    .iter()
                    .map(|untranslated_seed| {
                        translate_slice::<u8>(
                            memory_mapping,
                            untranslated_seed.as_ptr() as *const _ as u64,
                            untranslated_seed.len() as u64,
                            invoke_context.get_check_aligned(),
                            invoke_context.get_check_size(),
                        )
                    })
                    .collect::<Result<Vec<_>, EbpfError>>()?;
                let signer = Pubkey::create_program_address(&seeds, program_id)
                    .map_err(SyscallError::BadSeeds)?;
                signers.push(signer);
            }
            Ok(signers)
        } else {
            Ok(vec![])
        }
    }
}

/// Rust representation of C's SolInstruction
#[derive(Debug)]
#[repr(C)]
struct SolInstruction {
    program_id_addr: u64,
    accounts_addr: u64,
    accounts_len: u64,
    data_addr: u64,
    data_len: u64,
}

/// Rust representation of C's SolAccountMeta
#[derive(Debug)]
#[repr(C)]
struct SolAccountMeta {
    pubkey_addr: u64,
    is_writable: bool,
    is_signer: bool,
}

/// Rust representation of C's SolAccountInfo
#[derive(Debug)]
#[repr(C)]
struct SolAccountInfo {
    key_addr: u64,
    lamports_addr: u64,
    data_len: u64,
    data_addr: u64,
    owner_addr: u64,
    rent_epoch: u64,
    #[allow(dead_code)]
    is_signer: bool,
    #[allow(dead_code)]
    is_writable: bool,
    executable: bool,
}

/// Rust representation of C's SolSignerSeed
#[derive(Debug)]
#[repr(C)]
struct SolSignerSeedC {
    addr: u64,
    len: u64,
}

/// Rust representation of C's SolSignerSeeds
#[derive(Debug)]
#[repr(C)]
struct SolSignerSeedsC {
    addr: u64,
    len: u64,
}

declare_syscall!(
    /// Cross-program invocation called from C
    SyscallInvokeSignedC,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, EbpfError> {
        cpi_common::<Self>(
            invoke_context,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        )
    }
);

impl SyscallInvokeSigned for SyscallInvokeSignedC {
    fn translate_instruction(
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError> {
        let ix_c = translate_type::<SolInstruction>(
            memory_mapping,
            addr,
            invoke_context.get_check_aligned(),
        )?;

        check_instruction_size(
            ix_c.accounts_len as usize,
            ix_c.data_len as usize,
            invoke_context,
        )?;
        let program_id = translate_type::<Pubkey>(
            memory_mapping,
            ix_c.program_id_addr,
            invoke_context.get_check_aligned(),
        )?;
        let meta_cs = translate_slice::<SolAccountMeta>(
            memory_mapping,
            ix_c.accounts_addr,
            ix_c.accounts_len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let ix_data_len = ix_c.data_len;
        if invoke_context
            .feature_set
            .is_active(&feature_set::loosen_cpi_size_restriction::id())
        {
            consume_compute_meter(
                invoke_context,
                (ix_data_len)
                    .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
            )?;
        }

        let data = translate_slice::<u8>(
            memory_mapping,
            ix_c.data_addr,
            ix_data_len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?
        .to_vec();
        let accounts = meta_cs
            .iter()
            .map(|meta_c| {
                let pubkey = translate_type::<Pubkey>(
                    memory_mapping,
                    meta_c.pubkey_addr,
                    invoke_context.get_check_aligned(),
                )?;
                Ok(AccountMeta {
                    pubkey: *pubkey,
                    is_signer: meta_c.is_signer,
                    is_writable: meta_c.is_writable,
                })
            })
            .collect::<Result<Vec<AccountMeta>, EbpfError>>()?;

        Ok(Instruction {
            program_id: *program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts<'a>(
        instruction_accounts: &[InstructionAccount],
        program_indices: &[IndexOfAccount],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'a>, EbpfError> {
        let (account_infos, account_info_keys) = translate_account_infos(
            account_infos_addr,
            account_infos_len,
            |account_info: &SolAccountInfo| account_info.key_addr,
            memory_mapping,
            invoke_context,
        )?;

        translate_and_update_accounts(
            instruction_accounts,
            program_indices,
            &account_info_keys,
            account_infos,
            account_infos_addr,
            invoke_context,
            memory_mapping,
            CallerAccount::from_sol_account_info,
        )
    }

    fn translate_signers(
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError> {
        if signers_seeds_len > 0 {
            let signers_seeds = translate_slice::<SolSignerSeedsC>(
                memory_mapping,
                signers_seeds_addr,
                signers_seeds_len,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            if signers_seeds.len() > MAX_SIGNERS {
                return Err(SyscallError::TooManySigners.into());
            }
            Ok(signers_seeds
                .iter()
                .map(|signer_seeds| {
                    let seeds = translate_slice::<SolSignerSeedC>(
                        memory_mapping,
                        signer_seeds.addr,
                        signer_seeds.len,
                        invoke_context.get_check_aligned(),
                        invoke_context.get_check_size(),
                    )?;
                    if seeds.len() > MAX_SEEDS {
                        return Err(SyscallError::InstructionError(
                            InstructionError::MaxSeedLengthExceeded,
                        )
                        .into());
                    }
                    let seeds_bytes = seeds
                        .iter()
                        .map(|seed| {
                            translate_slice::<u8>(
                                memory_mapping,
                                seed.addr,
                                seed.len,
                                invoke_context.get_check_aligned(),
                                invoke_context.get_check_size(),
                            )
                        })
                        .collect::<Result<Vec<_>, EbpfError>>()?;
                    Pubkey::create_program_address(&seeds_bytes, program_id)
                        .map_err(|err| SyscallError::BadSeeds(err).into())
                })
                .collect::<Result<Vec<_>, EbpfError>>()?)
        } else {
            Ok(vec![])
        }
    }
}

fn translate_account_infos<'a, T, F>(
    account_infos_addr: u64,
    account_infos_len: u64,
    key_addr: F,
    memory_mapping: &mut MemoryMapping,
    invoke_context: &mut InvokeContext,
) -> Result<(&'a [T], Vec<&'a Pubkey>), EbpfError>
where
    F: Fn(&T) -> u64,
{
    let account_infos = translate_slice::<T>(
        memory_mapping,
        account_infos_addr,
        account_infos_len,
        invoke_context.get_check_aligned(),
        invoke_context.get_check_size(),
    )?;
    check_account_infos(account_infos.len(), invoke_context)?;
    let account_info_keys = account_infos
        .iter()
        .map(|account_info| {
            translate_type::<Pubkey>(
                memory_mapping,
                key_addr(account_info),
                invoke_context.get_check_aligned(),
            )
        })
        .collect::<Result<Vec<_>, EbpfError>>()?;

    Ok((account_infos, account_info_keys))
}

// Finish translating accounts, build CallerAccount values and update callee
// accounts in preparation of executing the callee.
fn translate_and_update_accounts<'a, T, F>(
    instruction_accounts: &[InstructionAccount],
    program_indices: &[IndexOfAccount],
    account_info_keys: &[&Pubkey],
    account_infos: &[T],
    account_infos_addr: u64,
    invoke_context: &mut InvokeContext,
    memory_mapping: &MemoryMapping,
    do_translate: F,
) -> Result<TranslatedAccounts<'a>, EbpfError>
where
    F: Fn(&InvokeContext, &MemoryMapping, u64, &T, usize) -> Result<CallerAccount<'a>, EbpfError>,
{
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;
    let mut accounts = Vec::with_capacity(instruction_accounts.len().saturating_add(1));

    let program_account_index = program_indices
        .last()
        .ok_or(SyscallError::InstructionError(
            InstructionError::MissingAccount,
        ))?;
    accounts.push((*program_account_index, None));

    // unwrapping here is fine: we're in a syscall and the method below fails
    // only outside syscalls
    let orig_data_lens = invoke_context.get_orig_account_lengths().unwrap();

    for (instruction_account_index, instruction_account) in instruction_accounts.iter().enumerate()
    {
        if instruction_account_index as IndexOfAccount != instruction_account.index_in_callee {
            continue; // Skip duplicate account
        }

        let callee_account = instruction_context
            .try_borrow_instruction_account(
                transaction_context,
                instruction_account.index_in_caller,
            )
            .map_err(SyscallError::InstructionError)?;
        let account_key = invoke_context
            .transaction_context
            .get_key_of_account_at_index(instruction_account.index_in_transaction)
            .map_err(SyscallError::InstructionError)?;

        if callee_account.is_executable() {
            // Use the known account
            consume_compute_meter(
                invoke_context,
                (callee_account.get_data().len() as u64)
                    .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
            )?;

            accounts.push((instruction_account.index_in_caller, None));
        } else if let Some(caller_account_index) =
            account_info_keys.iter().position(|key| *key == account_key)
        {
            let original_data_len = *orig_data_lens
                .get(instruction_account.index_in_caller as usize)
                .ok_or_else(|| {
                    ic_msg!(
                        invoke_context,
                        "Internal error: index mismatch for account {}",
                        account_key
                    );
                    SyscallError::InstructionError(InstructionError::MissingAccount)
                })?;

            // build the CallerAccount corresponding to this account.
            let caller_account =
                do_translate(
                    invoke_context,
                    memory_mapping,
                    account_infos_addr.saturating_add(
                        caller_account_index.saturating_mul(mem::size_of::<T>()) as u64,
                    ),
                    account_infos
                        .get(caller_account_index)
                        .ok_or(SyscallError::InvalidLength)?,
                    original_data_len,
                )?;

            // before initiating CPI, the caller may have modified the
            // account (caller_account). We need to update the corresponding
            // BorrowedAccount (callee_account) so the callee can see the
            // changes.
            update_callee_account(invoke_context, &caller_account, callee_account)?;

            let caller_account = if instruction_account.is_writable {
                Some(caller_account)
            } else {
                None
            };
            accounts.push((instruction_account.index_in_caller, caller_account));
        } else {
            ic_msg!(
                invoke_context,
                "Instruction references an unknown account {}",
                account_key
            );
            return Err(SyscallError::InstructionError(InstructionError::MissingAccount).into());
        }
    }

    Ok(accounts)
}

fn check_instruction_size(
    num_accounts: usize,
    data_len: usize,
    invoke_context: &mut InvokeContext,
) -> Result<(), EbpfError> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::loosen_cpi_size_restriction::id())
    {
        let data_len = data_len as u64;
        let max_data_len = MAX_CPI_INSTRUCTION_DATA_LEN;
        if data_len > max_data_len {
            return Err(SyscallError::MaxInstructionDataLenExceeded {
                data_len,
                max_data_len,
            }
            .into());
        }

        let num_accounts = num_accounts as u64;
        let max_accounts = MAX_CPI_INSTRUCTION_ACCOUNTS as u64;
        if num_accounts > max_accounts {
            return Err(SyscallError::MaxInstructionAccountsExceeded {
                num_accounts,
                max_accounts,
            }
            .into());
        }
    } else {
        let max_size = invoke_context.get_compute_budget().max_cpi_instruction_size;
        let size = num_accounts
            .saturating_mul(size_of::<AccountMeta>())
            .saturating_add(data_len);
        if size > max_size {
            return Err(SyscallError::InstructionTooLarge(size, max_size).into());
        }
    }
    Ok(())
}

fn check_account_infos(
    num_account_infos: usize,
    invoke_context: &mut InvokeContext,
) -> Result<(), EbpfError> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::loosen_cpi_size_restriction::id())
    {
        let max_cpi_account_infos = if invoke_context
            .feature_set
            .is_active(&feature_set::increase_tx_account_lock_limit::id())
        {
            MAX_CPI_ACCOUNT_INFOS
        } else {
            64
        };
        let num_account_infos = num_account_infos as u64;
        let max_account_infos = max_cpi_account_infos as u64;
        if num_account_infos > max_account_infos {
            return Err(SyscallError::MaxInstructionAccountInfosExceeded {
                num_account_infos,
                max_account_infos,
            }
            .into());
        }
    } else {
        let adjusted_len = num_account_infos.saturating_mul(size_of::<Pubkey>());

        if adjusted_len > invoke_context.get_compute_budget().max_cpi_instruction_size {
            // Cap the number of account_infos a caller can pass to approximate
            // maximum that accounts that could be passed in an instruction
            return Err(SyscallError::TooManyAccounts.into());
        };
    }
    Ok(())
}

fn check_authorized_program(
    program_id: &Pubkey,
    instruction_data: &[u8],
    invoke_context: &InvokeContext,
) -> Result<(), EbpfError> {
    if native_loader::check_id(program_id)
        || bpf_loader::check_id(program_id)
        || bpf_loader_deprecated::check_id(program_id)
        || (bpf_loader_upgradeable::check_id(program_id)
            && !(bpf_loader_upgradeable::is_upgrade_instruction(instruction_data)
                || bpf_loader_upgradeable::is_set_authority_instruction(instruction_data)
                || (invoke_context
                    .feature_set
                    .is_active(&enable_bpf_loader_set_authority_checked_ix::id())
                    && bpf_loader_upgradeable::is_set_authority_checked_instruction(
                        instruction_data,
                    ))
                || bpf_loader_upgradeable::is_close_instruction(instruction_data)))
        || is_precompile(program_id, |feature_id: &Pubkey| {
            invoke_context.feature_set.is_active(feature_id)
        })
    {
        return Err(SyscallError::ProgramNotSupported(*program_id).into());
    }
    Ok(())
}

/// Call process instruction, common to both Rust and C
fn cpi_common<S: SyscallInvokeSigned>(
    invoke_context: &mut InvokeContext,
    instruction_addr: u64,
    account_infos_addr: u64,
    account_infos_len: u64,
    signers_seeds_addr: u64,
    signers_seeds_len: u64,
    memory_mapping: &mut MemoryMapping,
) -> Result<u64, EbpfError> {
    // CPI entry.
    //
    // Translate the inputs to the syscall and synchronize the caller's account
    // changes so the callee can see them.
    consume_compute_meter(
        invoke_context,
        invoke_context.get_compute_budget().invoke_units,
    )?;

    let instruction = S::translate_instruction(instruction_addr, memory_mapping, invoke_context)?;
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;
    let caller_program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(SyscallError::InstructionError)?;
    let signers = S::translate_signers(
        caller_program_id,
        signers_seeds_addr,
        signers_seeds_len,
        memory_mapping,
        invoke_context,
    )?;
    let (instruction_accounts, program_indices) = invoke_context
        .prepare_instruction(&instruction, &signers)
        .map_err(SyscallError::InstructionError)?;
    check_authorized_program(&instruction.program_id, &instruction.data, invoke_context)?;
    let mut accounts = S::translate_accounts(
        &instruction_accounts,
        &program_indices,
        account_infos_addr,
        account_infos_len,
        memory_mapping,
        invoke_context,
    )?;

    // Process the callee instruction
    let mut compute_units_consumed = 0;
    invoke_context
        .process_instruction(
            &instruction.data,
            &instruction_accounts,
            &program_indices,
            &mut compute_units_consumed,
            &mut ExecuteTimings::default(),
        )
        .map_err(SyscallError::InstructionError)?;

    // re-bind to please the borrow checker
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;

    // CPI exit.
    //
    // Synchronize the callee's account changes so the caller can see them.
    for (index_in_caller, caller_account) in accounts.iter_mut() {
        if let Some(caller_account) = caller_account {
            let callee_account = instruction_context
                .try_borrow_instruction_account(transaction_context, *index_in_caller)
                .map_err(SyscallError::InstructionError)?;
            update_caller_account(
                invoke_context,
                memory_mapping,
                caller_account,
                &callee_account,
            )?;
        }
    }

    Ok(SUCCESS)
}

// Update the given account before executing CPI.
//
// caller_account and callee_account describe the same account. At CPI entry
// caller_account might include changes the caller has made to the account
// before executing CPI.
//
// This method updates callee_account so the CPI callee can see the caller's
// changes.
fn update_callee_account(
    invoke_context: &InvokeContext,
    caller_account: &CallerAccount,
    mut callee_account: BorrowedAccount<'_>,
) -> Result<(), EbpfError> {
    let is_disable_cpi_setting_executable_and_rent_epoch_active = invoke_context
        .feature_set
        .is_active(&disable_cpi_setting_executable_and_rent_epoch::id());

    if callee_account.get_lamports() != *caller_account.lamports {
        callee_account
            .set_lamports(*caller_account.lamports)
            .map_err(SyscallError::InstructionError)?;
    }

    // The redundant check helps to avoid the expensive data comparison if we can
    match callee_account
        .can_data_be_resized(caller_account.data.len())
        .and_then(|_| callee_account.can_data_be_changed())
    {
        Ok(()) => callee_account
            .set_data_from_slice(caller_account.data)
            .map_err(SyscallError::InstructionError)?,
        Err(err) if callee_account.get_data() != caller_account.data => {
            return Err(EbpfError::UserError(Box::new(BpfError::SyscallError(
                SyscallError::InstructionError(err),
            ))));
        }
        _ => {}
    }

    if !is_disable_cpi_setting_executable_and_rent_epoch_active
        && callee_account.is_executable() != caller_account.executable
    {
        callee_account
            .set_executable(caller_account.executable)
            .map_err(SyscallError::InstructionError)?;
    }

    // Change the owner at the end so that we are allowed to change the lamports and data before
    if callee_account.get_owner() != caller_account.owner {
        callee_account
            .set_owner(caller_account.owner.as_ref())
            .map_err(SyscallError::InstructionError)?;
    }

    // BorrowedAccount doesn't allow changing the rent epoch. Drop it and use
    // AccountSharedData directly.
    let index_in_transaction = callee_account.get_index_in_transaction();
    drop(callee_account);
    let callee_account = invoke_context
        .transaction_context
        .get_account_at_index(index_in_transaction)
        .map_err(SyscallError::InstructionError)?;
    if !is_disable_cpi_setting_executable_and_rent_epoch_active
        && callee_account.borrow().rent_epoch() != caller_account.rent_epoch
    {
        if invoke_context
            .feature_set
            .is_active(&enable_early_verification_of_account_modifications::id())
        {
            return Err(SyscallError::InstructionError(InstructionError::RentEpochModified).into());
        } else {
            callee_account
                .borrow_mut()
                .set_rent_epoch(caller_account.rent_epoch);
        }
    }

    Ok(())
}

// Update the given account after executing CPI.
//
// caller_account and callee_account describe to the same account. At CPI exit
// callee_account might include changes the callee has made to the account
// after executing.
//
// This method updates caller_account so the CPI caller can see the callee's
// changes.
fn update_caller_account(
    invoke_context: &InvokeContext,
    memory_mapping: &MemoryMapping,
    caller_account: &mut CallerAccount,
    callee_account: &BorrowedAccount<'_>,
) -> Result<(), EbpfError> {
    *caller_account.lamports = callee_account.get_lamports();
    *caller_account.owner = *callee_account.get_owner();
    let new_len = callee_account.get_data().len();
    if caller_account.data.len() != new_len {
        let data_overflow = new_len
            > caller_account
                .original_data_len
                .saturating_add(MAX_PERMITTED_DATA_INCREASE);
        if data_overflow {
            ic_msg!(
                invoke_context,
                "Account data size realloc limited to {} in inner instructions",
                MAX_PERMITTED_DATA_INCREASE
            );
            return Err(SyscallError::InstructionError(InstructionError::InvalidRealloc).into());
        }
        if new_len < caller_account.data.len() {
            caller_account
                .data
                .get_mut(new_len..)
                .ok_or(SyscallError::InstructionError(
                    InstructionError::AccountDataTooSmall,
                ))?
                .fill(0);
        }
        caller_account.data = translate_slice_mut::<u8>(
            memory_mapping,
            caller_account.vm_data_addr,
            new_len as u64,
            false, // Don't care since it is byte aligned
            invoke_context.get_check_size(),
        )?;
        // this is the len field in the AccountInfo::data slice
        *caller_account.ref_to_len_in_vm = new_len as u64;

        // this is the len field in the serialized parameters
        let serialized_len_ptr = translate_type_mut::<u64>(
            memory_mapping,
            caller_account
                .vm_data_addr
                .saturating_sub(std::mem::size_of::<u64>() as u64),
            invoke_context.get_check_aligned(),
        )?;
        *serialized_len_ptr = new_len as u64;
    }
    let to_slice = &mut caller_account.data;
    let from_slice = callee_account
        .get_data()
        .get(0..new_len)
        .ok_or(SyscallError::InvalidLength)?;
    if to_slice.len() != from_slice.len() {
        return Err(SyscallError::InstructionError(InstructionError::AccountDataTooSmall).into());
    }
    to_slice.copy_from_slice(from_slice);

    Ok(())
}

#[allow(clippy::indexing_slicing)]
#[allow(clippy::integer_arithmetic)]
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::allocator_bump::BpfAllocator,
        solana_rbpf::{
            aligned_memory::AlignedMemory, ebpf::MM_INPUT_START, memory_region::MemoryRegion,
            vm::Config,
        },
        solana_sdk::{
            account::{Account, AccountSharedData},
            clock::Epoch,
            rent::Rent,
            transaction_context::{TransactionAccount, TransactionContext},
        },
        std::{
            cell::{Cell, RefCell},
            mem, ptr,
            rc::Rc,
            slice,
        },
    };

    macro_rules! mock_invoke_context {
        ($invoke_context:ident,
         $transaction_context:ident,
         $instruction_data:expr,
         $transaction_accounts:expr,
         $program_accounts:expr,
         $instruction_accounts:expr) => {
            let program_accounts = $program_accounts;
            let instruction_data = $instruction_data;
            let instruction_accounts = $instruction_accounts
                .iter()
                .enumerate()
                .map(
                    |(index_in_callee, index_in_transaction)| InstructionAccount {
                        index_in_transaction: *index_in_transaction as IndexOfAccount,
                        index_in_caller: *index_in_transaction as IndexOfAccount,
                        index_in_callee: index_in_callee as IndexOfAccount,
                        is_signer: false,
                        is_writable: $transaction_accounts[*index_in_transaction as usize].2,
                    },
                )
                .collect::<Vec<_>>();
            let transaction_accounts = $transaction_accounts
                .into_iter()
                .map(|a| (a.0, a.1))
                .collect::<Vec<TransactionAccount>>();

            let program_accounts = program_accounts;
            let mut $transaction_context =
                TransactionContext::new(transaction_accounts, Some(Rent::default()), 1, 1);

            let mut $invoke_context = InvokeContext::new_mock(&mut $transaction_context, &[]);
            $invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(program_accounts, &instruction_accounts, instruction_data);
            $invoke_context.push().unwrap();
        };
    }

    #[test]
    fn test_translate_instruction() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foo".to_vec());
        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let program_id = Pubkey::new_unique();
        let accounts = vec![AccountMeta {
            pubkey: Pubkey::new_unique(),
            is_signer: true,
            is_writable: false,
        }];
        let data = b"ins data".to_vec();
        let vm_addr = MM_INPUT_START;
        let (_mem, region) = MockInstruction {
            program_id,
            accounts: accounts.clone(),
            data: data.clone(),
        }
        .into_region(vm_addr);

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mut memory_mapping = MemoryMapping::new(vec![region], &config).unwrap();

        let ins = SyscallInvokeSignedRust::translate_instruction(
            vm_addr,
            &mut memory_mapping,
            &mut invoke_context,
        )
        .unwrap();
        assert_eq!(ins.program_id, program_id);
        assert_eq!(ins.accounts, accounts);
        assert_eq!(ins.data, data);
    }

    #[test]
    fn test_translate_signers() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foo".to_vec());
        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let program_id = Pubkey::new_unique();
        let (derived_key, bump_seed) = Pubkey::find_program_address(&[b"foo"], &program_id);

        let vm_addr = MM_INPUT_START;
        let (_mem, region) = mock_signers(&[b"foo", &[bump_seed]], vm_addr);

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mut memory_mapping = MemoryMapping::new(vec![region], &config).unwrap();

        let signers = SyscallInvokeSignedRust::translate_signers(
            &program_id,
            vm_addr,
            1,
            &mut memory_mapping,
            &invoke_context,
        )
        .unwrap();
        assert_eq!(signers[0], derived_key);
    }

    #[test]
    fn test_caller_account_from_account_info() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foo".to_vec());
        let account = transaction_accounts[1].1.clone();
        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let key = Pubkey::new_unique();
        let vm_addr = MM_INPUT_START;
        let (_mem, region) = MockAccountInfo::new(key, &account).into_region(vm_addr);

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let memory_mapping = MemoryMapping::new(vec![region], &config).unwrap();

        let account_info = translate_type::<AccountInfo>(&memory_mapping, vm_addr, false).unwrap();

        let caller_account = CallerAccount::from_account_info(
            &invoke_context,
            &memory_mapping,
            vm_addr,
            account_info,
            account.data().len(),
        )
        .unwrap();
        assert_eq!(*caller_account.lamports, account.lamports());
        assert_eq!(caller_account.owner, account.owner());
        assert_eq!(caller_account.original_data_len, account.data().len());
        assert_eq!(
            *caller_account.ref_to_len_in_vm as usize,
            account.data().len()
        );
        assert_eq!(caller_account.data, account.data());
        assert_eq!(caller_account.executable, account.executable());
        assert_eq!(caller_account.rent_epoch, account.rent_epoch());
    }

    #[test]
    fn test_update_caller_account_lamports_owner() {
        let transaction_accounts = transaction_with_one_instruction_account(vec![]);
        let account = transaction_accounts[1].1.clone();
        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let instruction_context = invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap();

        let mut mock_caller_account =
            MockCallerAccount::new(1234, *account.owner(), 0xFFFFFFFF00000000, account.data());

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let memory_mapping =
            MemoryMapping::new(vec![mock_caller_account.region.clone()], &config).unwrap();

        let mut caller_account = mock_caller_account.caller_account();

        let mut callee_account = instruction_context
            .try_borrow_instruction_account(invoke_context.transaction_context, 0)
            .unwrap();

        callee_account.set_lamports(42).unwrap();
        callee_account
            .set_owner(Pubkey::new_unique().as_ref())
            .unwrap();

        update_caller_account(
            &invoke_context,
            &memory_mapping,
            &mut caller_account,
            &callee_account,
        )
        .unwrap();

        assert_eq!(*caller_account.lamports, 42);
        assert_eq!(caller_account.owner, callee_account.get_owner());
    }

    #[test]
    fn test_update_caller_account_data() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foobar".to_vec());
        let account = transaction_accounts[1].1.clone();
        let original_data_len = account.data().len();

        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let instruction_context = invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap();

        let mut mock_caller_account = MockCallerAccount::new(
            account.lamports(),
            *account.owner(),
            0xFFFFFFFF00000000,
            account.data(),
        );

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let memory_mapping =
            MemoryMapping::new(vec![mock_caller_account.region.clone()], &config).unwrap();

        let data_slice = mock_caller_account.data_slice();
        let len_ptr = unsafe {
            data_slice
                .as_ptr()
                .offset(-(mem::size_of::<u64>() as isize))
        };
        let serialized_len = || unsafe { *len_ptr.cast::<u64>() as usize };
        let mut caller_account = mock_caller_account.caller_account();

        let mut callee_account = instruction_context
            .try_borrow_instruction_account(invoke_context.transaction_context, 0)
            .unwrap();

        for (new_value, expected_realloc_size) in [
            (b"foo".to_vec(), MAX_PERMITTED_DATA_INCREASE + 3),
            (b"foobaz".to_vec(), MAX_PERMITTED_DATA_INCREASE),
            (b"foobazbad".to_vec(), MAX_PERMITTED_DATA_INCREASE - 3),
        ] {
            assert_eq!(caller_account.data, callee_account.get_data());
            callee_account.set_data_from_slice(&new_value).unwrap();

            update_caller_account(
                &invoke_context,
                &memory_mapping,
                &mut caller_account,
                &callee_account,
            )
            .unwrap();

            let data_len = callee_account.get_data().len();
            assert_eq!(data_len, *caller_account.ref_to_len_in_vm as usize);
            assert_eq!(data_len, serialized_len());
            assert_eq!(data_len, caller_account.data.len());
            assert_eq!(callee_account.get_data(), &caller_account.data[..data_len]);
            assert_eq!(data_slice[data_len..].len(), expected_realloc_size);
            assert!(is_zeroed(&data_slice[data_len..]));
        }

        callee_account
            .set_data_length(original_data_len + MAX_PERMITTED_DATA_INCREASE)
            .unwrap();
        update_caller_account(
            &invoke_context,
            &memory_mapping,
            &mut caller_account,
            &callee_account,
        )
        .unwrap();
        let data_len = callee_account.get_data().len();
        assert_eq!(data_slice[data_len..].len(), 0);
        assert!(is_zeroed(&data_slice[data_len..]));

        callee_account
            .set_data_length(original_data_len + MAX_PERMITTED_DATA_INCREASE + 1)
            .unwrap();
        assert!(matches!(
            update_caller_account(
                &invoke_context,
                &memory_mapping,
                &mut caller_account,
                &callee_account,
            ),
            Err(EbpfError::UserError(error)) if error.downcast_ref::<BpfError>().unwrap() == &BpfError::SyscallError(
                SyscallError::InstructionError(InstructionError::InvalidRealloc)
            )
        ));
    }

    macro_rules! borrow_instruction_account {
        ($invoke_context:expr, $index:expr) => {{
            let instruction_context = $invoke_context
                .transaction_context
                .get_current_instruction_context()
                .unwrap();
            instruction_context
                .try_borrow_instruction_account($invoke_context.transaction_context, $index)
                .unwrap()
        }};
    }

    #[test]
    fn test_update_callee_account_lamports_owner() {
        let transaction_accounts = transaction_with_one_instruction_account(vec![]);
        let account = transaction_accounts[1].1.clone();

        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let mut mock_caller_account =
            MockCallerAccount::new(1234, *account.owner(), 0xFFFFFFFF00000000, account.data());

        let caller_account = mock_caller_account.caller_account();

        let callee_account = borrow_instruction_account!(invoke_context, 0);

        *caller_account.lamports = 42;
        *caller_account.owner = Pubkey::new_unique();

        update_callee_account(&invoke_context, &caller_account, callee_account).unwrap();

        let callee_account = borrow_instruction_account!(invoke_context, 0);
        assert_eq!(callee_account.get_lamports(), 42);
        assert_eq!(caller_account.owner, callee_account.get_owner());
    }

    #[test]
    fn test_update_callee_account_data() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foobar".to_vec());
        let account = transaction_accounts[1].1.clone();

        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1]
        );

        let mut mock_caller_account =
            MockCallerAccount::new(1234, *account.owner(), 0xFFFFFFFF00000000, account.data());

        let mut caller_account = mock_caller_account.caller_account();

        let callee_account = borrow_instruction_account!(invoke_context, 0);

        let mut data = b"foo".to_vec();
        caller_account.data = &mut data;

        update_callee_account(&invoke_context, &caller_account, callee_account).unwrap();

        let callee_account = borrow_instruction_account!(invoke_context, 0);
        assert_eq!(callee_account.get_data(), caller_account.data);
    }

    #[test]
    fn test_translate_accounts_rust() {
        let transaction_accounts = transaction_with_one_instruction_account(b"foobar".to_vec());
        let account = transaction_accounts[1].1.clone();
        let key = transaction_accounts[1].0;
        let original_data_len = account.data().len();

        mock_invoke_context!(
            invoke_context,
            transaction_context,
            b"instruction data",
            transaction_accounts,
            &[0],
            &[1, 1]
        );
        invoke_context
            .set_syscall_context(
                true,
                true,
                vec![original_data_len],
                Rc::new(RefCell::new(BpfAllocator::new(
                    AlignedMemory::with_capacity(0),
                    0,
                ))),
            )
            .unwrap();

        let vm_addr = MM_INPUT_START;
        let (_mem, region) = MockAccountInfo::new(key, &account).into_region(vm_addr);

        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mut memory_mapping = MemoryMapping::new(vec![region], &config).unwrap();

        let accounts = SyscallInvokeSignedRust::translate_accounts(
            &[
                InstructionAccount {
                    index_in_transaction: 1,
                    index_in_caller: 0,
                    index_in_callee: 0,
                    is_signer: false,
                    is_writable: true,
                },
                InstructionAccount {
                    index_in_transaction: 1,
                    index_in_caller: 0,
                    index_in_callee: 0,
                    is_signer: false,
                    is_writable: true,
                },
            ],
            &[0],
            vm_addr,
            1,
            &mut memory_mapping,
            &mut invoke_context,
        )
        .unwrap();
        assert_eq!(accounts.len(), 2);
        assert!(accounts[0].1.is_none());
        let caller_account = accounts[1].1.as_ref().unwrap();
        assert_eq!(caller_account.data, account.data());
        assert_eq!(caller_account.original_data_len, original_data_len);
    }

    pub type TestTransactionAccount = (Pubkey, AccountSharedData, bool);

    struct MockCallerAccount {
        lamports: u64,
        owner: Pubkey,
        vm_addr: u64,
        data: Vec<u8>,
        len: u64,
        region: MemoryRegion,
    }

    impl MockCallerAccount {
        fn new(lamports: u64, owner: Pubkey, vm_addr: u64, data: &[u8]) -> MockCallerAccount {
            // write [len][data] into a vec so we can check that they get
            // properly updated by update_caller_account()
            let mut d = vec![0; mem::size_of::<u64>() + data.len() + MAX_PERMITTED_DATA_INCREASE];
            unsafe { ptr::write_unaligned::<u64>(d.as_mut_ptr().cast(), data.len() as u64) };
            d[mem::size_of::<u64>()..][..data.len()].copy_from_slice(data);

            // the memory region must include the realloc data
            let region = MemoryRegion::new_writable(d.as_mut_slice(), vm_addr);

            // caller_account.data must have the actual data length
            d.truncate(mem::size_of::<u64>() + data.len());

            MockCallerAccount {
                lamports,
                owner,
                vm_addr,
                data: d,
                len: data.len() as u64,
                region,
            }
        }

        fn data_slice<'a>(&self) -> &'a [u8] {
            // lifetime crimes
            unsafe {
                slice::from_raw_parts(
                    self.data[mem::size_of::<u64>()..].as_ptr(),
                    self.data.capacity() - mem::size_of::<u64>(),
                )
            }
        }

        fn caller_account(&mut self) -> CallerAccount<'_> {
            let data = &mut self.data[mem::size_of::<u64>()..];
            CallerAccount {
                lamports: &mut self.lamports,
                owner: &mut self.owner,
                original_data_len: data.len(),
                data,
                vm_data_addr: self.vm_addr + mem::size_of::<u64>() as u64,
                ref_to_len_in_vm: &mut self.len,
                executable: false,
                rent_epoch: 0,
            }
        }
    }

    fn transaction_with_one_instruction_account(data: Vec<u8>) -> Vec<TestTransactionAccount> {
        let program_id = Pubkey::new_unique();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: program_id,
            executable: false,
            rent_epoch: 100,
        });
        vec![
            (
                program_id,
                AccountSharedData::from(Account {
                    lamports: 0,
                    data: vec![],
                    owner: bpf_loader::id(),
                    executable: true,
                    rent_epoch: 0,
                }),
                false,
            ),
            (Pubkey::new_unique(), account, true),
        ]
    }

    struct MockInstruction {
        program_id: Pubkey,
        accounts: Vec<AccountMeta>,
        data: Vec<u8>,
    }

    impl MockInstruction {
        fn into_region(self, vm_addr: u64) -> (Vec<u8>, MemoryRegion) {
            let accounts_len = mem::size_of::<AccountMeta>() * self.accounts.len();

            let size = mem::size_of::<Instruction>() + accounts_len + self.data.len();

            let mut data = vec![0; size];

            let vm_addr = vm_addr as usize;
            let accounts_addr = vm_addr + mem::size_of::<Instruction>();
            let data_addr = accounts_addr + accounts_len;

            let ins = Instruction {
                program_id: self.program_id,
                accounts: unsafe {
                    Vec::from_raw_parts(
                        accounts_addr as *mut _,
                        self.accounts.len(),
                        self.accounts.len(),
                    )
                },
                data: unsafe {
                    Vec::from_raw_parts(data_addr as *mut _, self.data.len(), self.data.len())
                },
            };

            unsafe {
                ptr::write_unaligned(data.as_mut_ptr().cast(), ins);
                data[accounts_addr - vm_addr..][..accounts_len].copy_from_slice(
                    slice::from_raw_parts(self.accounts.as_ptr().cast(), accounts_len),
                );
                data[data_addr - vm_addr..].copy_from_slice(&self.data);
            }

            let region = MemoryRegion::new_writable(data.as_mut_slice(), vm_addr as u64);
            (data, region)
        }
    }

    fn mock_signers(signers: &[&[u8]], vm_addr: u64) -> (Vec<u8>, MemoryRegion) {
        let slice_size = mem::size_of::<&[()]>();
        let size = signers
            .iter()
            .fold(slice_size, |size, signer| size + slice_size + signer.len());

        let vm_addr = vm_addr as usize;
        let mut slices_addr = vm_addr + slice_size;

        let mut data = vec![0; size];
        unsafe {
            ptr::write_unaligned(
                data.as_mut_ptr().cast(),
                slice::from_raw_parts::<&[&[u8]]>(slices_addr as *const _, signers.len()),
            );
        }

        let mut signers_addr = slices_addr + signers.len() * slice_size;

        for signer in signers {
            unsafe {
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + slices_addr - vm_addr) as *mut _,
                    slice::from_raw_parts::<&[u8]>(signers_addr as *const _, signer.len()),
                );
            }
            slices_addr += slice_size;
            signers_addr += signer.len();
        }

        let slices_addr = vm_addr + slice_size;
        let mut signers_addr = slices_addr + signers.len() * slice_size;
        for signer in signers {
            data[signers_addr - vm_addr..][..signer.len()].copy_from_slice(signer);
            signers_addr += signer.len();
        }

        let region = MemoryRegion::new_writable(data.as_mut_slice(), vm_addr as u64);
        (data, region)
    }

    struct MockAccountInfo<'a> {
        key: Pubkey,
        is_signer: bool,
        is_writable: bool,
        lamports: u64,
        data: &'a [u8],
        owner: Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    }

    impl<'a> MockAccountInfo<'a> {
        fn new(key: Pubkey, account: &AccountSharedData) -> MockAccountInfo {
            MockAccountInfo {
                key,
                is_signer: false,
                is_writable: false,
                lamports: account.lamports(),
                data: account.data(),
                owner: *account.owner(),
                executable: account.executable(),
                rent_epoch: account.rent_epoch(),
            }
        }

        fn into_region(self, vm_addr: u64) -> (Vec<u8>, MemoryRegion) {
            let size = mem::size_of::<AccountInfo>()
                + mem::size_of::<Pubkey>() * 2
                + mem::size_of::<RcBox<RefCell<&mut u64>>>()
                + mem::size_of::<u64>()
                + mem::size_of::<RcBox<RefCell<&mut [u8]>>>()
                + self.data.len();
            let mut data = vec![0; size];

            let vm_addr = vm_addr as usize;
            let key_addr = vm_addr + mem::size_of::<AccountInfo>();
            let lamports_cell_addr = key_addr + mem::size_of::<Pubkey>();
            let lamports_addr = lamports_cell_addr + mem::size_of::<RcBox<RefCell<&mut u64>>>();
            let owner_addr = lamports_addr + mem::size_of::<u64>();
            let data_cell_addr = owner_addr + mem::size_of::<Pubkey>();
            let data_addr = data_cell_addr + mem::size_of::<RcBox<RefCell<&mut [u8]>>>();

            let info = AccountInfo {
                key: unsafe { (key_addr as *const Pubkey).as_ref() }.unwrap(),
                is_signer: self.is_signer,
                is_writable: self.is_writable,
                lamports: unsafe {
                    Rc::from_raw((lamports_cell_addr + RcBox::<&mut u64>::VALUE_OFFSET) as *const _)
                },
                data: unsafe {
                    Rc::from_raw((data_cell_addr + RcBox::<&mut [u8]>::VALUE_OFFSET) as *const _)
                },
                owner: unsafe { (owner_addr as *const Pubkey).as_ref() }.unwrap(),
                executable: self.executable,
                rent_epoch: self.rent_epoch,
            };

            unsafe {
                ptr::write_unaligned(data.as_mut_ptr().cast(), info);
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + key_addr - vm_addr) as *mut _,
                    self.key,
                );
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + lamports_cell_addr - vm_addr) as *mut _,
                    RcBox::new(RefCell::new((lamports_addr as *mut u64).as_mut().unwrap())),
                );
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + lamports_addr - vm_addr) as *mut _,
                    self.lamports,
                );
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + owner_addr - vm_addr) as *mut _,
                    self.owner,
                );
                ptr::write_unaligned(
                    (data.as_mut_ptr() as usize + data_cell_addr - vm_addr) as *mut _,
                    RcBox::new(RefCell::new(slice::from_raw_parts_mut(
                        data_addr as *mut u8,
                        self.data.len(),
                    ))),
                );
                data[data_addr - vm_addr..].copy_from_slice(self.data);
            }

            let region = MemoryRegion::new_writable(data.as_mut_slice(), vm_addr as u64);
            (data, region)
        }
    }

    #[repr(C)]
    struct RcBox<T> {
        strong: Cell<usize>,
        weak: Cell<usize>,
        value: T,
    }

    impl<T> RcBox<T> {
        const VALUE_OFFSET: usize = mem::size_of::<Cell<usize>>() * 2;
        fn new(value: T) -> RcBox<T> {
            RcBox {
                strong: Cell::new(0),
                weak: Cell::new(0),
                value,
            }
        }
    }

    fn is_zeroed(data: &[u8]) -> bool {
        data.iter().all(|b| *b == 0)
    }
}
