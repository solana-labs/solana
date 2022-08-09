use {
    super::*,
    crate::declare_syscall,
    solana_sdk::syscalls::{
        MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_ACCOUNTS, MAX_CPI_INSTRUCTION_DATA_LEN,
    },
};

struct CallerAccount<'a> {
    lamports: &'a mut u64,
    owner: &'a mut Pubkey,
    original_data_len: usize,
    data: &'a mut [u8],
    vm_data_addr: u64,
    ref_to_len_in_vm: &'a mut u64,
    executable: bool,
    rent_epoch: u64,
}
type TranslatedAccounts<'a> = Vec<(usize, Option<CallerAccount<'a>>)>;

/// Implemented by language specific data structure translators
trait SyscallInvokeSigned<'a, 'b> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut InvokeContext<'b>>, EbpfError<BpfError>>;
    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError<BpfError>>;
    fn translate_accounts<'c>(
        &'c self,
        instruction_accounts: &[InstructionAccount],
        program_indices: &[usize],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'c>, EbpfError<BpfError>>;
    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError<BpfError>>;
}

declare_syscall!(
    /// Cross-program invocation called from Rust
    SyscallInvokeSignedRust,
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        *result = call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        );
    }
);

impl<'a, 'b> SyscallInvokeSigned<'a, 'b> for SyscallInvokeSignedRust<'a, 'b> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut InvokeContext<'b>>, EbpfError<BpfError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }

    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError<BpfError>> {
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
            invoke_context.get_compute_meter().consume(
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

    fn translate_accounts<'c>(
        &'c self,
        instruction_accounts: &[InstructionAccount],
        program_indices: &[usize],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'c>, EbpfError<BpfError>> {
        let account_infos = translate_slice::<AccountInfo>(
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
                    account_info.key as *const _ as u64,
                    invoke_context.get_check_aligned(),
                )
            })
            .collect::<Result<Vec<_>, EbpfError<BpfError>>>()?;

        let translate = |account_info: &AccountInfo, invoke_context: &InvokeContext| {
            // Translate the account from user space

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

                invoke_context.get_compute_meter().consume(
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
                original_data_len: 0, // set later
                data,
                vm_data_addr,
                ref_to_len_in_vm,
                executable: account_info.executable,
                rent_epoch: account_info.rent_epoch,
            })
        };

        get_translated_accounts(
            instruction_accounts,
            program_indices,
            &account_info_keys,
            account_infos,
            invoke_context,
            translate,
        )
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError<BpfError>> {
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
                    .collect::<Result<Vec<_>, EbpfError<BpfError>>>()?;
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
    fn call(
        &mut self,
        instruction_addr: u64,
        account_infos_addr: u64,
        account_infos_len: u64,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        *result = call(
            self,
            instruction_addr,
            account_infos_addr,
            account_infos_len,
            signers_seeds_addr,
            signers_seeds_len,
            memory_mapping,
        );
    }
);

impl<'a, 'b> SyscallInvokeSigned<'a, 'b> for SyscallInvokeSignedC<'a, 'b> {
    fn get_context_mut(&self) -> Result<RefMut<&'a mut InvokeContext<'b>>, EbpfError<BpfError>> {
        self.invoke_context
            .try_borrow_mut()
            .map_err(|_| SyscallError::InvokeContextBorrowFailed.into())
    }

    fn translate_instruction(
        &self,
        addr: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<Instruction, EbpfError<BpfError>> {
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
            ix_c.accounts_len as u64,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;

        let ix_data_len = ix_c.data_len as u64;
        if invoke_context
            .feature_set
            .is_active(&feature_set::loosen_cpi_size_restriction::id())
        {
            invoke_context.get_compute_meter().consume(
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
            .collect::<Result<Vec<AccountMeta>, EbpfError<BpfError>>>()?;

        Ok(Instruction {
            program_id: *program_id,
            accounts,
            data,
        })
    }

    fn translate_accounts<'c>(
        &'c self,
        instruction_accounts: &[InstructionAccount],
        program_indices: &[usize],
        account_infos_addr: u64,
        account_infos_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &mut InvokeContext,
    ) -> Result<TranslatedAccounts<'c>, EbpfError<BpfError>> {
        let account_infos = translate_slice::<SolAccountInfo>(
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
                    account_info.key_addr,
                    invoke_context.get_check_aligned(),
                )
            })
            .collect::<Result<Vec<_>, EbpfError<BpfError>>>()?;

        let translate = |account_info: &SolAccountInfo, invoke_context: &InvokeContext| {
            // Translate the account from user space

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

            invoke_context.get_compute_meter().consume(
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

            let first_info_addr = account_infos.first().ok_or(SyscallError::InstructionError(
                InstructionError::InvalidArgument,
            ))? as *const _ as u64;
            let addr = &account_info.data_len as *const u64 as u64;
            let vm_addr = if invoke_context
                .feature_set
                .is_active(&syscall_saturated_math::id())
            {
                account_infos_addr.saturating_add(addr.saturating_sub(first_info_addr))
            } else {
                #[allow(clippy::integer_arithmetic)]
                {
                    account_infos_addr + (addr - first_info_addr)
                }
            };
            let _ = translate(
                memory_mapping,
                AccessType::Store,
                vm_addr,
                size_of::<u64>() as u64,
            )?;
            let ref_to_len_in_vm = unsafe { &mut *(addr as *mut u64) };

            Ok(CallerAccount {
                lamports,
                owner,
                original_data_len: 0, // set later
                data,
                vm_data_addr,
                ref_to_len_in_vm,
                executable: account_info.executable,
                rent_epoch: account_info.rent_epoch,
            })
        };

        get_translated_accounts(
            instruction_accounts,
            program_indices,
            &account_info_keys,
            account_infos,
            invoke_context,
            translate,
        )
    }

    fn translate_signers(
        &self,
        program_id: &Pubkey,
        signers_seeds_addr: u64,
        signers_seeds_len: u64,
        memory_mapping: &mut MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<Vec<Pubkey>, EbpfError<BpfError>> {
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
                        .collect::<Result<Vec<_>, EbpfError<BpfError>>>()?;
                    Pubkey::create_program_address(&seeds_bytes, program_id)
                        .map_err(|err| SyscallError::BadSeeds(err).into())
                })
                .collect::<Result<Vec<_>, EbpfError<BpfError>>>()?)
        } else {
            Ok(vec![])
        }
    }
}

fn get_translated_accounts<'a, T, F>(
    instruction_accounts: &[InstructionAccount],
    program_indices: &[usize],
    account_info_keys: &[&Pubkey],
    account_infos: &[T],
    invoke_context: &mut InvokeContext,
    do_translate: F,
) -> Result<TranslatedAccounts<'a>, EbpfError<BpfError>>
where
    F: Fn(&T, &InvokeContext) -> Result<CallerAccount<'a>, EbpfError<BpfError>>,
{
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;
    let mut accounts = Vec::with_capacity(instruction_accounts.len().saturating_add(1));
    let is_disable_cpi_setting_executable_and_rent_epoch_active = invoke_context
        .feature_set
        .is_active(&disable_cpi_setting_executable_and_rent_epoch::id());

    let program_account_index = program_indices
        .last()
        .ok_or(SyscallError::InstructionError(
            InstructionError::MissingAccount,
        ))?;
    accounts.push((*program_account_index, None));

    for (instruction_account_index, instruction_account) in instruction_accounts.iter().enumerate()
    {
        if instruction_account_index != instruction_account.index_in_callee {
            continue; // Skip duplicate account
        }
        let mut callee_account = instruction_context
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
            invoke_context.get_compute_meter().consume(
                (callee_account.get_data().len() as u64)
                    .saturating_div(invoke_context.get_compute_budget().cpi_bytes_per_unit),
            )?;

            accounts.push((instruction_account.index_in_caller, None));
        } else if let Some(caller_account_index) =
            account_info_keys.iter().position(|key| *key == account_key)
        {
            let mut caller_account = do_translate(
                account_infos
                    .get(caller_account_index)
                    .ok_or(SyscallError::InvalidLength)?,
                invoke_context,
            )?;
            {
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
                        .set_data(caller_account.data)
                        .map_err(SyscallError::InstructionError)?,
                    Err(err) if callee_account.get_data() != caller_account.data => {
                        return Err(EbpfError::UserError(BpfError::SyscallError(
                            SyscallError::InstructionError(err),
                        )));
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
                drop(callee_account);
                let callee_account = invoke_context
                    .transaction_context
                    .get_account_at_index(instruction_account.index_in_transaction)
                    .map_err(SyscallError::InstructionError)?;
                if !is_disable_cpi_setting_executable_and_rent_epoch_active
                    && callee_account.borrow().rent_epoch() != caller_account.rent_epoch
                {
                    if invoke_context
                        .feature_set
                        .is_active(&enable_early_verification_of_account_modifications::id())
                    {
                        return Err(SyscallError::InstructionError(
                            InstructionError::RentEpochModified,
                        )
                        .into());
                    } else {
                        callee_account
                            .borrow_mut()
                            .set_rent_epoch(caller_account.rent_epoch);
                    }
                }
            }
            let caller_account = if instruction_account.is_writable {
                let orig_data_lens = invoke_context
                    .get_orig_account_lengths()
                    .map_err(SyscallError::InstructionError)?;
                caller_account.original_data_len = *orig_data_lens
                    .get(instruction_account.index_in_caller)
                    .ok_or_else(|| {
                        ic_msg!(
                            invoke_context,
                            "Internal error: index mismatch for account {}",
                            account_key
                        );
                        SyscallError::InstructionError(InstructionError::MissingAccount)
                    })?;
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
) -> Result<(), EbpfError<BpfError>> {
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
) -> Result<(), EbpfError<BpfError>> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::loosen_cpi_size_restriction::id())
    {
        let num_account_infos = num_account_infos as u64;
        let max_account_infos = MAX_CPI_ACCOUNT_INFOS as u64;
        if num_account_infos > max_account_infos {
            return Err(SyscallError::MaxInstructionAccountInfosExceeded {
                num_account_infos,
                max_account_infos,
            }
            .into());
        }
    } else {
        let adjusted_len = if invoke_context
            .feature_set
            .is_active(&syscall_saturated_math::id())
        {
            num_account_infos.saturating_mul(size_of::<Pubkey>())
        } else {
            #[allow(clippy::integer_arithmetic)]
            {
                num_account_infos * size_of::<Pubkey>()
            }
        };
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
) -> Result<(), EbpfError<BpfError>> {
    #[allow(clippy::blocks_in_if_conditions)]
    if native_loader::check_id(program_id)
        || bpf_loader::check_id(program_id)
        || bpf_loader_deprecated::check_id(program_id)
        || (bpf_loader_upgradeable::check_id(program_id)
            && !(bpf_loader_upgradeable::is_upgrade_instruction(instruction_data)
                || bpf_loader_upgradeable::is_set_authority_instruction(instruction_data)
                || bpf_loader_upgradeable::is_close_instruction(instruction_data)))
        || (invoke_context
            .feature_set
            .is_active(&prevent_calling_precompiles_as_programs::id())
            && is_precompile(program_id, |feature_id: &Pubkey| {
                invoke_context.feature_set.is_active(feature_id)
            }))
    {
        return Err(SyscallError::ProgramNotSupported(*program_id).into());
    }
    Ok(())
}

/// Call process instruction, common to both Rust and C
fn call<'a, 'b: 'a>(
    syscall: &mut dyn SyscallInvokeSigned<'a, 'b>,
    instruction_addr: u64,
    account_infos_addr: u64,
    account_infos_len: u64,
    signers_seeds_addr: u64,
    signers_seeds_len: u64,
    memory_mapping: &mut MemoryMapping,
) -> Result<u64, EbpfError<BpfError>> {
    let mut invoke_context = syscall.get_context_mut()?;
    invoke_context
        .get_compute_meter()
        .consume(invoke_context.get_compute_budget().invoke_units)?;

    // Translate and verify caller's data
    let instruction =
        syscall.translate_instruction(instruction_addr, memory_mapping, *invoke_context)?;
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;
    let caller_program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(SyscallError::InstructionError)?;
    let signers = syscall.translate_signers(
        caller_program_id,
        signers_seeds_addr,
        signers_seeds_len,
        memory_mapping,
        *invoke_context,
    )?;
    let (instruction_accounts, program_indices) = invoke_context
        .prepare_instruction(&instruction, &signers)
        .map_err(SyscallError::InstructionError)?;
    check_authorized_program(&instruction.program_id, &instruction.data, *invoke_context)?;
    let mut accounts = syscall.translate_accounts(
        &instruction_accounts,
        &program_indices,
        account_infos_addr,
        account_infos_len,
        memory_mapping,
        *invoke_context,
    )?;

    // Process instruction
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

    // Copy results back to caller
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(SyscallError::InstructionError)?;
    for (index_in_caller, caller_account) in accounts.iter_mut() {
        if let Some(caller_account) = caller_account {
            let callee_account = instruction_context
                .try_borrow_instruction_account(transaction_context, *index_in_caller)
                .map_err(SyscallError::InstructionError)?;
            *caller_account.lamports = callee_account.get_lamports();
            *caller_account.owner = *callee_account.get_owner();
            let new_len = callee_account.get_data().len();
            if caller_account.data.len() != new_len {
                let data_overflow = if invoke_context
                    .feature_set
                    .is_active(&syscall_saturated_math::id())
                {
                    new_len
                        > caller_account
                            .original_data_len
                            .saturating_add(MAX_PERMITTED_DATA_INCREASE)
                } else {
                    #[allow(clippy::integer_arithmetic)]
                    {
                        new_len > caller_account.original_data_len + MAX_PERMITTED_DATA_INCREASE
                    }
                };
                if data_overflow {
                    ic_msg!(
                        invoke_context,
                        "Account data size realloc limited to {} in inner instructions",
                        MAX_PERMITTED_DATA_INCREASE
                    );
                    return Err(
                        SyscallError::InstructionError(InstructionError::InvalidRealloc).into(),
                    );
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
                *caller_account.ref_to_len_in_vm = new_len as u64;
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
                return Err(
                    SyscallError::InstructionError(InstructionError::AccountDataTooSmall).into(),
                );
            }
            to_slice.copy_from_slice(from_slice);
        }
    }

    Ok(SUCCESS)
}
