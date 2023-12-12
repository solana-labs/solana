use {
    crate::{
        accounts_data_meter::AccountsDataMeter,
        compute_budget::ComputeBudget,
        ic_logger_msg, ic_msg,
        loaded_programs::{LoadedProgram, LoadedProgramType, LoadedProgramsForTxBatch},
        log_collector::LogCollector,
        pre_account::PreAccount,
        stable_log,
        sysvar_cache::SysvarCache,
        timings::{ExecuteDetailsTimings, ExecuteTimings},
    },
    solana_measure::measure::Measure,
    solana_rbpf::{
        ebpf::MM_HEAP_START,
        error::{EbpfError, ProgramResult},
        memory_region::MemoryMapping,
        program::{BuiltinFunction, SBPFVersion},
        vm::{Config, ContextObject, EbpfVm},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        bpf_loader_deprecated,
        feature_set::{
            check_slice_translation_size, enable_early_verification_of_account_modifications,
            native_programs_consume_cu, FeatureSet,
        },
        hash::Hash,
        instruction::{AccountMeta, InstructionError},
        native_loader,
        pubkey::Pubkey,
        rent::Rent,
        saturating_add_assign,
        stable_layout::stable_instruction::StableInstruction,
        transaction_context::{
            IndexOfAccount, InstructionAccount, TransactionAccount, TransactionContext,
        },
    },
    std::{
        alloc::Layout,
        cell::RefCell,
        fmt::{self, Debug},
        rc::Rc,
        sync::{atomic::Ordering, Arc},
    },
};

pub type BuiltinFunctionWithContext = BuiltinFunction<InvokeContext<'static>>;

/// Adapter so we can unify the interfaces of built-in programs and syscalls
#[macro_export]
macro_rules! declare_process_instruction {
    ($process_instruction:ident, $cu_to_consume:expr, |$invoke_context:ident| $inner:tt) => {
        $crate::solana_rbpf::declare_builtin_function!(
            $process_instruction,
            fn rust(
                invoke_context: &mut $crate::invoke_context::InvokeContext,
                _arg0: u64,
                _arg1: u64,
                _arg2: u64,
                _arg3: u64,
                _arg4: u64,
                _memory_mapping: &mut $crate::solana_rbpf::memory_region::MemoryMapping,
            ) -> std::result::Result<u64, Box<dyn std::error::Error>> {
                fn process_instruction_inner(
                    $invoke_context: &mut $crate::invoke_context::InvokeContext,
                ) -> std::result::Result<(), solana_sdk::instruction::InstructionError> {
                    $inner
                }
                let consumption_result = if $cu_to_consume > 0
                    && invoke_context
                        .feature_set
                        .is_active(&solana_sdk::feature_set::native_programs_consume_cu::id())
                {
                    invoke_context.consume_checked($cu_to_consume)
                } else {
                    Ok(())
                };
                consumption_result
                    .and_then(|_| {
                        process_instruction_inner(invoke_context)
                            .map(|_| 0)
                            .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
                    })
                    .into()
            }
        );
    };
}

impl<'a> ContextObject for InvokeContext<'a> {
    fn trace(&mut self, state: [u64; 12]) {
        self.syscall_context
            .last_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .trace_log
            .push(state);
    }

    fn consume(&mut self, amount: u64) {
        // 1 to 1 instruction to compute unit mapping
        // ignore overflow, Ebpf will bail if exceeded
        let mut compute_meter = self.compute_meter.borrow_mut();
        *compute_meter = compute_meter.saturating_sub(amount);
    }

    fn get_remaining(&self) -> u64 {
        *self.compute_meter.borrow()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocErr;
impl fmt::Display for AllocErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Error: Memory allocation failed")
    }
}

pub struct BpfAllocator {
    len: u64,
    pos: u64,
}

impl BpfAllocator {
    pub fn new(len: u64) -> Self {
        Self { len, pos: 0 }
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<u64, AllocErr> {
        let bytes_to_align = (self.pos as *const u8).align_offset(layout.align()) as u64;
        if self
            .pos
            .saturating_add(bytes_to_align)
            .saturating_add(layout.size() as u64)
            <= self.len
        {
            self.pos = self.pos.saturating_add(bytes_to_align);
            let addr = MM_HEAP_START.saturating_add(self.pos);
            self.pos = self.pos.saturating_add(layout.size() as u64);
            Ok(addr)
        } else {
            Err(AllocErr)
        }
    }
}

pub struct SyscallContext {
    pub allocator: BpfAllocator,
    pub accounts_metadata: Vec<SerializedAccountMetadata>,
    pub trace_log: Vec<[u64; 12]>,
}

#[derive(Debug, Clone)]
pub struct SerializedAccountMetadata {
    pub original_data_len: usize,
    pub vm_data_addr: u64,
    pub vm_key_addr: u64,
    pub vm_lamports_addr: u64,
    pub vm_owner_addr: u64,
}

pub struct InvokeContext<'a> {
    pub transaction_context: &'a mut TransactionContext,
    rent: Rent,
    pre_accounts: Vec<PreAccount>,
    sysvar_cache: &'a SysvarCache,
    log_collector: Option<Rc<RefCell<LogCollector>>>,
    compute_budget: ComputeBudget,
    current_compute_budget: ComputeBudget,
    compute_meter: RefCell<u64>,
    accounts_data_meter: AccountsDataMeter,
    pub programs_loaded_for_tx_batch: &'a LoadedProgramsForTxBatch,
    pub programs_modified_by_tx: &'a mut LoadedProgramsForTxBatch,
    pub programs_updated_only_for_global_cache: &'a mut LoadedProgramsForTxBatch,
    pub feature_set: Arc<FeatureSet>,
    pub timings: ExecuteDetailsTimings,
    pub blockhash: Hash,
    pub lamports_per_signature: u64,
    pub syscall_context: Vec<Option<SyscallContext>>,
    traces: Vec<Vec<[u64; 12]>>,
}

impl<'a> InvokeContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction_context: &'a mut TransactionContext,
        rent: Rent,
        sysvar_cache: &'a SysvarCache,
        log_collector: Option<Rc<RefCell<LogCollector>>>,
        compute_budget: ComputeBudget,
        programs_loaded_for_tx_batch: &'a LoadedProgramsForTxBatch,
        programs_modified_by_tx: &'a mut LoadedProgramsForTxBatch,
        programs_updated_only_for_global_cache: &'a mut LoadedProgramsForTxBatch,
        feature_set: Arc<FeatureSet>,
        blockhash: Hash,
        lamports_per_signature: u64,
        prev_accounts_data_len: u64,
    ) -> Self {
        Self {
            transaction_context,
            rent,
            pre_accounts: Vec::new(),
            sysvar_cache,
            log_collector,
            current_compute_budget: compute_budget,
            compute_budget,
            compute_meter: RefCell::new(compute_budget.compute_unit_limit),
            accounts_data_meter: AccountsDataMeter::new(prev_accounts_data_len),
            programs_loaded_for_tx_batch,
            programs_modified_by_tx,
            programs_updated_only_for_global_cache,
            feature_set,
            timings: ExecuteDetailsTimings::default(),
            blockhash,
            lamports_per_signature,
            syscall_context: Vec::new(),
            traces: Vec::new(),
        }
    }

    pub fn find_program_in_cache(&self, pubkey: &Pubkey) -> Option<Arc<LoadedProgram>> {
        // First lookup the cache of the programs modified by the current transaction. If not found, lookup
        // the cache of the cache of the programs that are loaded for the transaction batch.
        self.programs_modified_by_tx
            .find(pubkey)
            .or_else(|| self.programs_loaded_for_tx_batch.find(pubkey))
    }

    /// Push a stack frame onto the invocation stack
    pub fn push(&mut self) -> Result<(), InstructionError> {
        let instruction_context = self
            .transaction_context
            .get_instruction_context_at_index_in_trace(
                self.transaction_context.get_instruction_trace_length(),
            )?;
        let program_id = instruction_context
            .get_last_program_key(self.transaction_context)
            .map_err(|_| InstructionError::UnsupportedProgramId)?;
        if self
            .transaction_context
            .get_instruction_context_stack_height()
            == 0
        {
            self.current_compute_budget = self.compute_budget;

            if !self
                .feature_set
                .is_active(&enable_early_verification_of_account_modifications::id())
            {
                self.pre_accounts = Vec::with_capacity(
                    instruction_context.get_number_of_instruction_accounts() as usize,
                );
                for instruction_account_index in
                    0..instruction_context.get_number_of_instruction_accounts()
                {
                    if instruction_context
                        .is_instruction_account_duplicate(instruction_account_index)?
                        .is_some()
                    {
                        continue; // Skip duplicate account
                    }
                    let index_in_transaction = instruction_context
                        .get_index_of_instruction_account_in_transaction(
                            instruction_account_index,
                        )?;
                    if index_in_transaction >= self.transaction_context.get_number_of_accounts() {
                        return Err(InstructionError::MissingAccount);
                    }
                    let account = self
                        .transaction_context
                        .get_account_at_index(index_in_transaction)?
                        .borrow()
                        .clone();
                    self.pre_accounts.push(PreAccount::new(
                        self.transaction_context
                            .get_key_of_account_at_index(index_in_transaction)?,
                        account,
                    ));
                }
            }
        } else {
            let contains = (0..self
                .transaction_context
                .get_instruction_context_stack_height())
                .any(|level| {
                    self.transaction_context
                        .get_instruction_context_at_nesting_level(level)
                        .and_then(|instruction_context| {
                            instruction_context
                                .try_borrow_last_program_account(self.transaction_context)
                        })
                        .map(|program_account| program_account.get_key() == program_id)
                        .unwrap_or(false)
                });
            let is_last = self
                .transaction_context
                .get_current_instruction_context()
                .and_then(|instruction_context| {
                    instruction_context.try_borrow_last_program_account(self.transaction_context)
                })
                .map(|program_account| program_account.get_key() == program_id)
                .unwrap_or(false);
            if contains && !is_last {
                // Reentrancy not allowed unless caller is calling itself
                return Err(InstructionError::ReentrancyNotAllowed);
            }
        }

        self.syscall_context.push(None);
        self.transaction_context.push()
    }

    /// Pop a stack frame from the invocation stack
    pub fn pop(&mut self) -> Result<(), InstructionError> {
        if let Some(Some(syscall_context)) = self.syscall_context.pop() {
            self.traces.push(syscall_context.trace_log);
        }
        self.transaction_context.pop()
    }

    /// Current height of the invocation stack, top level instructions are height
    /// `solana_sdk::instruction::TRANSACTION_LEVEL_STACK_HEIGHT`
    pub fn get_stack_height(&self) -> usize {
        self.transaction_context
            .get_instruction_context_stack_height()
    }

    /// Verify the results of an instruction
    ///
    /// Note: `instruction_accounts` must be the same as passed to `InvokeContext::push()`,
    /// so that they match the order of `pre_accounts`.
    fn verify(
        &mut self,
        instruction_accounts: &[InstructionAccount],
        program_indices: &[IndexOfAccount],
    ) -> Result<(), InstructionError> {
        let instruction_context = self
            .transaction_context
            .get_current_instruction_context()
            .map_err(|_| InstructionError::CallDepth)?;
        let program_id = instruction_context
            .get_last_program_key(self.transaction_context)
            .map_err(|_| InstructionError::CallDepth)?;

        // Verify all executable accounts have zero outstanding refs
        for account_index in program_indices.iter() {
            self.transaction_context
                .get_account_at_index(*account_index)?
                .try_borrow_mut()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
        }

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        let mut pre_account_index = 0;
        for (instruction_account_index, instruction_account) in
            instruction_accounts.iter().enumerate()
        {
            if instruction_account_index as IndexOfAccount != instruction_account.index_in_callee {
                continue; // Skip duplicate account
            }
            {
                // Verify account has no outstanding references
                let _ = self
                    .transaction_context
                    .get_account_at_index(instruction_account.index_in_transaction)?
                    .try_borrow_mut()
                    .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
            }
            let pre_account = &self
                .pre_accounts
                .get(pre_account_index)
                .ok_or(InstructionError::NotEnoughAccountKeys)?;
            pre_account_index = pre_account_index.saturating_add(1);
            let account = self
                .transaction_context
                .get_account_at_index(instruction_account.index_in_transaction)?
                .borrow();
            pre_account
                .verify(
                    program_id,
                    instruction_account.is_writable,
                    &self.rent,
                    &account,
                    &mut self.timings,
                    true,
                )
                .map_err(|err| {
                    ic_logger_msg!(
                        self.log_collector,
                        "failed to verify account {}: {}",
                        pre_account.key(),
                        err
                    );
                    err
                })?;
            pre_sum = pre_sum
                .checked_add(u128::from(pre_account.lamports()))
                .ok_or(InstructionError::UnbalancedInstruction)?;
            post_sum = post_sum
                .checked_add(u128::from(account.lamports()))
                .ok_or(InstructionError::UnbalancedInstruction)?;

            let pre_data_len = pre_account.data().len() as i64;
            let post_data_len = account.data().len() as i64;
            let data_len_delta = post_data_len.saturating_sub(pre_data_len);
            self.accounts_data_meter
                .adjust_delta_unchecked(data_len_delta);
        }

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }

    /// Verify and update PreAccount state based on program execution
    ///
    /// Note: `instruction_accounts` must be the same as passed to `InvokeContext::push()`,
    /// so that they match the order of `pre_accounts`.
    fn verify_and_update(
        &mut self,
        instruction_accounts: &[InstructionAccount],
        before_instruction_context_push: bool,
    ) -> Result<(), InstructionError> {
        let transaction_context = &self.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let program_id = instruction_context
            .get_last_program_key(transaction_context)
            .map_err(|_| InstructionError::CallDepth)?;

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        for (instruction_account_index, instruction_account) in
            instruction_accounts.iter().enumerate()
        {
            if instruction_account_index as IndexOfAccount != instruction_account.index_in_callee {
                continue; // Skip duplicate account
            }
            if instruction_account.index_in_transaction
                < transaction_context.get_number_of_accounts()
            {
                let key = transaction_context
                    .get_key_of_account_at_index(instruction_account.index_in_transaction)?;
                let account = transaction_context
                    .get_account_at_index(instruction_account.index_in_transaction)?;
                let is_writable = if before_instruction_context_push {
                    instruction_context
                        .is_instruction_account_writable(instruction_account.index_in_caller)?
                } else {
                    instruction_account.is_writable
                };
                // Find the matching PreAccount
                for pre_account in self.pre_accounts.iter_mut() {
                    if key == pre_account.key() {
                        {
                            // Verify account has no outstanding references
                            let _ = account
                                .try_borrow_mut()
                                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
                        }
                        let account = account.borrow();
                        pre_account
                            .verify(
                                program_id,
                                is_writable,
                                &self.rent,
                                &account,
                                &mut self.timings,
                                false,
                            )
                            .map_err(|err| {
                                ic_logger_msg!(
                                    self.log_collector,
                                    "failed to verify account {}: {}",
                                    key,
                                    err
                                );
                                err
                            })?;
                        pre_sum = pre_sum
                            .checked_add(u128::from(pre_account.lamports()))
                            .ok_or(InstructionError::UnbalancedInstruction)?;
                        post_sum = post_sum
                            .checked_add(u128::from(account.lamports()))
                            .ok_or(InstructionError::UnbalancedInstruction)?;
                        if is_writable && !pre_account.executable() {
                            pre_account.update(account.clone());
                        }

                        let pre_data_len = pre_account.data().len() as i64;
                        let post_data_len = account.data().len() as i64;
                        let data_len_delta = post_data_len.saturating_sub(pre_data_len);
                        self.accounts_data_meter
                            .adjust_delta_unchecked(data_len_delta);

                        break;
                    }
                }
            }
        }

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }

    /// Entrypoint for a cross-program invocation from a builtin program
    pub fn native_invoke(
        &mut self,
        instruction: StableInstruction,
        signers: &[Pubkey],
    ) -> Result<(), InstructionError> {
        let (instruction_accounts, program_indices) =
            self.prepare_instruction(&instruction, signers)?;
        let mut compute_units_consumed = 0;
        self.process_instruction(
            &instruction.data,
            &instruction_accounts,
            &program_indices,
            &mut compute_units_consumed,
            &mut ExecuteTimings::default(),
        )?;
        Ok(())
    }

    /// Helper to prepare for process_instruction()
    #[allow(clippy::type_complexity)]
    pub fn prepare_instruction(
        &mut self,
        instruction: &StableInstruction,
        signers: &[Pubkey],
    ) -> Result<(Vec<InstructionAccount>, Vec<IndexOfAccount>), InstructionError> {
        // Finds the index of each account in the instruction by its pubkey.
        // Then normalizes / unifies the privileges of duplicate accounts.
        // Note: This is an O(n^2) algorithm,
        // but performed on a very small slice and requires no heap allocations.
        let instruction_context = self.transaction_context.get_current_instruction_context()?;
        let mut deduplicated_instruction_accounts: Vec<InstructionAccount> = Vec::new();
        let mut duplicate_indicies = Vec::with_capacity(instruction.accounts.len());
        for (instruction_account_index, account_meta) in instruction.accounts.iter().enumerate() {
            let index_in_transaction = self
                .transaction_context
                .find_index_of_account(&account_meta.pubkey)
                .ok_or_else(|| {
                    ic_msg!(
                        self,
                        "Instruction references an unknown account {}",
                        account_meta.pubkey,
                    );
                    InstructionError::MissingAccount
                })?;
            if let Some(duplicate_index) =
                deduplicated_instruction_accounts
                    .iter()
                    .position(|instruction_account| {
                        instruction_account.index_in_transaction == index_in_transaction
                    })
            {
                duplicate_indicies.push(duplicate_index);
                let instruction_account = deduplicated_instruction_accounts
                    .get_mut(duplicate_index)
                    .ok_or(InstructionError::NotEnoughAccountKeys)?;
                instruction_account.is_signer |= account_meta.is_signer;
                instruction_account.is_writable |= account_meta.is_writable;
            } else {
                let index_in_caller = instruction_context
                    .find_index_of_instruction_account(
                        self.transaction_context,
                        &account_meta.pubkey,
                    )
                    .ok_or_else(|| {
                        ic_msg!(
                            self,
                            "Instruction references an unknown account {}",
                            account_meta.pubkey,
                        );
                        InstructionError::MissingAccount
                    })?;
                duplicate_indicies.push(deduplicated_instruction_accounts.len());
                deduplicated_instruction_accounts.push(InstructionAccount {
                    index_in_transaction,
                    index_in_caller,
                    index_in_callee: instruction_account_index as IndexOfAccount,
                    is_signer: account_meta.is_signer,
                    is_writable: account_meta.is_writable,
                });
            }
        }
        for instruction_account in deduplicated_instruction_accounts.iter() {
            let borrowed_account = instruction_context.try_borrow_instruction_account(
                self.transaction_context,
                instruction_account.index_in_caller,
            )?;

            // Readonly in caller cannot become writable in callee
            if instruction_account.is_writable && !borrowed_account.is_writable() {
                ic_msg!(
                    self,
                    "{}'s writable privilege escalated",
                    borrowed_account.get_key(),
                );
                return Err(InstructionError::PrivilegeEscalation);
            }

            // To be signed in the callee,
            // it must be either signed in the caller or by the program
            if instruction_account.is_signer
                && !(borrowed_account.is_signer() || signers.contains(borrowed_account.get_key()))
            {
                ic_msg!(
                    self,
                    "{}'s signer privilege escalated",
                    borrowed_account.get_key()
                );
                return Err(InstructionError::PrivilegeEscalation);
            }
        }
        let instruction_accounts = duplicate_indicies
            .into_iter()
            .map(|duplicate_index| {
                Ok(deduplicated_instruction_accounts
                    .get(duplicate_index)
                    .ok_or(InstructionError::NotEnoughAccountKeys)?
                    .clone())
            })
            .collect::<Result<Vec<InstructionAccount>, InstructionError>>()?;

        // Find and validate executables / program accounts
        let callee_program_id = instruction.program_id;
        let program_account_index = instruction_context
            .find_index_of_instruction_account(self.transaction_context, &callee_program_id)
            .ok_or_else(|| {
                ic_msg!(self, "Unknown program {}", callee_program_id);
                InstructionError::MissingAccount
            })?;
        let borrowed_program_account = instruction_context
            .try_borrow_instruction_account(self.transaction_context, program_account_index)?;
        if !borrowed_program_account.is_executable() {
            ic_msg!(self, "Account {} is not executable", callee_program_id);
            return Err(InstructionError::AccountNotExecutable);
        }

        Ok((
            instruction_accounts,
            vec![borrowed_program_account.get_index_in_transaction()],
        ))
    }

    /// Processes an instruction and returns how many compute units were used
    pub fn process_instruction(
        &mut self,
        instruction_data: &[u8],
        instruction_accounts: &[InstructionAccount],
        program_indices: &[IndexOfAccount],
        compute_units_consumed: &mut u64,
        timings: &mut ExecuteTimings,
    ) -> Result<(), InstructionError> {
        *compute_units_consumed = 0;

        let nesting_level = self
            .transaction_context
            .get_instruction_context_stack_height();
        let is_top_level_instruction = nesting_level == 0;
        if !is_top_level_instruction
            && !self
                .feature_set
                .is_active(&enable_early_verification_of_account_modifications::id())
        {
            // Verify the calling program hasn't misbehaved
            let mut verify_caller_time = Measure::start("verify_caller_time");
            let verify_caller_result = self.verify_and_update(instruction_accounts, true);
            verify_caller_time.stop();
            saturating_add_assign!(
                timings
                    .execute_accessories
                    .process_instructions
                    .verify_caller_us,
                verify_caller_time.as_us()
            );
            verify_caller_result?;
        }

        self.transaction_context
            .get_next_instruction_context()?
            .configure(program_indices, instruction_accounts, instruction_data);
        self.push()?;
        self.process_executable_chain(compute_units_consumed, timings)
            .and_then(|_| {
                if self
                    .feature_set
                    .is_active(&enable_early_verification_of_account_modifications::id())
                {
                    Ok(())
                } else {
                    // Verify the called program has not misbehaved
                    let mut verify_callee_time = Measure::start("verify_callee_time");
                    let result = if is_top_level_instruction {
                        self.verify(instruction_accounts, program_indices)
                    } else {
                        self.verify_and_update(instruction_accounts, false)
                    };
                    verify_callee_time.stop();
                    saturating_add_assign!(
                        timings
                            .execute_accessories
                            .process_instructions
                            .verify_callee_us,
                        verify_callee_time.as_us()
                    );
                    result
                }
            })
            // MUST pop if and only if `push` succeeded, independent of `result`.
            // Thus, the `.and()` instead of an `.and_then()`.
            .and(self.pop())
    }

    /// Calls the instruction's program entrypoint method
    fn process_executable_chain(
        &mut self,
        compute_units_consumed: &mut u64,
        timings: &mut ExecuteTimings,
    ) -> Result<(), InstructionError> {
        let instruction_context = self.transaction_context.get_current_instruction_context()?;
        let mut process_executable_chain_time = Measure::start("process_executable_chain_time");

        let builtin_id = {
            let borrowed_root_account = instruction_context
                .try_borrow_program_account(self.transaction_context, 0)
                .map_err(|_| InstructionError::UnsupportedProgramId)?;
            let owner_id = borrowed_root_account.get_owner();
            if native_loader::check_id(owner_id) {
                *borrowed_root_account.get_key()
            } else {
                *owner_id
            }
        };

        // The Murmur3 hash value (used by RBPF) of the string "entrypoint"
        const ENTRYPOINT_KEY: u32 = 0x71E3CF81;
        let entry = self
            .programs_loaded_for_tx_batch
            .find(&builtin_id)
            .ok_or(InstructionError::UnsupportedProgramId)?;
        let function = match &entry.program {
            LoadedProgramType::Builtin(program) => program
                .get_function_registry()
                .lookup_by_key(ENTRYPOINT_KEY)
                .map(|(_name, function)| function),
            _ => None,
        }
        .ok_or(InstructionError::UnsupportedProgramId)?;
        entry.ix_usage_counter.fetch_add(1, Ordering::Relaxed);

        let program_id = *instruction_context.get_last_program_key(self.transaction_context)?;
        self.transaction_context
            .set_return_data(program_id, Vec::new())?;
        let logger = self.get_log_collector();
        stable_log::program_invoke(&logger, &program_id, self.get_stack_height());
        let pre_remaining_units = self.get_remaining();
        // In program-runtime v2 we will create this VM instance only once per transaction.
        // `program_runtime_environment_v2.get_config()` will be used instead of `mock_config`.
        // For now, only built-ins are invoked from here, so the VM and its Config are irrelevant.
        let mock_config = Config::default();
        let empty_memory_mapping =
            MemoryMapping::new(Vec::new(), &mock_config, &SBPFVersion::V1).unwrap();
        let mut vm = EbpfVm::new(
            self.programs_loaded_for_tx_batch
                .environments
                .program_runtime_v2
                .clone(),
            &SBPFVersion::V1,
            // Removes lifetime tracking
            unsafe { std::mem::transmute::<&mut InvokeContext, &mut InvokeContext>(self) },
            empty_memory_mapping,
            0,
        );
        vm.invoke_function(function);
        let result = match vm.program_result {
            ProgramResult::Ok(_) => {
                stable_log::program_success(&logger, &program_id);
                Ok(())
            }
            ProgramResult::Err(ref err) => {
                if let EbpfError::SyscallError(syscall_error) = err {
                    if let Some(instruction_err) = syscall_error.downcast_ref::<InstructionError>()
                    {
                        stable_log::program_failure(&logger, &program_id, instruction_err);
                        Err(instruction_err.clone())
                    } else {
                        stable_log::program_failure(&logger, &program_id, syscall_error);
                        Err(InstructionError::ProgramFailedToComplete)
                    }
                } else {
                    stable_log::program_failure(&logger, &program_id, err);
                    Err(InstructionError::ProgramFailedToComplete)
                }
            }
        };
        let post_remaining_units = self.get_remaining();
        *compute_units_consumed = pre_remaining_units.saturating_sub(post_remaining_units);

        if builtin_id == program_id
            && result.is_ok()
            && *compute_units_consumed == 0
            && self
                .feature_set
                .is_active(&native_programs_consume_cu::id())
        {
            return Err(InstructionError::BuiltinProgramsMustConsumeComputeUnits);
        }

        process_executable_chain_time.stop();
        saturating_add_assign!(
            timings
                .execute_accessories
                .process_instructions
                .process_executable_chain_us,
            process_executable_chain_time.as_us()
        );
        result
    }

    /// Get this invocation's LogCollector
    pub fn get_log_collector(&self) -> Option<Rc<RefCell<LogCollector>>> {
        self.log_collector.clone()
    }

    /// Consume compute units
    pub fn consume_checked(&self, amount: u64) -> Result<(), Box<dyn std::error::Error>> {
        let mut compute_meter = self.compute_meter.borrow_mut();
        let exceeded = *compute_meter < amount;
        *compute_meter = compute_meter.saturating_sub(amount);
        if exceeded {
            return Err(Box::new(InstructionError::ComputationalBudgetExceeded));
        }
        Ok(())
    }

    /// Set compute units
    ///
    /// Only use for tests and benchmarks
    pub fn mock_set_remaining(&self, remaining: u64) {
        *self.compute_meter.borrow_mut() = remaining;
    }

    /// Get this invocation's AccountsDataMeter
    pub fn get_accounts_data_meter(&self) -> &AccountsDataMeter {
        &self.accounts_data_meter
    }

    /// Get this invocation's compute budget
    pub fn get_compute_budget(&self) -> &ComputeBudget {
        &self.current_compute_budget
    }

    /// Get cached sysvars
    pub fn get_sysvar_cache(&self) -> &SysvarCache {
        self.sysvar_cache
    }

    // Should alignment be enforced during user pointer translation
    pub fn get_check_aligned(&self) -> bool {
        self.transaction_context
            .get_current_instruction_context()
            .and_then(|instruction_context| {
                let program_account =
                    instruction_context.try_borrow_last_program_account(self.transaction_context);
                debug_assert!(program_account.is_ok());
                program_account
            })
            .map(|program_account| *program_account.get_owner() != bpf_loader_deprecated::id())
            .unwrap_or(true)
    }

    // Set should type size be checked during user pointer translation
    pub fn get_check_size(&self) -> bool {
        self.feature_set
            .is_active(&check_slice_translation_size::id())
    }

    // Set this instruction syscall context
    pub fn set_syscall_context(
        &mut self,
        syscall_context: SyscallContext,
    ) -> Result<(), InstructionError> {
        *self
            .syscall_context
            .last_mut()
            .ok_or(InstructionError::CallDepth)? = Some(syscall_context);
        Ok(())
    }

    // Get this instruction's SyscallContext
    pub fn get_syscall_context(&self) -> Result<&SyscallContext, InstructionError> {
        self.syscall_context
            .last()
            .and_then(std::option::Option::as_ref)
            .ok_or(InstructionError::CallDepth)
    }

    // Get this instruction's SyscallContext
    pub fn get_syscall_context_mut(&mut self) -> Result<&mut SyscallContext, InstructionError> {
        self.syscall_context
            .last_mut()
            .and_then(|syscall_context| syscall_context.as_mut())
            .ok_or(InstructionError::CallDepth)
    }

    /// Return a references to traces
    pub fn get_traces(&self) -> &Vec<Vec<[u64; 12]>> {
        &self.traces
    }
}

#[macro_export]
macro_rules! with_mock_invoke_context {
    (
        $invoke_context:ident,
        $transaction_context:ident,
        $transaction_accounts:expr $(,)?
    ) => {
        use {
            solana_sdk::{
                account::ReadableAccount, feature_set::FeatureSet, hash::Hash, sysvar::rent::Rent,
                transaction_context::TransactionContext,
            },
            std::sync::Arc,
            $crate::{
                compute_budget::ComputeBudget, invoke_context::InvokeContext,
                loaded_programs::LoadedProgramsForTxBatch, log_collector::LogCollector,
                sysvar_cache::SysvarCache,
            },
        };
        let compute_budget = ComputeBudget::default();
        let mut $transaction_context = TransactionContext::new(
            $transaction_accounts,
            Some(Rent::default()),
            compute_budget.max_invoke_stack_height,
            compute_budget.max_instruction_trace_length,
        );
        $transaction_context.enable_cap_accounts_data_allocations_per_transaction();
        let mut sysvar_cache = SysvarCache::default();
        sysvar_cache.fill_missing_entries(|pubkey, callback| {
            for index in 0..$transaction_context.get_number_of_accounts() {
                if $transaction_context
                    .get_key_of_account_at_index(index)
                    .unwrap()
                    == pubkey
                {
                    callback(
                        $transaction_context
                            .get_account_at_index(index)
                            .unwrap()
                            .borrow()
                            .data(),
                    );
                }
            }
        });
        let programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        let mut programs_modified_by_tx = LoadedProgramsForTxBatch::default();
        let mut programs_updated_only_for_global_cache = LoadedProgramsForTxBatch::default();
        let mut $invoke_context = InvokeContext::new(
            &mut $transaction_context,
            Rent::default(),
            &sysvar_cache,
            Some(LogCollector::new_ref()),
            compute_budget,
            &programs_loaded_for_tx_batch,
            &mut programs_modified_by_tx,
            &mut programs_updated_only_for_global_cache,
            Arc::new(FeatureSet::all_enabled()),
            Hash::default(),
            0,
            0,
        );
    };
}

pub fn mock_process_instruction<F: FnMut(&mut InvokeContext), G: FnMut(&mut InvokeContext)>(
    loader_id: &Pubkey,
    mut program_indices: Vec<IndexOfAccount>,
    instruction_data: &[u8],
    mut transaction_accounts: Vec<TransactionAccount>,
    instruction_account_metas: Vec<AccountMeta>,
    expected_result: Result<(), InstructionError>,
    builtin_function: BuiltinFunctionWithContext,
    mut pre_adjustments: F,
    mut post_adjustments: G,
) -> Vec<AccountSharedData> {
    let mut instruction_accounts: Vec<InstructionAccount> =
        Vec::with_capacity(instruction_account_metas.len());
    for (instruction_account_index, account_meta) in instruction_account_metas.iter().enumerate() {
        let index_in_transaction = transaction_accounts
            .iter()
            .position(|(key, _account)| *key == account_meta.pubkey)
            .unwrap_or(transaction_accounts.len())
            as IndexOfAccount;
        let index_in_callee = instruction_accounts
            .get(0..instruction_account_index)
            .unwrap()
            .iter()
            .position(|instruction_account| {
                instruction_account.index_in_transaction == index_in_transaction
            })
            .unwrap_or(instruction_account_index) as IndexOfAccount;
        instruction_accounts.push(InstructionAccount {
            index_in_transaction,
            index_in_caller: index_in_transaction,
            index_in_callee,
            is_signer: account_meta.is_signer,
            is_writable: account_meta.is_writable,
        });
    }
    program_indices.insert(0, transaction_accounts.len() as IndexOfAccount);
    let processor_account = AccountSharedData::new(0, 0, &native_loader::id());
    transaction_accounts.push((*loader_id, processor_account));
    with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
    let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
    programs_loaded_for_tx_batch.replenish(
        *loader_id,
        Arc::new(LoadedProgram::new_builtin(0, 0, builtin_function)),
    );
    invoke_context.programs_loaded_for_tx_batch = &programs_loaded_for_tx_batch;
    pre_adjustments(&mut invoke_context);
    let result = invoke_context.process_instruction(
        instruction_data,
        &instruction_accounts,
        &program_indices,
        &mut 0,
        &mut ExecuteTimings::default(),
    );
    assert_eq!(result, expected_result);
    post_adjustments(&mut invoke_context);
    let mut transaction_accounts = transaction_context.deconstruct_without_keys().unwrap();
    transaction_accounts.pop();
    transaction_accounts
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::compute_budget,
        serde::{Deserialize, Serialize},
        solana_sdk::{account::WritableAccount, instruction::Instruction},
    };

    #[derive(Debug, Serialize, Deserialize)]
    enum MockInstruction {
        NoopSuccess,
        NoopFail,
        ModifyOwned,
        ModifyNotOwned,
        ModifyReadonly,
        UnbalancedPush,
        UnbalancedPop,
        ConsumeComputeUnits {
            compute_units_to_consume: u64,
            desired_result: Result<(), InstructionError>,
        },
        Resize {
            new_len: u64,
        },
    }

    const MOCK_BUILTIN_COMPUTE_UNIT_COST: u64 = 1;

    declare_process_instruction!(
        MockBuiltin,
        MOCK_BUILTIN_COMPUTE_UNIT_COST,
        |invoke_context| {
            let transaction_context = &invoke_context.transaction_context;
            let instruction_context = transaction_context.get_current_instruction_context()?;
            let instruction_data = instruction_context.get_instruction_data();
            let program_id = instruction_context.get_last_program_key(transaction_context)?;
            let instruction_accounts = (0..4)
                .map(|instruction_account_index| InstructionAccount {
                    index_in_transaction: instruction_account_index,
                    index_in_caller: instruction_account_index,
                    index_in_callee: instruction_account_index,
                    is_signer: false,
                    is_writable: false,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                program_id,
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 0)?
                    .get_owner()
            );
            assert_ne!(
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 1)?
                    .get_owner(),
                instruction_context
                    .try_borrow_instruction_account(transaction_context, 0)?
                    .get_key()
            );

            if let Ok(instruction) = bincode::deserialize(instruction_data) {
                match instruction {
                    MockInstruction::NoopSuccess => (),
                    MockInstruction::NoopFail => return Err(InstructionError::GenericError),
                    MockInstruction::ModifyOwned => instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .set_data_from_slice(&[1])?,
                    MockInstruction::ModifyNotOwned => instruction_context
                        .try_borrow_instruction_account(transaction_context, 1)?
                        .set_data_from_slice(&[1])?,
                    MockInstruction::ModifyReadonly => instruction_context
                        .try_borrow_instruction_account(transaction_context, 2)?
                        .set_data_from_slice(&[1])?,
                    MockInstruction::UnbalancedPush => {
                        instruction_context
                            .try_borrow_instruction_account(transaction_context, 0)?
                            .checked_add_lamports(1)?;
                        let program_id = *transaction_context.get_key_of_account_at_index(3)?;
                        let metas = vec![
                            AccountMeta::new_readonly(
                                *transaction_context.get_key_of_account_at_index(0)?,
                                false,
                            ),
                            AccountMeta::new_readonly(
                                *transaction_context.get_key_of_account_at_index(1)?,
                                false,
                            ),
                        ];
                        let inner_instruction = Instruction::new_with_bincode(
                            program_id,
                            &MockInstruction::NoopSuccess,
                            metas,
                        );
                        invoke_context
                            .transaction_context
                            .get_next_instruction_context()
                            .unwrap()
                            .configure(&[3], &instruction_accounts, &[]);
                        let result = invoke_context.push();
                        assert_eq!(result, Err(InstructionError::UnbalancedInstruction));
                        result?;
                        invoke_context
                            .native_invoke(inner_instruction.into(), &[])
                            .and(invoke_context.pop())?;
                    }
                    MockInstruction::UnbalancedPop => instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .checked_add_lamports(1)?,
                    MockInstruction::ConsumeComputeUnits {
                        compute_units_to_consume,
                        desired_result,
                    } => {
                        invoke_context
                            .consume_checked(compute_units_to_consume)
                            .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
                        return desired_result;
                    }
                    MockInstruction::Resize { new_len } => instruction_context
                        .try_borrow_instruction_account(transaction_context, 0)?
                        .set_data(vec![0; new_len as usize])?,
                }
            } else {
                return Err(InstructionError::InvalidInstructionData);
            }
            Ok(())
        }
    );

    #[test]
    fn test_instruction_stack_height() {
        let one_more_than_max_depth = ComputeBudget::default()
            .max_invoke_stack_height
            .saturating_add(1);
        let mut invoke_stack = vec![];
        let mut transaction_accounts = vec![];
        let mut instruction_accounts = vec![];
        for index in 0..one_more_than_max_depth {
            invoke_stack.push(solana_sdk::pubkey::new_rand());
            transaction_accounts.push((
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new(index as u64, 1, invoke_stack.get(index).unwrap()),
            ));
            instruction_accounts.push(InstructionAccount {
                index_in_transaction: index as IndexOfAccount,
                index_in_caller: index as IndexOfAccount,
                index_in_callee: instruction_accounts.len() as IndexOfAccount,
                is_signer: false,
                is_writable: true,
            });
        }
        for (index, program_id) in invoke_stack.iter().enumerate() {
            transaction_accounts.push((
                *program_id,
                AccountSharedData::new(1, 1, &solana_sdk::pubkey::Pubkey::default()),
            ));
            instruction_accounts.push(InstructionAccount {
                index_in_transaction: index as IndexOfAccount,
                index_in_caller: index as IndexOfAccount,
                index_in_callee: index as IndexOfAccount,
                is_signer: false,
                is_writable: false,
            });
        }
        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);

        // Check call depth increases and has a limit
        let mut depth_reached = 0;
        for _ in 0..invoke_stack.len() {
            invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(
                    &[one_more_than_max_depth.saturating_add(depth_reached) as IndexOfAccount],
                    &instruction_accounts,
                    &[],
                );
            if Err(InstructionError::CallDepth) == invoke_context.push() {
                break;
            }
            depth_reached = depth_reached.saturating_add(1);
        }
        assert_ne!(depth_reached, 0);
        assert!(depth_reached < one_more_than_max_depth);
    }

    #[test]
    fn test_max_instruction_trace_length() {
        const MAX_INSTRUCTIONS: usize = 8;
        let mut transaction_context =
            TransactionContext::new(Vec::new(), Some(Rent::default()), 1, MAX_INSTRUCTIONS);
        for _ in 0..MAX_INSTRUCTIONS {
            transaction_context.push().unwrap();
            transaction_context.pop().unwrap();
        }
        assert_eq!(
            transaction_context.push(),
            Err(InstructionError::MaxInstructionTraceLengthExceeded)
        );
    }

    #[test]
    fn test_process_instruction() {
        let callee_program_id = solana_sdk::pubkey::new_rand();
        let owned_account = AccountSharedData::new(42, 1, &callee_program_id);
        let not_owned_account = AccountSharedData::new(84, 1, &solana_sdk::pubkey::new_rand());
        let readonly_account = AccountSharedData::new(168, 1, &solana_sdk::pubkey::new_rand());
        let loader_account = AccountSharedData::new(0, 0, &native_loader::id());
        let mut program_account = AccountSharedData::new(1, 0, &native_loader::id());
        program_account.set_executable(true);
        let transaction_accounts = vec![
            (solana_sdk::pubkey::new_rand(), owned_account),
            (solana_sdk::pubkey::new_rand(), not_owned_account),
            (solana_sdk::pubkey::new_rand(), readonly_account),
            (callee_program_id, program_account),
            (solana_sdk::pubkey::new_rand(), loader_account),
        ];
        let metas = vec![
            AccountMeta::new(transaction_accounts.get(0).unwrap().0, false),
            AccountMeta::new(transaction_accounts.get(1).unwrap().0, false),
            AccountMeta::new_readonly(transaction_accounts.get(2).unwrap().0, false),
        ];
        let instruction_accounts = (0..4)
            .map(|instruction_account_index| InstructionAccount {
                index_in_transaction: instruction_account_index,
                index_in_caller: instruction_account_index,
                index_in_callee: instruction_account_index,
                is_signer: false,
                is_writable: instruction_account_index < 2,
            })
            .collect::<Vec<_>>();
        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
        let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        programs_loaded_for_tx_batch.replenish(
            callee_program_id,
            Arc::new(LoadedProgram::new_builtin(0, 0, MockBuiltin::vm)),
        );
        invoke_context.programs_loaded_for_tx_batch = &programs_loaded_for_tx_batch;

        // Account modification tests
        let cases = vec![
            (MockInstruction::NoopSuccess, Ok(())),
            (
                MockInstruction::NoopFail,
                Err(InstructionError::GenericError),
            ),
            (MockInstruction::ModifyOwned, Ok(())),
            (
                MockInstruction::ModifyNotOwned,
                Err(InstructionError::ExternalAccountDataModified),
            ),
            (
                MockInstruction::ModifyReadonly,
                Err(InstructionError::ReadonlyDataModified),
            ),
            (
                MockInstruction::UnbalancedPush,
                Err(InstructionError::UnbalancedInstruction),
            ),
            (
                MockInstruction::UnbalancedPop,
                Err(InstructionError::UnbalancedInstruction),
            ),
        ];
        for case in cases {
            invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(&[4], &instruction_accounts, &[]);
            invoke_context.push().unwrap();
            let inner_instruction =
                Instruction::new_with_bincode(callee_program_id, &case.0, metas.clone());
            let result = invoke_context
                .native_invoke(inner_instruction.into(), &[])
                .and(invoke_context.pop());
            assert_eq!(result, case.1);
        }

        // Compute unit consumption tests
        let compute_units_to_consume = 10;
        let expected_results = vec![Ok(()), Err(InstructionError::GenericError)];
        for expected_result in expected_results {
            invoke_context
                .transaction_context
                .get_next_instruction_context()
                .unwrap()
                .configure(&[4], &instruction_accounts, &[]);
            invoke_context.push().unwrap();
            let inner_instruction = Instruction::new_with_bincode(
                callee_program_id,
                &MockInstruction::ConsumeComputeUnits {
                    compute_units_to_consume,
                    desired_result: expected_result.clone(),
                },
                metas.clone(),
            );
            let inner_instruction = StableInstruction::from(inner_instruction);
            let (inner_instruction_accounts, program_indices) = invoke_context
                .prepare_instruction(&inner_instruction, &[])
                .unwrap();

            let mut compute_units_consumed = 0;
            let result = invoke_context.process_instruction(
                &inner_instruction.data,
                &inner_instruction_accounts,
                &program_indices,
                &mut compute_units_consumed,
                &mut ExecuteTimings::default(),
            );

            // Because the instruction had compute cost > 0, then regardless of the execution result,
            // the number of compute units consumed should be a non-default which is something greater
            // than zero.
            assert!(compute_units_consumed > 0);
            assert_eq!(
                compute_units_consumed,
                compute_units_to_consume.saturating_add(MOCK_BUILTIN_COMPUTE_UNIT_COST),
            );
            assert_eq!(result, expected_result);

            invoke_context.pop().unwrap();
        }
    }

    #[test]
    fn test_invoke_context_compute_budget() {
        let transaction_accounts =
            vec![(solana_sdk::pubkey::new_rand(), AccountSharedData::default())];

        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
        invoke_context.compute_budget =
            ComputeBudget::new(compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64);

        invoke_context
            .transaction_context
            .get_next_instruction_context()
            .unwrap()
            .configure(&[0], &[], &[]);
        invoke_context.push().unwrap();
        assert_eq!(
            *invoke_context.get_compute_budget(),
            ComputeBudget::new(compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64)
        );
        invoke_context.pop().unwrap();
    }

    #[test]
    fn test_process_instruction_accounts_resize_delta() {
        let program_key = Pubkey::new_unique();
        let user_account_data_len = 123u64;
        let user_account =
            AccountSharedData::new(100, user_account_data_len as usize, &program_key);
        let dummy_account = AccountSharedData::new(10, 0, &program_key);
        let mut program_account = AccountSharedData::new(500, 500, &native_loader::id());
        program_account.set_executable(true);
        let transaction_accounts = vec![
            (Pubkey::new_unique(), user_account),
            (Pubkey::new_unique(), dummy_account),
            (program_key, program_account),
        ];
        let instruction_accounts = [
            InstructionAccount {
                index_in_transaction: 0,
                index_in_caller: 0,
                index_in_callee: 0,
                is_signer: false,
                is_writable: true,
            },
            InstructionAccount {
                index_in_transaction: 1,
                index_in_caller: 1,
                index_in_callee: 1,
                is_signer: false,
                is_writable: false,
            },
        ];
        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
        let mut programs_loaded_for_tx_batch = LoadedProgramsForTxBatch::default();
        programs_loaded_for_tx_batch.replenish(
            program_key,
            Arc::new(LoadedProgram::new_builtin(0, 0, MockBuiltin::vm)),
        );
        invoke_context.programs_loaded_for_tx_batch = &programs_loaded_for_tx_batch;

        // Test: Resize the account to *the same size*, so not consuming any additional size; this must succeed
        {
            let resize_delta: i64 = 0;
            let new_len = (user_account_data_len as i64).saturating_add(resize_delta) as u64;
            let instruction_data =
                bincode::serialize(&MockInstruction::Resize { new_len }).unwrap();

            let result = invoke_context.process_instruction(
                &instruction_data,
                &instruction_accounts,
                &[2],
                &mut 0,
                &mut ExecuteTimings::default(),
            );

            assert!(result.is_ok());
            assert_eq!(
                invoke_context
                    .transaction_context
                    .accounts_resize_delta()
                    .unwrap(),
                resize_delta
            );
        }

        // Test: Resize the account larger; this must succeed
        {
            let resize_delta: i64 = 1;
            let new_len = (user_account_data_len as i64).saturating_add(resize_delta) as u64;
            let instruction_data =
                bincode::serialize(&MockInstruction::Resize { new_len }).unwrap();

            let result = invoke_context.process_instruction(
                &instruction_data,
                &instruction_accounts,
                &[2],
                &mut 0,
                &mut ExecuteTimings::default(),
            );

            assert!(result.is_ok());
            assert_eq!(
                invoke_context
                    .transaction_context
                    .accounts_resize_delta()
                    .unwrap(),
                resize_delta
            );
        }

        // Test: Resize the account smaller; this must succeed
        {
            let resize_delta: i64 = -1;
            let new_len = (user_account_data_len as i64).saturating_add(resize_delta) as u64;
            let instruction_data =
                bincode::serialize(&MockInstruction::Resize { new_len }).unwrap();

            let result = invoke_context.process_instruction(
                &instruction_data,
                &instruction_accounts,
                &[2],
                &mut 0,
                &mut ExecuteTimings::default(),
            );

            assert!(result.is_ok());
            assert_eq!(
                invoke_context
                    .transaction_context
                    .accounts_resize_delta()
                    .unwrap(),
                resize_delta
            );
        }
    }
}
