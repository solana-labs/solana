use crate::{
    accounts::Accounts, ancestors::Ancestors, instruction_recorder::InstructionRecorder,
    log_collector::LogCollector, rent_collector::RentCollector,
};
use log::*;
use serde::{Deserialize, Serialize};
use solana_measure::measure::Measure;
use solana_program_runtime::{ExecuteDetailsTimings, Executors, InstructionProcessor, PreAccount};
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    compute_budget::ComputeBudget,
    feature_set::{
        demote_program_write_locks, do_support_realloc, neon_evm_compute_budget,
        tx_wide_compute_cap, FeatureSet,
    },
    fee_calculator::FeeCalculator,
    hash::Hash,
    ic_logger_msg,
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::{create_keyed_accounts_unified, KeyedAccount},
    message::Message,
    process_instruction::{
        ComputeMeter, Executor, InvokeContext, InvokeContextStackFrame, Logger,
        ProcessInstructionWithContext,
    },
    pubkey::Pubkey,
    rent::Rent,
    sysvar::instructions,
    transaction::TransactionError,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub struct ThisComputeMeter {
    remaining: u64,
}
impl ComputeMeter for ThisComputeMeter {
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError> {
        let exceeded = self.remaining < amount;
        self.remaining = self.remaining.saturating_sub(amount);
        if exceeded {
            return Err(InstructionError::ComputationalBudgetExceeded);
        }
        Ok(())
    }
    fn get_remaining(&self) -> u64 {
        self.remaining
    }
}
pub struct ThisInvokeContext<'a> {
    invoke_stack: Vec<InvokeContextStackFrame<'a>>,
    rent: Rent,
    pre_accounts: Vec<PreAccount>,
    accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
    programs: &'a [(Pubkey, ProcessInstructionWithContext)],
    logger: Rc<RefCell<dyn Logger>>,
    compute_budget: ComputeBudget,
    #[allow(deprecated)]
    bpf_compute_budget: solana_sdk::process_instruction::BpfComputeBudget,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    executors: Rc<RefCell<Executors>>,
    instruction_recorder: Option<InstructionRecorder>,
    feature_set: Arc<FeatureSet>,
    pub timings: ExecuteDetailsTimings,
    account_db: Arc<Accounts>,
    ancestors: &'a Ancestors,
    #[allow(clippy::type_complexity)]
    sysvars: RefCell<Vec<(Pubkey, Option<Rc<Vec<u8>>>)>>,
    blockhash: &'a Hash,
    fee_calculator: &'a FeeCalculator,
    // return data and program_id that set it
    return_data: Option<(Pubkey, Vec<u8>)>,
}
impl<'a> ThisInvokeContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        program_id: &Pubkey,
        rent: Rent,
        message: &'a Message,
        instruction: &'a CompiledInstruction,
        program_indices: &[usize],
        accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
        programs: &'a [(Pubkey, ProcessInstructionWithContext)],
        log_collector: Option<Rc<LogCollector>>,
        compute_budget: ComputeBudget,
        compute_meter: Rc<RefCell<dyn ComputeMeter>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorder: Option<InstructionRecorder>,
        feature_set: Arc<FeatureSet>,
        account_db: Arc<Accounts>,
        ancestors: &'a Ancestors,
        blockhash: &'a Hash,
        fee_calculator: &'a FeeCalculator,
    ) -> Result<Self, InstructionError> {
        let pre_accounts = MessageProcessor::create_pre_accounts(message, instruction, accounts);
        let compute_meter = if feature_set.is_active(&tx_wide_compute_cap::id()) {
            compute_meter
        } else {
            Rc::new(RefCell::new(ThisComputeMeter {
                remaining: compute_budget.max_units,
            }))
        };
        let mut invoke_context = Self {
            invoke_stack: Vec::with_capacity(compute_budget.max_invoke_depth),
            rent,
            pre_accounts,
            accounts,
            programs,
            logger: Rc::new(RefCell::new(ThisLogger { log_collector })),
            compute_budget,
            bpf_compute_budget: compute_budget.into(),
            compute_meter,
            executors,
            instruction_recorder,
            feature_set,
            timings: ExecuteDetailsTimings::default(),
            account_db,
            ancestors,
            sysvars: RefCell::new(vec![]),
            blockhash,
            fee_calculator,
            return_data: None,
        };
        let account_indices = (0..accounts.len()).collect::<Vec<usize>>();
        invoke_context.push(
            program_id,
            message,
            instruction,
            program_indices,
            &account_indices,
        )?;
        Ok(invoke_context)
    }
}
impl<'a> InvokeContext for ThisInvokeContext<'a> {
    fn push(
        &mut self,
        key: &Pubkey,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        account_indices: &[usize],
    ) -> Result<(), InstructionError> {
        if self.invoke_stack.len() > self.compute_budget.max_invoke_depth {
            return Err(InstructionError::CallDepth);
        }

        let contains = self.invoke_stack.iter().any(|frame| frame.key == *key);
        let is_last = if let Some(last_frame) = self.invoke_stack.last() {
            last_frame.key == *key
        } else {
            false
        };
        if contains && !is_last {
            // Reentrancy not allowed unless caller is calling itself
            return Err(InstructionError::ReentrancyNotAllowed);
        }

        // Create the KeyedAccounts that will be passed to the program
        let demote_program_write_locks = self
            .feature_set
            .is_active(&demote_program_write_locks::id());
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
                let account_index = account_indices[index_in_instruction];
                (
                    message.is_signer(index_in_instruction),
                    message.is_writable(index_in_instruction, demote_program_write_locks),
                    &self.accounts[account_index].0,
                    &self.accounts[account_index].1 as &RefCell<AccountSharedData>,
                )
            }))
            .collect::<Vec<_>>();
        self.invoke_stack.push(InvokeContextStackFrame::new(
            *key,
            create_keyed_accounts_unified(keyed_accounts.as_slice()),
        ));
        Ok(())
    }
    fn pop(&mut self) {
        self.invoke_stack.pop();
    }
    fn invoke_depth(&self) -> usize {
        self.invoke_stack.len()
    }
    fn verify_and_update(
        &mut self,
        instruction: &CompiledInstruction,
        account_indices: &[usize],
        write_privileges: &[bool],
    ) -> Result<(), InstructionError> {
        let do_support_realloc = self.feature_set.is_active(&do_support_realloc::id());
        let stack_frame = self
            .invoke_stack
            .last()
            .ok_or(InstructionError::CallDepth)?;
        let program_id = &stack_frame.key;
        let rent = &self.rent;
        let logger = self.get_logger();
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
                                ic_logger_msg!(logger, "failed to verify account {}: {}", key, err);
                                err
                            })?;
                        pre_sum += u128::from(pre_account.lamports());
                        post_sum += u128::from(account.lamports());
                        if is_writable && !pre_account.executable() {
                            pre_account.update(&account);
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
    fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        self.invoke_stack
            .last()
            .map(|frame| &frame.key)
            .ok_or(InstructionError::CallDepth)
    }
    fn remove_first_keyed_account(&mut self) -> Result<(), InstructionError> {
        let stack_frame = &mut self
            .invoke_stack
            .last_mut()
            .ok_or(InstructionError::CallDepth)?;
        stack_frame.keyed_accounts_range.start =
            stack_frame.keyed_accounts_range.start.saturating_add(1);
        Ok(())
    }
    fn get_keyed_accounts(&self) -> Result<&[KeyedAccount], InstructionError> {
        self.invoke_stack
            .last()
            .map(|frame| &frame.keyed_accounts[frame.keyed_accounts_range.clone()])
            .ok_or(InstructionError::CallDepth)
    }
    fn get_programs(&self) -> &[(Pubkey, ProcessInstructionWithContext)] {
        self.programs
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        self.logger.clone()
    }
    #[allow(deprecated)]
    fn get_bpf_compute_budget(&self) -> &solana_sdk::process_instruction::BpfComputeBudget {
        &self.bpf_compute_budget
    }
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
        self.compute_meter.clone()
    }
    fn add_executor(&self, pubkey: &Pubkey, executor: Arc<dyn Executor>) {
        self.executors.borrow_mut().insert(*pubkey, executor);
    }
    fn get_executor(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.executors.borrow().get(pubkey)
    }
    fn record_instruction(&self, instruction: &Instruction) {
        if let Some(recorder) = &self.instruction_recorder {
            recorder.record_instruction(instruction.clone());
        }
    }
    fn is_feature_active(&self, feature_id: &Pubkey) -> bool {
        self.feature_set.is_active(feature_id)
    }
    fn get_account(&self, pubkey: &Pubkey) -> Option<(usize, Rc<RefCell<AccountSharedData>>)> {
        for (index, (key, account)) in self.accounts.iter().enumerate().rev() {
            if key == pubkey {
                return Some((index, account.clone()));
            }
        }
        None
    }
    fn update_timing(
        &mut self,
        serialize_us: u64,
        create_vm_us: u64,
        execute_us: u64,
        deserialize_us: u64,
    ) {
        self.timings.serialize_us += serialize_us;
        self.timings.create_vm_us += create_vm_us;
        self.timings.execute_us += execute_us;
        self.timings.deserialize_us += deserialize_us;
    }
    fn get_sysvar_data(&self, id: &Pubkey) -> Option<Rc<Vec<u8>>> {
        if let Ok(mut sysvars) = self.sysvars.try_borrow_mut() {
            // Try share from cache
            let mut result = sysvars
                .iter()
                .find_map(|(key, sysvar)| if id == key { sysvar.clone() } else { None });
            if result.is_none() {
                // Load it
                result = self
                    .account_db
                    .load_with_fixed_root(self.ancestors, id)
                    .map(|(account, _)| Rc::new(account.data().to_vec()));
                // Cache it
                sysvars.push((*id, result.clone()));
            }
            result
        } else {
            None
        }
    }
    fn get_compute_budget(&self) -> &ComputeBudget {
        &self.compute_budget
    }
    fn get_blockhash(&self) -> &Hash {
        self.blockhash
    }
    fn get_fee_calculator(&self) -> &FeeCalculator {
        self.fee_calculator
    }
    fn set_return_data(&mut self, return_data: Option<(Pubkey, Vec<u8>)>) {
        self.return_data = return_data;
    }
    fn get_return_data(&self) -> &Option<(Pubkey, Vec<u8>)> {
        &self.return_data
    }
}
pub struct ThisLogger {
    log_collector: Option<Rc<LogCollector>>,
}
impl Logger for ThisLogger {
    fn log_enabled(&self) -> bool {
        log_enabled!(log::Level::Info) || self.log_collector.is_some()
    }
    fn log(&self, message: &str) {
        debug!("{}", message);
        if let Some(log_collector) = &self.log_collector {
            log_collector.log(message);
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct MessageProcessor {
    #[serde(skip)]
    instruction_processor: InstructionProcessor,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for MessageProcessor {
    fn example() -> Self {
        // MessageProcessor's fields are #[serde(skip)]-ed and not Serialize
        // so, just rely on Default anyway.
        MessageProcessor::default()
    }
}

impl MessageProcessor {
    /// Add a static entrypoint to intercept instructions before the dynamic loader.
    pub fn add_program(
        &mut self,
        program_id: Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) {
        self.instruction_processor
            .add_program(program_id, process_instruction);
    }

    /// Record the initial state of the accounts so that they can be compared
    /// after the instruction is processed
    pub fn create_pre_accounts(
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
    ) -> Vec<PreAccount> {
        let mut pre_accounts = Vec::with_capacity(instruction.accounts.len());
        {
            let mut work = |_unique_index: usize, account_index: usize| {
                if account_index < message.account_keys.len() && account_index < accounts.len() {
                    let account = accounts[account_index].1.borrow();
                    pre_accounts.push(PreAccount::new(&accounts[account_index].0, &account));
                    return Ok(());
                }
                Err(InstructionError::MissingAccount)
            };
            let _ = instruction.visit_each_account(&mut work);
        }
        pre_accounts
    }

    /// Verify there are no outstanding borrows
    pub fn verify_account_references(
        accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
        program_indices: &[usize],
    ) -> Result<(), InstructionError> {
        for account_index in program_indices.iter() {
            accounts[*account_index]
                .1
                .try_borrow_mut()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
        }
        Ok(())
    }

    /// Verify the results of an instruction
    #[allow(clippy::too_many_arguments)]
    pub fn verify(
        message: &Message,
        instruction: &CompiledInstruction,
        pre_accounts: &[PreAccount],
        program_indices: &[usize],
        accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
        rent: &Rent,
        timings: &mut ExecuteDetailsTimings,
        logger: Rc<RefCell<dyn Logger>>,
        demote_program_write_locks: bool,
        do_support_realloc: bool,
    ) -> Result<(), InstructionError> {
        // Verify all executable accounts have zero outstanding refs
        Self::verify_account_references(accounts, program_indices)?;

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        {
            let program_id = instruction.program_id(&message.account_keys);
            let mut work = |unique_index: usize, account_index: usize| {
                {
                    // Verify account has no outstanding references
                    let _ = accounts[account_index]
                        .1
                        .try_borrow_mut()
                        .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
                }
                let account = accounts[account_index].1.borrow();
                pre_accounts[unique_index]
                    .verify(
                        program_id,
                        message.is_writable(account_index, demote_program_write_locks),
                        rent,
                        &account,
                        timings,
                        true,
                        do_support_realloc,
                    )
                    .map_err(|err| {
                        ic_logger_msg!(
                            logger,
                            "failed to verify account {}: {}",
                            pre_accounts[unique_index].key(),
                            err
                        );
                        err
                    })?;
                pre_sum += u128::from(pre_accounts[unique_index].lamports());
                post_sum += u128::from(account.lamports());
                Ok(())
            };
            instruction.visit_each_account(&mut work)?;
        }

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }

    /// Execute an instruction
    /// This method calls the instruction's program entrypoint method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if this function returns Ok(_).
    #[allow(clippy::too_many_arguments)]
    fn execute_instruction(
        &self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
        rent_collector: &RentCollector,
        log_collector: Option<Rc<LogCollector>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorder: Option<InstructionRecorder>,
        instruction_index: usize,
        feature_set: Arc<FeatureSet>,
        compute_budget: ComputeBudget,
        compute_meter: Rc<RefCell<dyn ComputeMeter>>,
        timings: &mut ExecuteDetailsTimings,
        account_db: Arc<Accounts>,
        ancestors: &Ancestors,
        blockhash: &Hash,
        fee_calculator: &FeeCalculator,
    ) -> Result<(), InstructionError> {
        // Fixup the special instructions key if present
        // before the account pre-values are taken care of
        for (pubkey, accont) in accounts.iter().take(message.account_keys.len()) {
            if instructions::check_id(pubkey) {
                let mut mut_account_ref = accont.borrow_mut();
                instructions::store_current_index(
                    mut_account_ref.data_as_mut_slice(),
                    instruction_index as u16,
                );
                break;
            }
        }

        let program_id = instruction.program_id(&message.account_keys);

        let mut compute_budget = compute_budget;
        if feature_set.is_active(&neon_evm_compute_budget::id())
            && *program_id == crate::neon_evm_program::id()
        {
            // Bump the compute budget for neon_evm
            compute_budget.max_units = compute_budget.max_units.max(500_000);
            compute_budget.heap_size = Some(256 * 1024);
        }

        let programs = self.instruction_processor.programs();
        let mut invoke_context = ThisInvokeContext::new(
            program_id,
            rent_collector.rent,
            message,
            instruction,
            program_indices,
            accounts,
            programs,
            log_collector,
            compute_budget,
            compute_meter,
            executors,
            instruction_recorder,
            feature_set,
            account_db,
            ancestors,
            blockhash,
            fee_calculator,
        )?;

        self.instruction_processor.process_instruction(
            program_id,
            &instruction.data,
            &mut invoke_context,
        )?;
        Self::verify(
            message,
            instruction,
            &invoke_context.pre_accounts,
            program_indices,
            accounts,
            &rent_collector.rent,
            timings,
            invoke_context.get_logger(),
            invoke_context.is_feature_active(&demote_program_write_locks::id()),
            invoke_context.is_feature_active(&do_support_realloc::id()),
        )?;

        timings.accumulate(&invoke_context.timings);

        Ok(())
    }

    /// Process a message.
    /// This method calls each instruction in the message over the set of loaded Accounts
    /// The accounts are committed back to the bank only if every instruction succeeds
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    pub fn process_message(
        &self,
        message: &Message,
        program_indices: &[Vec<usize>],
        accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
        rent_collector: &RentCollector,
        log_collector: Option<Rc<LogCollector>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorders: Option<&[InstructionRecorder]>,
        feature_set: Arc<FeatureSet>,
        compute_budget: ComputeBudget,
        compute_meter: Rc<RefCell<dyn ComputeMeter>>,
        timings: &mut ExecuteDetailsTimings,
        account_db: Arc<Accounts>,
        ancestors: &Ancestors,
        blockhash: Hash,
        fee_calculator: FeeCalculator,
    ) -> Result<(), TransactionError> {
        for (instruction_index, instruction) in message.instructions.iter().enumerate() {
            let mut time = Measure::start("execute_instruction");
            let pre_remaining_units = compute_meter.borrow().get_remaining();
            let instruction_recorder = instruction_recorders
                .as_ref()
                .map(|recorders| recorders[instruction_index].clone());
            let err = self
                .execute_instruction(
                    message,
                    instruction,
                    &program_indices[instruction_index],
                    accounts,
                    rent_collector,
                    log_collector.clone(),
                    executors.clone(),
                    instruction_recorder,
                    instruction_index,
                    feature_set.clone(),
                    compute_budget,
                    compute_meter.clone(),
                    timings,
                    account_db.clone(),
                    ancestors,
                    &blockhash,
                    &fee_calculator,
                )
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err));
            time.stop();
            let post_remaining_units = compute_meter.borrow().get_remaining();

            timings.accumulate_program(
                instruction.program_id(&message.account_keys),
                time.as_us(),
                pre_remaining_units - post_remaining_units,
            );

            err?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        native_loader::{self, create_loadable_account_for_test},
        process_instruction::MockComputeMeter,
    };

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
        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            &invoke_stack[0],
            Rent::default(),
            &message,
            &message.instructions[0],
            &[],
            &accounts,
            &[],
            None,
            ComputeBudget::default(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(FeatureSet::all_enabled()),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            &blockhash,
            &fee_calculator,
        )
        .unwrap();

        // Check call depth increases and has a limit
        let mut depth_reached = 1;
        for program_id in invoke_stack.iter().skip(1) {
            if Err(InstructionError::CallDepth)
                == invoke_context.push(
                    program_id,
                    &message,
                    &message.instructions[0],
                    &[],
                    &account_indices,
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
                .map(|i| message.is_writable(i, /*demote_program_write_locks=*/ true))
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
    fn test_verify_account_references() {
        let accounts = vec![(
            solana_sdk::pubkey::new_rand(),
            Rc::new(RefCell::new(AccountSharedData::default())),
        )];

        assert!(MessageProcessor::verify_account_references(&accounts, &[0]).is_ok());

        let mut _borrowed = accounts[0].1.borrow();
        assert_eq!(
            MessageProcessor::verify_account_references(&accounts, &[0]),
            Err(InstructionError::AccountBorrowOutstanding)
        );
    }

    #[test]
    fn test_process_message_readonly_handling() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            Correct,
            AttemptCredit { lamports: u64 },
            AttemptDataChange { data: u8 },
        }

        fn mock_system_process_instruction(
            _program_id: &Pubkey,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::Correct => Ok(()),
                    MockSystemInstruction::AttemptCredit { lamports } => {
                        keyed_accounts[0]
                            .account
                            .borrow_mut()
                            .checked_sub_lamports(lamports)?;
                        keyed_accounts[1]
                            .account
                            .borrow_mut()
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                    // Change data in a read-only account
                    MockSystemInstruction::AttemptDataChange { data } => {
                        keyed_accounts[1].account.borrow_mut().set_data(vec![data]);
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let mock_system_program_id = Pubkey::new(&[2u8; 32]);
        let rent_collector = RentCollector::default();
        let mut message_processor = MessageProcessor::default();
        message_processor.add_program(mock_system_program_id, mock_system_process_instruction);

        let program_account = Rc::new(RefCell::new(create_loadable_account_for_test(
            "mock_system_program",
        )));
        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new_ref(100, 1, &mock_system_program_id),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new_ref(0, 1, &mock_system_program_id),
            ),
            (mock_system_program_id, program_account),
        ];
        let program_indices = vec![vec![2]];

        let executors = Rc::new(RefCell::new(Executors::default()));
        let ancestors = Ancestors::default();

        let account_metas = vec![
            AccountMeta::new(accounts[0].0, true),
            AccountMeta::new_readonly(accounts[1].0, false),
        ];
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::Correct,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );

        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].1.borrow().lamports(), 100);
        assert_eq!(accounts[1].1.borrow().lamports(), 0);

        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::AttemptCredit { lamports: 50 },
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );

        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );

        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_system_program_id,
                &MockSystemInstruction::AttemptDataChange { data: 50 },
                account_metas,
            )],
            Some(&accounts[0].0),
        );

        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors,
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyDataModified
            ))
        );
    }

    #[test]
    fn test_process_message_duplicate_accounts() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            BorrowFail,
            MultiBorrowMut,
            DoWork { lamports: u64, data: u8 },
        }

        fn mock_system_process_instruction(
            _program_id: &Pubkey,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::BorrowFail => {
                        let from_account = keyed_accounts[0].try_account_ref_mut()?;
                        let dup_account = keyed_accounts[2].try_account_ref_mut()?;
                        if from_account.lamports() != dup_account.lamports() {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::MultiBorrowMut => {
                        let from_lamports = {
                            let from_account = keyed_accounts[0].try_account_ref_mut()?;
                            from_account.lamports()
                        };
                        let dup_lamports = {
                            let dup_account = keyed_accounts[2].try_account_ref_mut()?;
                            dup_account.lamports()
                        };
                        if from_lamports != dup_lamports {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::DoWork { lamports, data } => {
                        {
                            let mut to_account = keyed_accounts[1].try_account_ref_mut()?;
                            let mut dup_account = keyed_accounts[2].try_account_ref_mut()?;
                            dup_account.checked_sub_lamports(lamports)?;
                            to_account.checked_add_lamports(lamports)?;
                            dup_account.set_data(vec![data]);
                        }
                        keyed_accounts[0]
                            .try_account_ref_mut()?
                            .checked_sub_lamports(lamports)?;
                        keyed_accounts[1]
                            .try_account_ref_mut()?
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let mock_program_id = Pubkey::new(&[2u8; 32]);
        let rent_collector = RentCollector::default();
        let mut message_processor = MessageProcessor::default();
        message_processor.add_program(mock_program_id, mock_system_process_instruction);

        let program_account = Rc::new(RefCell::new(create_loadable_account_for_test(
            "mock_system_program",
        )));
        let accounts = vec![
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new_ref(100, 1, &mock_program_id),
            ),
            (
                solana_sdk::pubkey::new_rand(),
                AccountSharedData::new_ref(0, 1, &mock_program_id),
            ),
            (mock_program_id, program_account),
        ];
        let program_indices = vec![vec![2]];

        let executors = Rc::new(RefCell::new(Executors::default()));
        let ancestors = Ancestors::default();

        let account_metas = vec![
            AccountMeta::new(accounts[0].0, true),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new(accounts[0].0, false),
        ];

        // Try to borrow mut the same account
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::BorrowFail,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );
        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::AccountBorrowFailed
            ))
        );

        // Try to borrow mut the same account in a safe way
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::MultiBorrowMut,
                account_metas.clone(),
            )],
            Some(&accounts[0].0),
        );
        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors.clone(),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(result, Ok(()));

        // Do work on the same account but at different location in keyed_accounts[]
        let message = Message::new(
            &[Instruction::new_with_bincode(
                mock_program_id,
                &MockSystemInstruction::DoWork {
                    lamports: 10,
                    data: 42,
                },
                account_metas,
            )],
            Some(&accounts[0].0),
        );
        let ancestors = Ancestors::default();
        let result = message_processor.process_message(
            &message,
            &program_indices,
            &accounts,
            &rent_collector,
            None,
            executors,
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].1.borrow().lamports(), 80);
        assert_eq!(accounts[1].1.borrow().lamports(), 20);
        assert_eq!(accounts[0].1.borrow().data(), &vec![42]);
    }

    #[test]
    fn test_process_cross_program() {
        #[derive(Debug, Serialize, Deserialize)]
        enum MockInstruction {
            NoopSuccess,
            NoopFail,
            ModifyOwned,
            ModifyNotOwned,
            ModifyReadonly,
        }

        fn mock_process_instruction(
            program_id: &Pubkey,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            assert_eq!(*program_id, keyed_accounts[0].owner()?);
            assert_ne!(
                keyed_accounts[1].owner()?,
                *keyed_accounts[0].unsigned_key()
            );

            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockInstruction::NoopSuccess => (),
                    MockInstruction::NoopFail => return Err(InstructionError::GenericError),
                    MockInstruction::ModifyOwned => {
                        keyed_accounts[0].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                    MockInstruction::ModifyNotOwned => {
                        keyed_accounts[1].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                    MockInstruction::ModifyReadonly => {
                        keyed_accounts[2].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                }
            } else {
                return Err(InstructionError::InvalidInstructionData);
            }
            Ok(())
        }

        let caller_program_id = solana_sdk::pubkey::new_rand();
        let callee_program_id = solana_sdk::pubkey::new_rand();

        let owned_account = AccountSharedData::new(42, 1, &callee_program_id);
        let not_owned_account = AccountSharedData::new(84, 1, &solana_sdk::pubkey::new_rand());
        let readonly_account = AccountSharedData::new(168, 1, &caller_program_id);
        let mut program_account = AccountSharedData::new(1, 0, &native_loader::id());
        program_account.set_executable(true);

        #[allow(unused_mut)]
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
            (callee_program_id, Rc::new(RefCell::new(program_account))),
        ];
        let account_indices = [0, 1, 2];
        let program_indices = vec![3];

        let programs: Vec<(_, ProcessInstructionWithContext)> =
            vec![(callee_program_id, mock_process_instruction)];
        let metas = vec![
            AccountMeta::new(accounts[0].0, false),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new_readonly(accounts[2].0, false),
        ];

        let caller_instruction = CompiledInstruction::new(2, &(), vec![0, 1, 2, 3]);
        let callee_instruction = Instruction::new_with_bincode(
            callee_program_id,
            &MockInstruction::NoopSuccess,
            metas.clone(),
        );
        let message = Message::new(&[callee_instruction], None);

        let feature_set = FeatureSet::all_enabled();
        let demote_program_write_locks = feature_set.is_active(&demote_program_write_locks::id());

        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            &caller_program_id,
            Rent::default(),
            &message,
            &caller_instruction,
            &program_indices,
            &accounts,
            programs.as_slice(),
            None,
            ComputeBudget::default(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(feature_set),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            &blockhash,
            &fee_calculator,
        )
        .unwrap();

        // not owned account modified by the caller (before the invoke)
        let caller_write_privileges = message
            .account_keys
            .iter()
            .enumerate()
            .map(|(i, _)| message.is_writable(i, demote_program_write_locks))
            .collect::<Vec<bool>>();
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::process_cross_program_instruction(
                &message,
                &program_indices,
                &account_indices,
                &caller_write_privileges,
                &mut invoke_context,
            ),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 0;

        // readonly account modified by the invoker
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::process_cross_program_instruction(
                &message,
                &program_indices,
                &account_indices,
                &caller_write_privileges,
                &mut invoke_context,
            ),
            Err(InstructionError::ReadonlyDataModified)
        );
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 0;

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
        ];

        for case in cases {
            let callee_instruction =
                Instruction::new_with_bincode(callee_program_id, &case.0, metas.clone());
            let message = Message::new(&[callee_instruction], None);

            let ancestors = Ancestors::default();
            let blockhash = Hash::default();
            let fee_calculator = FeeCalculator::default();
            let mut invoke_context = ThisInvokeContext::new(
                &caller_program_id,
                Rent::default(),
                &message,
                &caller_instruction,
                &program_indices,
                &accounts,
                programs.as_slice(),
                None,
                ComputeBudget::default(),
                Rc::new(RefCell::new(MockComputeMeter::default())),
                Rc::new(RefCell::new(Executors::default())),
                None,
                Arc::new(FeatureSet::all_enabled()),
                Arc::new(Accounts::default_for_tests()),
                &ancestors,
                &blockhash,
                &fee_calculator,
            )
            .unwrap();

            let caller_write_privileges = message
                .account_keys
                .iter()
                .enumerate()
                .map(|(i, _)| message.is_writable(i, demote_program_write_locks))
                .collect::<Vec<bool>>();
            assert_eq!(
                InstructionProcessor::process_cross_program_instruction(
                    &message,
                    &program_indices,
                    &account_indices,
                    &caller_write_privileges,
                    &mut invoke_context,
                ),
                case.1
            );
        }
    }

    #[test]
    fn test_native_invoke() {
        #[derive(Debug, Serialize, Deserialize)]
        enum MockInstruction {
            NoopSuccess,
            NoopFail,
            ModifyOwned,
            ModifyNotOwned,
            ModifyReadonly,
        }

        fn mock_process_instruction(
            program_id: &Pubkey,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            assert_eq!(*program_id, keyed_accounts[0].owner()?);
            assert_ne!(
                keyed_accounts[1].owner()?,
                *keyed_accounts[0].unsigned_key()
            );

            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockInstruction::NoopSuccess => (),
                    MockInstruction::NoopFail => return Err(InstructionError::GenericError),
                    MockInstruction::ModifyOwned => {
                        keyed_accounts[0].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                    MockInstruction::ModifyNotOwned => {
                        keyed_accounts[1].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                    MockInstruction::ModifyReadonly => {
                        keyed_accounts[2].try_account_ref_mut()?.data_as_mut_slice()[0] = 1
                    }
                }
            } else {
                return Err(InstructionError::InvalidInstructionData);
            }
            Ok(())
        }

        let caller_program_id = solana_sdk::pubkey::new_rand();
        let callee_program_id = solana_sdk::pubkey::new_rand();

        let owned_account = AccountSharedData::new(42, 1, &callee_program_id);
        let not_owned_account = AccountSharedData::new(84, 1, &solana_sdk::pubkey::new_rand());
        let readonly_account = AccountSharedData::new(168, 1, &caller_program_id);
        let mut program_account = AccountSharedData::new(1, 0, &native_loader::id());
        program_account.set_executable(true);

        #[allow(unused_mut)]
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
            (callee_program_id, Rc::new(RefCell::new(program_account))),
        ];
        let program_indices = vec![3];
        let programs: Vec<(_, ProcessInstructionWithContext)> =
            vec![(callee_program_id, mock_process_instruction)];
        let metas = vec![
            AccountMeta::new(accounts[0].0, false),
            AccountMeta::new(accounts[1].0, false),
            AccountMeta::new_readonly(accounts[2].0, false),
        ];

        let caller_instruction = CompiledInstruction::new(2, &(), vec![0, 1, 2, 3]);
        let callee_instruction = Instruction::new_with_bincode(
            callee_program_id,
            &MockInstruction::NoopSuccess,
            metas.clone(),
        );
        let message = Message::new(&[callee_instruction.clone()], None);

        let feature_set = FeatureSet::all_enabled();
        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            &caller_program_id,
            Rent::default(),
            &message,
            &caller_instruction,
            &program_indices,
            &accounts,
            programs.as_slice(),
            None,
            ComputeBudget::default(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(feature_set),
            Arc::new(Accounts::default_for_tests()),
            &ancestors,
            &blockhash,
            &fee_calculator,
        )
        .unwrap();

        // not owned account modified by the invoker
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::native_invoke(
                &mut invoke_context,
                callee_instruction.clone(),
                &[0, 1, 2, 3],
                &[]
            ),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 0;

        // readonly account modified by the invoker
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::native_invoke(
                &mut invoke_context,
                callee_instruction,
                &[0, 1, 2, 3],
                &[]
            ),
            Err(InstructionError::ReadonlyDataModified)
        );
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 0;

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

            let ancestors = Ancestors::default();
            let blockhash = Hash::default();
            let fee_calculator = FeeCalculator::default();
            let mut invoke_context = ThisInvokeContext::new(
                &caller_program_id,
                Rent::default(),
                &message,
                &caller_instruction,
                &program_indices,
                &accounts,
                programs.as_slice(),
                None,
                ComputeBudget::default(),
                Rc::new(RefCell::new(MockComputeMeter::default())),
                Rc::new(RefCell::new(Executors::default())),
                None,
                Arc::new(FeatureSet::all_enabled()),
                Arc::new(Accounts::default_for_tests()),
                &ancestors,
                &blockhash,
                &fee_calculator,
            )
            .unwrap();

            assert_eq!(
                InstructionProcessor::native_invoke(
                    &mut invoke_context,
                    callee_instruction,
                    &[0, 1, 2, 3],
                    &[]
                ),
                case.1
            );
        }
    }
}
