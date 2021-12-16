use {
    crate::{
        ic_logger_msg, ic_msg, instruction_recorder::InstructionRecorder,
        log_collector::LogCollector, native_loader::NativeLoader, pre_account::PreAccount,
        timings::ExecuteDetailsTimings,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        compute_budget::ComputeBudget,
        feature_set::{
            do_support_realloc, neon_evm_compute_budget, reject_empty_instruction_without_program,
            remove_native_loader, requestable_heap_size, tx_wide_compute_cap, FeatureSet,
        },
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
        keyed_account::{create_keyed_accounts_unified, keyed_account_at_index, KeyedAccount},
        message::Message,
        pubkey::Pubkey,
        rent::Rent,
        sysvar::Sysvar,
    },
    std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc, sync::Arc},
};

pub type TransactionAccountRefCell = (Pubkey, Rc<RefCell<AccountSharedData>>);
pub type TransactionAccountRefCells = Vec<TransactionAccountRefCell>;

pub type ProcessInstructionWithContext =
    fn(usize, &[u8], &mut InvokeContext) -> Result<(), InstructionError>;

#[derive(Clone)]
pub struct BuiltinProgram {
    pub program_id: Pubkey,
    pub process_instruction: ProcessInstructionWithContext,
}

impl std::fmt::Debug for BuiltinProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // These are just type aliases for work around of Debug-ing above pointers
        type ErasedProcessInstructionWithContext = fn(
            usize,
            &'static [u8],
            &'static mut InvokeContext<'static>,
        ) -> Result<(), InstructionError>;

        // rustc doesn't compile due to bug without this work around
        // https://github.com/rust-lang/rust/issues/50280
        // https://users.rust-lang.org/t/display-function-pointer/17073/2
        let erased_instruction: ErasedProcessInstructionWithContext = self.process_instruction;
        write!(f, "{}: {:p}", self.program_id, erased_instruction)
    }
}

/// Program executor
pub trait Executor: Debug + Send + Sync {
    /// Execute the program
    fn execute<'a, 'b>(
        &self,
        first_instruction_account: usize,
        instruction_data: &[u8],
        invoke_context: &'a mut InvokeContext<'b>,
        use_jit: bool,
    ) -> Result<(), InstructionError>;
}

#[derive(Default)]
pub struct Executors {
    pub executors: HashMap<Pubkey, Arc<dyn Executor>>,
    pub is_dirty: bool,
}
impl Executors {
    pub fn insert(&mut self, key: Pubkey, executor: Arc<dyn Executor>) {
        let _ = self.executors.insert(key, executor);
        self.is_dirty = true;
    }
    pub fn get(&self, key: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.executors.get(key).cloned()
    }
}

/// Compute meter
pub struct ComputeMeter {
    remaining: u64,
}
impl ComputeMeter {
    /// Consume compute units
    pub fn consume(&mut self, amount: u64) -> Result<(), InstructionError> {
        let exceeded = self.remaining < amount;
        self.remaining = self.remaining.saturating_sub(amount);
        if exceeded {
            return Err(InstructionError::ComputationalBudgetExceeded);
        }
        Ok(())
    }
    /// Get the number of remaining compute units
    pub fn get_remaining(&self) -> u64 {
        self.remaining
    }
    /// Set compute units
    ///
    /// Only use for tests and benchmarks
    pub fn mock_set_remaining(&mut self, remaining: u64) {
        self.remaining = remaining;
    }
    /// Construct a new one with the given remaining units
    pub fn new_ref(remaining: u64) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { remaining }))
    }
}

pub struct StackFrame<'a> {
    pub number_of_program_accounts: usize,
    pub keyed_accounts: Vec<KeyedAccount<'a>>,
    pub keyed_accounts_range: std::ops::Range<usize>,
}

impl<'a> StackFrame<'a> {
    pub fn new(number_of_program_accounts: usize, keyed_accounts: Vec<KeyedAccount<'a>>) -> Self {
        let keyed_accounts_range = std::ops::Range {
            start: 0,
            end: keyed_accounts.len(),
        };
        Self {
            number_of_program_accounts,
            keyed_accounts,
            keyed_accounts_range,
        }
    }

    pub fn program_id(&self) -> Option<&Pubkey> {
        self.keyed_accounts
            .get(self.number_of_program_accounts.saturating_sub(1))
            .map(|keyed_account| keyed_account.unsigned_key())
    }
}

pub struct InvokeContext<'a> {
    invoke_stack: Vec<StackFrame<'a>>,
    rent: Rent,
    pre_accounts: Vec<PreAccount>,
    accounts: &'a [TransactionAccountRefCell],
    builtin_programs: &'a [BuiltinProgram],
    pub sysvars: &'a [(Pubkey, Vec<u8>)],
    log_collector: Option<Rc<RefCell<LogCollector>>>,
    compute_budget: ComputeBudget,
    current_compute_budget: ComputeBudget,
    compute_meter: Rc<RefCell<ComputeMeter>>,
    executors: Rc<RefCell<Executors>>,
    pub instruction_recorder: Option<&'a InstructionRecorder>,
    pub feature_set: Arc<FeatureSet>,
    pub timings: ExecuteDetailsTimings,
    pub blockhash: Hash,
    pub lamports_per_signature: u64,
    pub return_data: (Pubkey, Vec<u8>),
}

impl<'a> InvokeContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rent: Rent,
        accounts: &'a [TransactionAccountRefCell],
        builtin_programs: &'a [BuiltinProgram],
        sysvars: &'a [(Pubkey, Vec<u8>)],
        log_collector: Option<Rc<RefCell<LogCollector>>>,
        compute_budget: ComputeBudget,
        executors: Rc<RefCell<Executors>>,
        feature_set: Arc<FeatureSet>,
        blockhash: Hash,
        lamports_per_signature: u64,
    ) -> Self {
        Self {
            invoke_stack: Vec::with_capacity(compute_budget.max_invoke_depth),
            rent,
            pre_accounts: Vec::new(),
            accounts,
            builtin_programs,
            sysvars,
            log_collector,
            current_compute_budget: compute_budget,
            compute_budget,
            compute_meter: ComputeMeter::new_ref(compute_budget.max_units),
            executors,
            instruction_recorder: None,
            feature_set,
            timings: ExecuteDetailsTimings::default(),
            blockhash,
            lamports_per_signature,
            return_data: (Pubkey::default(), Vec::new()),
        }
    }

    pub fn new_mock(
        accounts: &'a [TransactionAccountRefCell],
        builtin_programs: &'a [BuiltinProgram],
    ) -> Self {
        Self::new(
            Rent::default(),
            accounts,
            builtin_programs,
            &[],
            Some(LogCollector::new_ref()),
            ComputeBudget::default(),
            Rc::new(RefCell::new(Executors::default())),
            Arc::new(FeatureSet::all_enabled()),
            Hash::default(),
            0,
        )
    }

    /// Push a stack frame onto the invocation stack
    pub fn push(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        account_indices: &[usize],
    ) -> Result<(), InstructionError> {
        if self.invoke_stack.len() > self.compute_budget.max_invoke_depth {
            return Err(InstructionError::CallDepth);
        }

        let program_id = program_indices
            .last()
            .map(|index_of_program_id| &self.accounts[*index_of_program_id].0);
        if program_id.is_none()
            && self
                .feature_set
                .is_active(&reject_empty_instruction_without_program::id())
        {
            return Err(InstructionError::UnsupportedProgramId);
        }
        if self.invoke_stack.is_empty() {
            let mut compute_budget = self.compute_budget;
            if !self.feature_set.is_active(&tx_wide_compute_cap::id())
                && self.feature_set.is_active(&neon_evm_compute_budget::id())
                && program_id == Some(&crate::neon_evm_program::id())
            {
                // Bump the compute budget for neon_evm
                compute_budget.max_units = compute_budget.max_units.max(500_000);
            }
            if !self.feature_set.is_active(&requestable_heap_size::id())
                && self.feature_set.is_active(&neon_evm_compute_budget::id())
                && program_id == Some(&crate::neon_evm_program::id())
            {
                // Bump the compute budget for neon_evm
                compute_budget.heap_size = Some(256_usize.saturating_mul(1024));
            }
            self.current_compute_budget = compute_budget;

            if !self.feature_set.is_active(&tx_wide_compute_cap::id()) {
                self.compute_meter = ComputeMeter::new_ref(self.current_compute_budget.max_units);
            }

            self.pre_accounts = Vec::with_capacity(instruction.accounts.len());
            let mut work = |_unique_index: usize, account_index: usize| {
                if account_index < self.accounts.len() {
                    let account = self.accounts[account_index].1.borrow().clone();
                    self.pre_accounts
                        .push(PreAccount::new(&self.accounts[account_index].0, account));
                    return Ok(());
                }
                Err(InstructionError::MissingAccount)
            };
            instruction.visit_each_account(&mut work)?;
        } else {
            let contains = self
                .invoke_stack
                .iter()
                .any(|frame| frame.program_id() == program_id);
            let is_last = if let Some(last_frame) = self.invoke_stack.last() {
                last_frame.program_id() == program_id
            } else {
                false
            };
            if contains && !is_last {
                // Reentrancy not allowed unless caller is calling itself
                return Err(InstructionError::ReentrancyNotAllowed);
            }
        }

        // Create the KeyedAccounts that will be passed to the program
        let keyed_accounts = program_indices
            .iter()
            .map(|account_index| {
                (
                    false,
                    false,
                    &self.accounts[*account_index].0,
                    &self.accounts[*account_index].1 as &RefCell<AccountSharedData>,
                )
            })
            .chain(instruction.accounts.iter().map(|index_in_instruction| {
                let index_in_instruction = *index_in_instruction as usize;
                let account_index = if account_indices.is_empty() {
                    index_in_instruction
                } else {
                    account_indices[index_in_instruction]
                };
                (
                    message.is_signer(index_in_instruction),
                    message.is_writable(index_in_instruction),
                    &self.accounts[account_index].0,
                    &self.accounts[account_index].1 as &RefCell<AccountSharedData>,
                )
            }))
            .collect::<Vec<_>>();

        self.invoke_stack.push(StackFrame::new(
            program_indices.len(),
            create_keyed_accounts_unified(keyed_accounts.as_slice()),
        ));
        Ok(())
    }

    /// Pop a stack frame from the invocation stack
    pub fn pop(&mut self) {
        self.invoke_stack.pop();
    }

    /// Current depth of the invocation stack
    pub fn invoke_depth(&self) -> usize {
        self.invoke_stack.len()
    }

    /// Verify the results of an instruction
    fn verify(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
    ) -> Result<(), InstructionError> {
        let program_id = instruction.program_id(&message.account_keys);
        let do_support_realloc = self.feature_set.is_active(&do_support_realloc::id());

        // Verify all executable accounts have zero outstanding refs
        for account_index in program_indices.iter() {
            self.accounts[*account_index]
                .1
                .try_borrow_mut()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
        }

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        let mut work = |unique_index: usize, account_index: usize| {
            {
                // Verify account has no outstanding references
                let _ = self.accounts[account_index]
                    .1
                    .try_borrow_mut()
                    .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
            }
            let pre_account = &self.pre_accounts[unique_index];
            let account = self.accounts[account_index].1.borrow();
            pre_account
                .verify(
                    program_id,
                    message.is_writable(account_index),
                    &self.rent,
                    &account,
                    &mut self.timings,
                    true,
                    do_support_realloc,
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
            Ok(())
        };
        instruction.visit_each_account(&mut work)?;

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }

    /// Verify and update PreAccount state based on program execution
    fn verify_and_update(
        &mut self,
        instruction: &CompiledInstruction,
        account_indices: &[usize],
        write_privileges: &[bool],
    ) -> Result<(), InstructionError> {
        let do_support_realloc = self.feature_set.is_active(&do_support_realloc::id());
        let program_id = self
            .invoke_stack
            .last()
            .and_then(|frame| frame.program_id())
            .ok_or(InstructionError::CallDepth)?;
        let rent = &self.rent;
        let log_collector = &self.log_collector;
        let accounts = &self.accounts;
        let pre_accounts = &mut self.pre_accounts;
        let timings = &mut self.timings;

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        let mut work = |_unique_index: usize, index_in_instruction: usize| {
            if index_in_instruction < write_privileges.len()
                && index_in_instruction < account_indices.len()
            {
                let account_index = account_indices[index_in_instruction];
                let (key, account) = &accounts[account_index];
                let is_writable = write_privileges[index_in_instruction];
                // Find the matching PreAccount
                for pre_account in pre_accounts.iter_mut() {
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
                                rent,
                                &account,
                                timings,
                                false,
                                do_support_realloc,
                            )
                            .map_err(|err| {
                                ic_logger_msg!(
                                    log_collector,
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
                        return Ok(());
                    }
                }
            }
            Err(InstructionError::MissingAccount)
        };
        instruction.visit_each_account(&mut work)?;

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }

    /// Entrypoint for a cross-program invocation from a builtin program
    pub fn native_invoke(
        &mut self,
        instruction: Instruction,
        signers: &[Pubkey],
    ) -> Result<(), InstructionError> {
        let (message, caller_write_privileges, program_indices) =
            self.create_message(&instruction, signers)?;
        let mut account_indices = Vec::with_capacity(message.account_keys.len());
        let mut prev_account_sizes = Vec::with_capacity(message.account_keys.len());
        for account_key in message.account_keys.iter() {
            let account_index = self
                .find_index_of_account(account_key)
                .ok_or(InstructionError::MissingAccount)?;
            let account_length = self.accounts[account_index].1.borrow().data().len();
            account_indices.push(account_index);
            prev_account_sizes.push((account_index, account_length));
        }

        if let Some(instruction_recorder) = &self.instruction_recorder {
            instruction_recorder.record_instruction(instruction);
        }
        self.process_instruction(
            &message,
            &message.instructions[0],
            &program_indices,
            &account_indices,
            &caller_write_privileges,
        )?;

        // Verify the called program has not misbehaved
        let do_support_realloc = self.feature_set.is_active(&do_support_realloc::id());
        for (account_index, prev_size) in prev_account_sizes.into_iter() {
            if !do_support_realloc
                && prev_size != self.accounts[account_index].1.borrow().data().len()
                && prev_size != 0
            {
                // Only support for `CreateAccount` at this time.
                // Need a way to limit total realloc size across multiple CPI calls
                ic_msg!(
                    self,
                    "Inner instructions do not support realloc, only SystemProgram::CreateAccount",
                );
                return Err(InstructionError::InvalidRealloc);
            }
        }

        Ok(())
    }

    /// Helper to prepare for process_instruction()
    pub fn create_message(
        &mut self,
        instruction: &Instruction,
        signers: &[Pubkey],
    ) -> Result<(Message, Vec<bool>, Vec<usize>), InstructionError> {
        let message = Message::new(&[instruction.clone()], None);

        // Gather keyed_accounts in the order of message.account_keys
        let caller_keyed_accounts = self.get_instruction_keyed_accounts()?;
        let callee_keyed_accounts = message
            .account_keys
            .iter()
            .map(|account_key| {
                caller_keyed_accounts
                    .iter()
                    .find(|keyed_account| keyed_account.unsigned_key() == account_key)
                    .ok_or_else(|| {
                        ic_msg!(
                            self,
                            "Instruction references an unknown account {}",
                            account_key
                        );
                        InstructionError::MissingAccount
                    })
            })
            .collect::<Result<Vec<_>, InstructionError>>()?;

        // Check for privilege escalation
        for account in instruction.accounts.iter() {
            let keyed_account = callee_keyed_accounts
                .iter()
                .find_map(|keyed_account| {
                    if &account.pubkey == keyed_account.unsigned_key() {
                        Some(keyed_account)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    ic_msg!(
                        self,
                        "Instruction references an unknown account {}",
                        account.pubkey
                    );
                    InstructionError::MissingAccount
                })?;
            // Readonly account cannot become writable
            if account.is_writable && !keyed_account.is_writable() {
                ic_msg!(self, "{}'s writable privilege escalated", account.pubkey);
                return Err(InstructionError::PrivilegeEscalation);
            }

            if account.is_signer && // If message indicates account is signed
            !( // one of the following needs to be true:
                keyed_account.signer_key().is_some() // Signed in the parent instruction
                || signers.contains(&account.pubkey) // Signed by the program
            ) {
                ic_msg!(self, "{}'s signer privilege escalated", account.pubkey);
                return Err(InstructionError::PrivilegeEscalation);
            }
        }
        let caller_write_privileges = callee_keyed_accounts
            .iter()
            .map(|keyed_account| keyed_account.is_writable())
            .collect::<Vec<bool>>();

        // Find and validate executables / program accounts
        let callee_program_id = instruction.program_id;
        let program_account_index = callee_keyed_accounts
            .iter()
            .find(|keyed_account| &callee_program_id == keyed_account.unsigned_key())
            .and_then(|_keyed_account| self.find_index_of_account(&callee_program_id))
            .ok_or_else(|| {
                ic_msg!(self, "Unknown program {}", callee_program_id);
                InstructionError::MissingAccount
            })?;
        let program_account = self.accounts[program_account_index].1.borrow();
        if !program_account.executable() {
            ic_msg!(self, "Account {} is not executable", callee_program_id);
            return Err(InstructionError::AccountNotExecutable);
        }
        let mut program_indices = vec![];
        if program_account.owner() == &bpf_loader_upgradeable::id() {
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program_account.state()?
            {
                if let Some(programdata_account_index) =
                    self.find_index_of_account(&programdata_address)
                {
                    program_indices.push(programdata_account_index);
                } else {
                    ic_msg!(
                        self,
                        "Unknown upgradeable programdata account {}",
                        programdata_address,
                    );
                    return Err(InstructionError::MissingAccount);
                }
            } else {
                ic_msg!(
                    self,
                    "Invalid upgradeable program account {}",
                    callee_program_id,
                );
                return Err(InstructionError::MissingAccount);
            }
        }
        program_indices.push(program_account_index);

        Ok((message, caller_write_privileges, program_indices))
    }

    /// Processes a cross-program instruction and returns how many compute units were used
    pub fn process_instruction(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        account_indices: &[usize],
        caller_write_privileges: &[bool],
    ) -> Result<u64, InstructionError> {
        let is_lowest_invocation_level = self.invoke_stack.is_empty();
        if !is_lowest_invocation_level {
            // Verify the calling program hasn't misbehaved
            self.verify_and_update(instruction, account_indices, caller_write_privileges)?;
        }

        let result = self
            .push(message, instruction, program_indices, account_indices)
            .and_then(|_| {
                self.return_data = (*instruction.program_id(&message.account_keys), Vec::new());
                let pre_remaining_units = self.compute_meter.borrow().get_remaining();
                self.process_executable_chain(&instruction.data)?;
                let post_remaining_units = self.compute_meter.borrow().get_remaining();

                // Verify the called program has not misbehaved
                if is_lowest_invocation_level {
                    self.verify(message, instruction, program_indices)?;
                } else {
                    let write_privileges: Vec<bool> = (0..message.account_keys.len())
                        .map(|i| message.is_writable(i))
                        .collect();
                    self.verify_and_update(instruction, account_indices, &write_privileges)?;
                }

                Ok(pre_remaining_units.saturating_sub(post_remaining_units))
            });

        // Pop the invoke_stack to restore previous state
        self.pop();
        result
    }

    /// Calls the instruction's program entrypoint method
    fn process_executable_chain(
        &mut self,
        instruction_data: &[u8],
    ) -> Result<(), InstructionError> {
        let keyed_accounts = self.get_keyed_accounts()?;
        let root_account = keyed_account_at_index(keyed_accounts, 0)
            .map_err(|_| InstructionError::UnsupportedProgramId)?;
        let root_id = root_account.unsigned_key();
        let owner_id = &root_account.owner()?;
        if solana_sdk::native_loader::check_id(owner_id) {
            for entry in self.builtin_programs {
                if entry.program_id == *root_id {
                    // Call the builtin program
                    return (entry.process_instruction)(
                        1, // root_id to be skipped
                        instruction_data,
                        self,
                    );
                }
            }
            if !self.feature_set.is_active(&remove_native_loader::id()) {
                let native_loader = NativeLoader::default();
                // Call the program via the native loader
                return native_loader.process_instruction(0, instruction_data, self);
            }
        } else {
            for entry in self.builtin_programs {
                if entry.program_id == *owner_id {
                    // Call the program via a builtin loader
                    return (entry.process_instruction)(
                        0, // no root_id was provided
                        instruction_data,
                        self,
                    );
                }
            }
        }
        Err(InstructionError::UnsupportedProgramId)
    }

    /// Get the program ID of the currently executing program
    pub fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        self.invoke_stack
            .last()
            .and_then(|frame| frame.program_id())
            .ok_or(InstructionError::CallDepth)
    }

    /// Get the owner of the currently executing program
    pub fn get_loader(&self) -> Result<Pubkey, InstructionError> {
        self.get_instruction_keyed_accounts()
            .and_then(|keyed_accounts| keyed_accounts.first().ok_or(InstructionError::CallDepth))
            .and_then(|keyed_account| keyed_account.owner())
    }

    /// Removes the first keyed account
    #[deprecated(
        since = "1.9.0",
        note = "To be removed together with remove_native_loader"
    )]
    pub fn remove_first_keyed_account(&mut self) -> Result<(), InstructionError> {
        if !self.feature_set.is_active(&remove_native_loader::id()) {
            let stack_frame = &mut self
                .invoke_stack
                .last_mut()
                .ok_or(InstructionError::CallDepth)?;
            stack_frame.keyed_accounts_range.start =
                stack_frame.keyed_accounts_range.start.saturating_add(1);
        }
        Ok(())
    }

    /// Get the list of keyed accounts including the chain of program accounts
    pub fn get_keyed_accounts(&self) -> Result<&[KeyedAccount], InstructionError> {
        self.invoke_stack
            .last()
            .map(|frame| &frame.keyed_accounts[frame.keyed_accounts_range.clone()])
            .ok_or(InstructionError::CallDepth)
    }

    /// Get the list of keyed accounts without the chain of program accounts
    ///
    /// Note: The `KeyedAccount` at index `0` has the key `program_id` and
    /// is followed by the `KeyedAccount`s passed by the caller.
    pub fn get_instruction_keyed_accounts(&self) -> Result<&[KeyedAccount], InstructionError> {
        let frame = self
            .invoke_stack
            .last()
            .ok_or(InstructionError::CallDepth)?;
        let first_instruction_account = frame
            .number_of_program_accounts
            .checked_sub(1)
            .ok_or(InstructionError::CallDepth)?;
        Ok(&frame.keyed_accounts[first_instruction_account..])
    }

    /// Get this invocation's LogCollector
    pub fn get_log_collector(&self) -> Option<Rc<RefCell<LogCollector>>> {
        self.log_collector.clone()
    }

    /// Get this invocation's ComputeMeter
    pub fn get_compute_meter(&self) -> Rc<RefCell<ComputeMeter>> {
        self.compute_meter.clone()
    }

    /// Loaders may need to do work in order to execute a program. Cache
    /// the work that can be re-used across executions
    pub fn add_executor(&self, pubkey: &Pubkey, executor: Arc<dyn Executor>) {
        self.executors.borrow_mut().insert(*pubkey, executor);
    }

    /// Get the completed loader work that can be re-used across execution
    pub fn get_executor(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.executors.borrow().get(pubkey)
    }

    /// Finds an account_index by its key
    pub fn find_index_of_account(&self, pubkey: &Pubkey) -> Option<usize> {
        for (index, (key, _account)) in self.accounts.iter().enumerate().rev() {
            if key == pubkey {
                return Some(index);
            }
        }
        None
    }

    /// Returns an account by its account_index
    pub fn get_account_at_index(&self, account_index: usize) -> &RefCell<AccountSharedData> {
        &self.accounts[account_index].1
    }

    /// Get this invocation's compute budget
    pub fn get_compute_budget(&self) -> &ComputeBudget {
        &self.current_compute_budget
    }

    /// Get the value of a sysvar by its id
    pub fn get_sysvar<T: Sysvar>(&self, id: &Pubkey) -> Result<T, InstructionError> {
        self.sysvars
            .iter()
            .find_map(|(key, data)| {
                if id == key {
                    bincode::deserialize(data).ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ic_msg!(self, "Unable to get sysvar {}", id);
                InstructionError::UnsupportedSysvar
            })
    }
}

pub struct MockInvokeContextPreparation {
    pub accounts: TransactionAccountRefCells,
    pub message: Message,
    pub account_indices: Vec<usize>,
}

pub fn prepare_mock_invoke_context(
    program_indices: &[usize],
    instruction_data: &[u8],
    keyed_accounts: &[(bool, bool, Pubkey, Rc<RefCell<AccountSharedData>>)],
) -> MockInvokeContextPreparation {
    #[allow(clippy::type_complexity)]
    let (accounts, mut metas): (TransactionAccountRefCells, Vec<AccountMeta>) = keyed_accounts
        .iter()
        .map(|(is_signer, is_writable, pubkey, account)| {
            (
                (*pubkey, account.clone()),
                AccountMeta {
                    pubkey: *pubkey,
                    is_signer: *is_signer,
                    is_writable: *is_writable,
                },
            )
        })
        .unzip();
    let program_id = if let Some(program_index) = program_indices.last() {
        accounts[*program_index].0
    } else {
        Pubkey::default()
    };
    for program_index in program_indices.iter().rev() {
        metas.remove(*program_index);
    }
    let message = Message::new(
        &[Instruction::new_with_bytes(
            program_id,
            instruction_data,
            metas,
        )],
        None,
    );
    let account_indices: Vec<usize> = message
        .account_keys
        .iter()
        .map(|search_key| {
            accounts
                .iter()
                .position(|(key, _account)| key == search_key)
                .unwrap_or(accounts.len())
        })
        .collect();
    MockInvokeContextPreparation {
        accounts,
        message,
        account_indices,
    }
}

pub fn with_mock_invoke_context<R, F: FnMut(&mut InvokeContext) -> R>(
    loader_id: Pubkey,
    account_size: usize,
    mut callback: F,
) -> R {
    let program_indices = vec![0, 1];
    let keyed_accounts = [
        (
            false,
            false,
            loader_id,
            AccountSharedData::new_ref(0, 0, &solana_sdk::native_loader::id()),
        ),
        (
            false,
            false,
            Pubkey::new_unique(),
            AccountSharedData::new_ref(1, 0, &loader_id),
        ),
        (
            false,
            false,
            Pubkey::new_unique(),
            AccountSharedData::new_ref(2, account_size, &Pubkey::new_unique()),
        ),
    ];
    let preparation = prepare_mock_invoke_context(&program_indices, &[], &keyed_accounts);
    let mut invoke_context = InvokeContext::new_mock(&preparation.accounts, &[]);
    invoke_context
        .push(
            &preparation.message,
            &preparation.message.instructions[0],
            &program_indices,
            &preparation.account_indices,
        )
        .unwrap();
    callback(&mut invoke_context)
}

pub fn mock_process_instruction_with_sysvars(
    loader_id: &Pubkey,
    mut program_indices: Vec<usize>,
    instruction_data: &[u8],
    keyed_accounts: &[(bool, bool, Pubkey, Rc<RefCell<AccountSharedData>>)],
    sysvars: &[(Pubkey, Vec<u8>)],
    process_instruction: ProcessInstructionWithContext,
) -> Result<(), InstructionError> {
    let mut preparation =
        prepare_mock_invoke_context(&program_indices, instruction_data, keyed_accounts);
    let processor_account = AccountSharedData::new_ref(0, 0, &solana_sdk::native_loader::id());
    program_indices.insert(0, preparation.accounts.len());
    preparation.accounts.push((*loader_id, processor_account));
    let mut invoke_context = InvokeContext::new_mock(&preparation.accounts, &[]);
    invoke_context.sysvars = sysvars;
    invoke_context.push(
        &preparation.message,
        &preparation.message.instructions[0],
        &program_indices,
        &preparation.account_indices,
    )?;
    process_instruction(1, instruction_data, &mut invoke_context)
}

pub fn mock_process_instruction(
    loader_id: &Pubkey,
    program_indices: Vec<usize>,
    instruction_data: &[u8],
    keyed_accounts: &[(bool, bool, Pubkey, Rc<RefCell<AccountSharedData>>)],
    process_instruction: ProcessInstructionWithContext,
) -> Result<(), InstructionError> {
    mock_process_instruction_with_sysvars(
        loader_id,
        program_indices,
        instruction_data,
        keyed_accounts,
        &[],
        process_instruction,
    )
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        serde::{Deserialize, Serialize},
        solana_sdk::{
            account::{ReadableAccount, WritableAccount},
            instruction::{AccountMeta, Instruction, InstructionError},
            message::Message,
            native_loader,
        },
    };

    #[derive(Debug, Serialize, Deserialize)]
    enum MockInstruction {
        NoopSuccess,
        NoopFail,
        ModifyOwned,
        ModifyNotOwned,
        ModifyReadonly,
    }

    #[test]
    fn test_program_entry_debug() {
        #[allow(clippy::unnecessary_wraps)]
        fn mock_process_instruction(
            _first_instruction_account: usize,
            _data: &[u8],
            _invoke_context: &mut InvokeContext,
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps)]
        fn mock_ix_processor(
            _first_instruction_account: usize,
            _data: &[u8],
            _context: &mut InvokeContext,
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        let builtin_programs = &[
            BuiltinProgram {
                program_id: solana_sdk::pubkey::new_rand(),
                process_instruction: mock_process_instruction,
            },
            BuiltinProgram {
                program_id: solana_sdk::pubkey::new_rand(),
                process_instruction: mock_ix_processor,
            },
        ];
        assert!(!format!("{:?}", builtin_programs).is_empty());
    }

    #[allow(clippy::integer_arithmetic)]
    fn mock_process_instruction(
        first_instruction_account: usize,
        data: &[u8],
        invoke_context: &mut InvokeContext,
    ) -> Result<(), InstructionError> {
        let program_id = invoke_context.get_caller()?;
        let keyed_accounts = invoke_context.get_keyed_accounts()?;
        assert_eq!(
            *program_id,
            keyed_account_at_index(keyed_accounts, first_instruction_account)?.owner()?
        );
        assert_ne!(
            keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?.owner()?,
            *keyed_account_at_index(keyed_accounts, first_instruction_account)?.unsigned_key()
        );

        if let Ok(instruction) = bincode::deserialize(data) {
            match instruction {
                MockInstruction::NoopSuccess => (),
                MockInstruction::NoopFail => return Err(InstructionError::GenericError),
                MockInstruction::ModifyOwned => {
                    keyed_account_at_index(keyed_accounts, first_instruction_account)?
                        .try_account_ref_mut()?
                        .data_as_mut_slice()[0] = 1
                }
                MockInstruction::ModifyNotOwned => {
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?
                        .try_account_ref_mut()?
                        .data_as_mut_slice()[0] = 1
                }
                MockInstruction::ModifyReadonly => {
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?
                        .try_account_ref_mut()?
                        .data_as_mut_slice()[0] = 1
                }
            }
        } else {
            return Err(InstructionError::InvalidInstructionData);
        }
        Ok(())
    }

    #[test]
    fn test_invoke_context() {
        const MAX_DEPTH: usize = 10;
        let mut invoke_stack = vec![];
        let mut accounts = vec![];
        let mut metas = vec![];
        for i in 0..MAX_DEPTH {
            invoke_stack.push(solana_sdk::pubkey::new_rand());
            accounts.push((
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(AccountSharedData::new(
                    i as u64,
                    1,
                    &invoke_stack[i],
                ))),
            ));
            metas.push(AccountMeta::new(accounts[i].0, false));
        }
        for program_id in invoke_stack.iter() {
            accounts.push((
                *program_id,
                Rc::new(RefCell::new(AccountSharedData::new(
                    1,
                    1,
                    &solana_sdk::pubkey::Pubkey::default(),
                ))),
            ));
            metas.push(AccountMeta::new(*program_id, false));
        }
        let account_indices = (0..accounts.len()).collect::<Vec<usize>>();

        let message = Message::new(
            &[Instruction::new_with_bytes(invoke_stack[0], &[0], metas)],
            None,
        );
        let mut invoke_context = InvokeContext::new_mock(&accounts, &[]);

        // Check call depth increases and has a limit
        let mut depth_reached = 0;
        for _ in 0..invoke_stack.len() {
            if Err(InstructionError::CallDepth)
                == invoke_context.push(
                    &message,
                    &message.instructions[0],
                    &[MAX_DEPTH + depth_reached],
                    &[],
                )
            {
                break;
            }
            depth_reached += 1;
        }
        assert_ne!(depth_reached, 0);
        assert!(depth_reached < MAX_DEPTH);

        // Mock each invocation
        for owned_index in (1..depth_reached).rev() {
            let not_owned_index = owned_index - 1;
            let metas = vec![
                AccountMeta::new(accounts[not_owned_index].0, false),
                AccountMeta::new(accounts[owned_index].0, false),
            ];
            let message = Message::new(
                &[Instruction::new_with_bytes(
                    invoke_stack[owned_index],
                    &[0],
                    metas,
                )],
                None,
            );
            let write_privileges: Vec<bool> = (0..message.account_keys.len())
                .map(|i| message.is_writable(i))
                .collect();

            // modify account owned by the program
            accounts[owned_index].1.borrow_mut().data_as_mut_slice()[0] =
                (MAX_DEPTH + owned_index) as u8;
            invoke_context
                .verify_and_update(
                    &message.instructions[0],
                    &account_indices[not_owned_index..owned_index + 1],
                    &write_privileges,
                )
                .unwrap();
            assert_eq!(
                invoke_context.pre_accounts[owned_index].data()[0],
                (MAX_DEPTH + owned_index) as u8
            );

            // modify account not owned by the program
            let data = accounts[not_owned_index].1.borrow_mut().data()[0];
            accounts[not_owned_index].1.borrow_mut().data_as_mut_slice()[0] =
                (MAX_DEPTH + not_owned_index) as u8;
            assert_eq!(
                invoke_context.verify_and_update(
                    &message.instructions[0],
                    &account_indices[not_owned_index..owned_index + 1],
                    &write_privileges,
                ),
                Err(InstructionError::ExternalAccountDataModified)
            );
            assert_eq!(invoke_context.pre_accounts[not_owned_index].data()[0], data);
            accounts[not_owned_index].1.borrow_mut().data_as_mut_slice()[0] = data;

            invoke_context.pop();
        }
    }

    #[test]
    fn test_invoke_context_verify() {
        let accounts = vec![(
            solana_sdk::pubkey::new_rand(),
            Rc::new(RefCell::new(AccountSharedData::default())),
        )];
        let message = Message::new(
            &[Instruction::new_with_bincode(
                accounts[0].0,
                &MockInstruction::NoopSuccess,
                vec![AccountMeta::new_readonly(accounts[0].0, false)],
            )],
            None,
        );
        let mut invoke_context = InvokeContext::new_mock(&accounts, &[]);
        invoke_context
            .push(&message, &message.instructions[0], &[0], &[])
            .unwrap();
        assert!(invoke_context
            .verify(&message, &message.instructions[0], &[0])
            .is_ok());

        let mut _borrowed = accounts[0].1.borrow();
        assert_eq!(
            invoke_context.verify(&message, &message.instructions[0], &[0]),
            Err(InstructionError::AccountBorrowOutstanding)
        );
    }

    #[test]
    fn test_process_cross_program() {
        let caller_program_id = solana_sdk::pubkey::new_rand();
        let callee_program_id = solana_sdk::pubkey::new_rand();

        let owned_account = AccountSharedData::new(42, 1, &callee_program_id);
        let not_owned_account = AccountSharedData::new(84, 1, &solana_sdk::pubkey::new_rand());
        let readonly_account = AccountSharedData::new(168, 1, &solana_sdk::pubkey::new_rand());
        let loader_account = AccountSharedData::new(0, 0, &native_loader::id());
        let mut program_account = AccountSharedData::new(1, 0, &native_loader::id());
        program_account.set_executable(true);

        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(owned_account)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(not_owned_account)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(readonly_account)),
            ),
            (caller_program_id, Rc::new(RefCell::new(loader_account))),
            (callee_program_id, Rc::new(RefCell::new(program_account))),
        ];
        let account_indices = [0, 1, 2];
        let program_indices = [3, 4];

        let metas = vec![
            AccountMeta::new(accounts[0].0, false),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new_readonly(accounts[2].0, false),
        ];

        let caller_instruction =
            CompiledInstruction::new(program_indices[0] as u8, &(), vec![0, 1, 2, 3, 4]);
        let callee_instruction = Instruction::new_with_bincode(
            callee_program_id,
            &MockInstruction::NoopSuccess,
            metas.clone(),
        );
        let message = Message::new(&[callee_instruction], None);

        let builtin_programs = &[BuiltinProgram {
            program_id: callee_program_id,
            process_instruction: mock_process_instruction,
        }];
        let mut invoke_context = InvokeContext::new_mock(&accounts, builtin_programs);
        invoke_context
            .push(&message, &caller_instruction, &program_indices[..1], &[])
            .unwrap();

        // not owned account modified by the caller (before the invoke)
        let caller_write_privileges = message
            .account_keys
            .iter()
            .enumerate()
            .map(|(i, _)| message.is_writable(i))
            .collect::<Vec<bool>>();
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            invoke_context.process_instruction(
                &message,
                &message.instructions[0],
                &program_indices[1..],
                &account_indices,
                &caller_write_privileges,
            ),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 0;

        // readonly account modified by the invoker
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            invoke_context.process_instruction(
                &message,
                &message.instructions[0],
                &program_indices[1..],
                &account_indices,
                &caller_write_privileges,
            ),
            Err(InstructionError::ReadonlyDataModified)
        );
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 0;

        invoke_context.pop();

        let cases = vec![
            (MockInstruction::NoopSuccess, Ok(0)),
            (
                MockInstruction::NoopFail,
                Err(InstructionError::GenericError),
            ),
            (MockInstruction::ModifyOwned, Ok(0)),
            (
                MockInstruction::ModifyNotOwned,
                Err(InstructionError::ExternalAccountDataModified),
            ),
        ];
        for case in cases {
            let callee_instruction =
                Instruction::new_with_bincode(callee_program_id, &case.0, metas.clone());
            let message = Message::new(&[callee_instruction], None);
            invoke_context
                .push(&message, &caller_instruction, &program_indices[..1], &[])
                .unwrap();
            let caller_write_privileges = message
                .account_keys
                .iter()
                .enumerate()
                .map(|(i, _)| message.is_writable(i))
                .collect::<Vec<bool>>();
            assert_eq!(
                invoke_context.process_instruction(
                    &message,
                    &message.instructions[0],
                    &program_indices[1..],
                    &account_indices,
                    &caller_write_privileges,
                ),
                case.1
            );
            invoke_context.pop();
        }
    }

    #[test]
    fn test_native_invoke() {
        let caller_program_id = solana_sdk::pubkey::new_rand();
        let callee_program_id = solana_sdk::pubkey::new_rand();

        let owned_account = AccountSharedData::new(42, 1, &callee_program_id);
        let not_owned_account = AccountSharedData::new(84, 1, &solana_sdk::pubkey::new_rand());
        let readonly_account = AccountSharedData::new(168, 1, &solana_sdk::pubkey::new_rand());
        let loader_account = AccountSharedData::new(0, 0, &native_loader::id());
        let mut program_account = AccountSharedData::new(1, 0, &native_loader::id());
        program_account.set_executable(true);

        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(owned_account)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(not_owned_account)),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(readonly_account)),
            ),
            (caller_program_id, Rc::new(RefCell::new(loader_account))),
            (callee_program_id, Rc::new(RefCell::new(program_account))),
        ];
        let program_indices = [3];
        let metas = vec![
            AccountMeta::new(accounts[0].0, false),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new_readonly(accounts[2].0, false),
        ];

        let caller_instruction =
            CompiledInstruction::new(program_indices[0] as u8, &(), vec![0, 1, 2, 3, 4]);
        let callee_instruction = Instruction::new_with_bincode(
            callee_program_id,
            &MockInstruction::NoopSuccess,
            metas.clone(),
        );
        let message = Message::new(&[callee_instruction.clone()], None);

        let builtin_programs = &[BuiltinProgram {
            program_id: callee_program_id,
            process_instruction: mock_process_instruction,
        }];
        let mut invoke_context = InvokeContext::new_mock(&accounts, builtin_programs);
        invoke_context
            .push(&message, &caller_instruction, &program_indices, &[])
            .unwrap();

        // not owned account modified by the invoker
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            invoke_context.native_invoke(callee_instruction.clone(), &[]),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 0;

        // readonly account modified by the invoker
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            invoke_context.native_invoke(callee_instruction, &[]),
            Err(InstructionError::ReadonlyDataModified)
        );
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 0;

        invoke_context.pop();

        // Other test cases
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
        ];
        for case in cases {
            let callee_instruction =
                Instruction::new_with_bincode(callee_program_id, &case.0, metas.clone());
            let message = Message::new(&[callee_instruction.clone()], None);
            invoke_context
                .push(&message, &caller_instruction, &program_indices, &[])
                .unwrap();
            assert_eq!(
                invoke_context.native_invoke(callee_instruction, &[]),
                case.1
            );
            invoke_context.pop();
        }
    }

    #[test]
    fn test_invoke_context_compute_budget() {
        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                Rc::new(RefCell::new(AccountSharedData::default())),
            ),
            (
                crate::neon_evm_program::id(),
                Rc::new(RefCell::new(AccountSharedData::default())),
            ),
        ];

        let noop_message = Message::new(
            &[Instruction::new_with_bincode(
                accounts[0].0,
                &MockInstruction::NoopSuccess,
                vec![AccountMeta::new_readonly(accounts[0].0, false)],
            )],
            None,
        );
        let neon_message = Message::new(
            &[Instruction::new_with_bincode(
                crate::neon_evm_program::id(),
                &MockInstruction::NoopSuccess,
                vec![AccountMeta::new_readonly(accounts[0].0, false)],
            )],
            None,
        );

        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&tx_wide_compute_cap::id());
        feature_set.deactivate(&requestable_heap_size::id());
        let mut invoke_context = InvokeContext::new_mock(&accounts, &[]);
        invoke_context.feature_set = Arc::new(feature_set);

        invoke_context
            .push(&noop_message, &noop_message.instructions[0], &[0], &[])
            .unwrap();
        assert_eq!(
            *invoke_context.get_compute_budget(),
            ComputeBudget::default()
        );
        invoke_context.pop();

        invoke_context
            .push(&neon_message, &neon_message.instructions[0], &[1], &[])
            .unwrap();
        let expected_compute_budget = ComputeBudget {
            max_units: 500_000,
            heap_size: Some(256_usize.saturating_mul(1024)),
            ..ComputeBudget::default()
        };
        assert_eq!(
            *invoke_context.get_compute_budget(),
            expected_compute_budget
        );
        invoke_context.pop();

        invoke_context
            .push(&noop_message, &noop_message.instructions[0], &[0], &[])
            .unwrap();
        assert_eq!(
            *invoke_context.get_compute_budget(),
            ComputeBudget::default()
        );
        invoke_context.pop();
    }
}
