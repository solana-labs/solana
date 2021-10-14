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
        prevent_calling_precompiles_as_programs, remove_native_loader, tx_wide_compute_cap,
        FeatureSet,
    },
    fee_calculator::FeeCalculator,
    hash::Hash,
    ic_logger_msg,
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::{create_keyed_accounts_unified, KeyedAccount},
    message::Message,
    precompiles::is_precompile,
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
    instruction_index: usize,
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
    instruction_recorders: Option<&'a [InstructionRecorder]>,
    feature_set: Arc<FeatureSet>,
    pub timings: ExecuteDetailsTimings,
    account_db: Arc<Accounts>,
    ancestors: &'a Ancestors,
    #[allow(clippy::type_complexity)]
    sysvars: RefCell<Vec<(Pubkey, Option<Rc<Vec<u8>>>)>>,
    blockhash: &'a Hash,
    fee_calculator: &'a FeeCalculator,
    return_data: (Pubkey, Vec<u8>),
}
impl<'a> ThisInvokeContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rent: Rent,
        accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
        programs: &'a [(Pubkey, ProcessInstructionWithContext)],
        log_collector: Option<Rc<LogCollector>>,
        compute_budget: ComputeBudget,
        compute_meter: Rc<RefCell<dyn ComputeMeter>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorders: Option<&'a [InstructionRecorder]>,
        feature_set: Arc<FeatureSet>,
        account_db: Arc<Accounts>,
        ancestors: &'a Ancestors,
        blockhash: &'a Hash,
        fee_calculator: &'a FeeCalculator,
    ) -> Self {
        Self {
            instruction_index: 0,
            invoke_stack: Vec::with_capacity(compute_budget.max_invoke_depth),
            rent,
            pre_accounts: Vec::new(),
            accounts,
            programs,
            logger: Rc::new(RefCell::new(ThisLogger { log_collector })),
            compute_budget,
            bpf_compute_budget: compute_budget.into(),
            compute_meter,
            executors,
            instruction_recorders,
            feature_set,
            timings: ExecuteDetailsTimings::default(),
            account_db,
            ancestors,
            sysvars: RefCell::new(vec![]),
            blockhash,
            fee_calculator,
            return_data: (Pubkey::default(), Vec::new()),
        }
    }
}
impl<'a> InvokeContext for ThisInvokeContext<'a> {
    fn push(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        account_indices: Option<&[usize]>,
    ) -> Result<(), InstructionError> {
        if self.invoke_stack.len() > self.compute_budget.max_invoke_depth {
            return Err(InstructionError::CallDepth);
        }

        if let Some(index_of_program_id) = program_indices.last() {
            let program_id = &self.accounts[*index_of_program_id].0;
            let contains = self
                .invoke_stack
                .iter()
                .any(|frame| frame.program_id() == Some(program_id));
            let is_last = if let Some(last_frame) = self.invoke_stack.last() {
                last_frame.program_id() == Some(program_id)
            } else {
                false
            };
            if contains && !is_last {
                // Reentrancy not allowed unless caller is calling itself
                return Err(InstructionError::ReentrancyNotAllowed);
            }
        }

        if self.invoke_stack.is_empty() {
            self.pre_accounts = Vec::with_capacity(instruction.accounts.len());
            let mut work = |_unique_index: usize, account_index: usize| {
                if account_index < self.accounts.len() {
                    let account = self.accounts[account_index].1.borrow();
                    self.pre_accounts
                        .push(PreAccount::new(&self.accounts[account_index].0, &account));
                    return Ok(());
                }
                Err(InstructionError::MissingAccount)
            };
            instruction.visit_each_account(&mut work)?;
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
                let account_index = if let Some(account_indices) = account_indices {
                    account_indices[index_in_instruction]
                } else {
                    index_in_instruction
                };
                (
                    message.is_signer(index_in_instruction),
                    message.is_writable(index_in_instruction, demote_program_write_locks),
                    &self.accounts[account_index].0,
                    &self.accounts[account_index].1 as &RefCell<AccountSharedData>,
                )
            }))
            .collect::<Vec<_>>();

        self.invoke_stack.push(InvokeContextStackFrame::new(
            program_indices.len(),
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
    fn verify(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
    ) -> Result<(), InstructionError> {
        let program_id = instruction.program_id(&message.account_keys);
        let demote_program_write_locks = self.is_feature_active(&demote_program_write_locks::id());
        let do_support_realloc = self.is_feature_active(&do_support_realloc::id());

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
            let account = self.accounts[account_index].1.borrow();
            self.pre_accounts[unique_index]
                .verify(
                    program_id,
                    message.is_writable(account_index, demote_program_write_locks),
                    &self.rent,
                    &account,
                    &mut self.timings,
                    true,
                    do_support_realloc,
                )
                .map_err(|err| {
                    ic_logger_msg!(
                        self.logger,
                        "failed to verify account {}: {}",
                        self.pre_accounts[unique_index].key(),
                        err
                    );
                    err
                })?;
            pre_sum += u128::from(self.pre_accounts[unique_index].lamports());
            post_sum += u128::from(account.lamports());
            Ok(())
        };
        instruction.visit_each_account(&mut work)?;

        // Verify that the total sum of all the lamports did not change
        if pre_sum != post_sum {
            return Err(InstructionError::UnbalancedInstruction);
        }
        Ok(())
    }
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
        let logger = &self.logger;
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
            .and_then(|frame| frame.program_id())
            .ok_or(InstructionError::CallDepth)
    }
    fn remove_first_keyed_account(&mut self) -> Result<(), InstructionError> {
        if !self.is_feature_active(&remove_native_loader::id()) {
            let stack_frame = &mut self
                .invoke_stack
                .last_mut()
                .ok_or(InstructionError::CallDepth)?;
            stack_frame.keyed_accounts_range.start =
                stack_frame.keyed_accounts_range.start.saturating_add(1);
        }
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
    fn set_instruction_index(&mut self, instruction_index: usize) {
        if !self.feature_set.is_active(&tx_wide_compute_cap::id()) {
            self.compute_meter = Rc::new(RefCell::new(ThisComputeMeter {
                remaining: self.compute_budget.max_units,
            }));
        }
        self.instruction_index = instruction_index;
    }
    fn record_instruction(&self, instruction: &Instruction) {
        if let Some(instruction_recorders) = &self.instruction_recorders {
            instruction_recorders[self.instruction_index].record_instruction(instruction.clone());
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
    fn set_return_data(&mut self, data: Vec<u8>) -> Result<(), InstructionError> {
        self.return_data = (*self.get_caller()?, data);
        Ok(())
    }
    fn get_return_data(&self) -> (Pubkey, &[u8]) {
        (self.return_data.0, &self.return_data.1)
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
pub struct MessageProcessor {}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for MessageProcessor {
    fn example() -> Self {
        // MessageProcessor's fields are #[serde(skip)]-ed and not Serialize
        // so, just rely on Default anyway.
        MessageProcessor::default()
    }
}

impl MessageProcessor {
    /// Process a message.
    /// This method calls each instruction in the message over the set of loaded accounts.
    /// For each instruction it calls the program entrypoint method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if every instruction succeeds.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    pub fn process_message(
        instruction_processor: &InstructionProcessor,
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
        let mut invoke_context = ThisInvokeContext::new(
            rent_collector.rent,
            accounts,
            instruction_processor.programs(),
            log_collector,
            compute_budget,
            compute_meter,
            executors,
            instruction_recorders,
            feature_set,
            account_db,
            ancestors,
            &blockhash,
            &fee_calculator,
        );
        let compute_meter = invoke_context.get_compute_meter();

        debug_assert_eq!(program_indices.len(), message.instructions.len());
        for (instruction_index, (instruction, program_indices)) in message
            .instructions
            .iter()
            .zip(program_indices.iter())
            .enumerate()
        {
            let program_id = instruction.program_id(&message.account_keys);
            if invoke_context.is_feature_active(&prevent_calling_precompiles_as_programs::id())
                && is_precompile(program_id, |id| invoke_context.is_feature_active(id))
            {
                // Precompiled programs don't have an instruction processor
                continue;
            }

            let mut time = Measure::start("execute_instruction");
            let pre_remaining_units = compute_meter.borrow().get_remaining();

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

            let mut compute_budget = compute_budget;
            if invoke_context.is_feature_active(&neon_evm_compute_budget::id())
                && *program_id == crate::neon_evm_program::id()
            {
                // Bump the compute budget for neon_evm
                compute_budget.max_units = compute_budget.max_units.max(500_000);
                compute_budget.heap_size = Some(256 * 1024);
            }

            invoke_context.set_instruction_index(instruction_index);
            let result = invoke_context
                .push(message, instruction, program_indices, None)
                .and_then(|_| {
                    instruction_processor
                        .process_instruction(&instruction.data, &mut invoke_context)?;
                    invoke_context.verify(message, instruction, program_indices)?;
                    timings.accumulate(&invoke_context.timings);
                    Ok(())
                })
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err));
            invoke_context.pop();

            time.stop();
            let post_remaining_units = compute_meter.borrow().get_remaining();
            timings.accumulate_program(
                instruction.program_id(&message.account_keys),
                time.as_us(),
                pre_remaining_units - post_remaining_units,
            );

            result?;
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
        secp256k1_instruction::new_secp256k1_instruction,
        secp256k1_program,
    };

    #[derive(Debug, Serialize, Deserialize)]
    enum MockInstruction {
        NoopSuccess,
        NoopFail,
        ModifyOwned,
        ModifyNotOwned,
        ModifyReadonly,
    }

    fn mock_process_instruction(
        first_instruction_account: usize,
        data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        let program_id = invoke_context.get_caller()?;
        let keyed_accounts = invoke_context.get_keyed_accounts()?;
        assert_eq!(
            *program_id,
            keyed_accounts[first_instruction_account].owner()?
        );
        assert_ne!(
            keyed_accounts[first_instruction_account + 1].owner()?,
            *keyed_accounts[first_instruction_account].unsigned_key()
        );

        if let Ok(instruction) = bincode::deserialize(data) {
            match instruction {
                MockInstruction::NoopSuccess => (),
                MockInstruction::NoopFail => return Err(InstructionError::GenericError),
                MockInstruction::ModifyOwned => {
                    keyed_accounts[first_instruction_account]
                        .try_account_ref_mut()?
                        .data_as_mut_slice()[0] = 1
                }
                MockInstruction::ModifyNotOwned => {
                    keyed_accounts[first_instruction_account + 1]
                        .try_account_ref_mut()?
                        .data_as_mut_slice()[0] = 1
                }
                MockInstruction::ModifyReadonly => {
                    keyed_accounts[first_instruction_account + 2]
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
        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            Rent::default(),
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
        );

        // Check call depth increases and has a limit
        let mut depth_reached = 0;
        for _ in 0..invoke_stack.len() {
            if Err(InstructionError::CallDepth)
                == invoke_context.push(
                    &message,
                    &message.instructions[0],
                    &[MAX_DEPTH + depth_reached],
                    None,
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
        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            Rent::default(),
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
        );
        invoke_context
            .push(&message, &message.instructions[0], &[0], None)
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
    fn test_process_message_readonly_handling() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            Correct,
            AttemptCredit { lamports: u64 },
            AttemptDataChange { data: u8 },
        }

        fn mock_system_process_instruction(
            first_instruction_account: usize,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::Correct => Ok(()),
                    MockSystemInstruction::AttemptCredit { lamports } => {
                        keyed_accounts[first_instruction_account]
                            .account
                            .borrow_mut()
                            .checked_sub_lamports(lamports)?;
                        keyed_accounts[first_instruction_account + 1]
                            .account
                            .borrow_mut()
                            .checked_add_lamports(lamports)?;
                        Ok(())
                    }
                    // Change data in a read-only account
                    MockSystemInstruction::AttemptDataChange { data } => {
                        keyed_accounts[first_instruction_account + 1]
                            .account
                            .borrow_mut()
                            .set_data(vec![data]);
                        Ok(())
                    }
                }
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }

        let mock_system_program_id = Pubkey::new(&[2u8; 32]);
        let rent_collector = RentCollector::default();
        let mut instruction_processor = InstructionProcessor::default();
        instruction_processor.add_program(&mock_system_program_id, mock_system_process_instruction);

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

        let result = MessageProcessor::process_message(
            &instruction_processor,
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

        let result = MessageProcessor::process_message(
            &instruction_processor,
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

        let result = MessageProcessor::process_message(
            &instruction_processor,
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
            first_instruction_account: usize,
            data: &[u8],
            invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::BorrowFail => {
                        let from_account =
                            keyed_accounts[first_instruction_account].try_account_ref_mut()?;
                        let dup_account =
                            keyed_accounts[first_instruction_account + 2].try_account_ref_mut()?;
                        if from_account.lamports() != dup_account.lamports() {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::MultiBorrowMut => {
                        let from_lamports = {
                            let from_account =
                                keyed_accounts[first_instruction_account].try_account_ref_mut()?;
                            from_account.lamports()
                        };
                        let dup_lamports = {
                            let dup_account = keyed_accounts[first_instruction_account + 2]
                                .try_account_ref_mut()?;
                            dup_account.lamports()
                        };
                        if from_lamports != dup_lamports {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::DoWork { lamports, data } => {
                        {
                            let mut to_account = keyed_accounts[first_instruction_account + 1]
                                .try_account_ref_mut()?;
                            let mut dup_account = keyed_accounts[first_instruction_account + 2]
                                .try_account_ref_mut()?;
                            dup_account.checked_sub_lamports(lamports)?;
                            to_account.checked_add_lamports(lamports)?;
                            dup_account.set_data(vec![data]);
                        }
                        keyed_accounts[first_instruction_account]
                            .try_account_ref_mut()?
                            .checked_sub_lamports(lamports)?;
                        keyed_accounts[first_instruction_account + 1]
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
        let mut instruction_processor = InstructionProcessor::default();
        instruction_processor.add_program(&mock_program_id, mock_system_process_instruction);

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
        let result = MessageProcessor::process_message(
            &instruction_processor,
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
        let result = MessageProcessor::process_message(
            &instruction_processor,
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
        let result = MessageProcessor::process_message(
            &instruction_processor,
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

        let programs: Vec<(_, ProcessInstructionWithContext)> =
            vec![(callee_program_id, mock_process_instruction)];
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

        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            Rent::default(),
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
        );
        invoke_context
            .push(&message, &caller_instruction, &program_indices[..1], None)
            .unwrap();

        // not owned account modified by the caller (before the invoke)
        let demote_program_write_locks =
            invoke_context.is_feature_active(&demote_program_write_locks::id());
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
                &program_indices[1..],
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
                &program_indices[1..],
                &account_indices,
                &caller_write_privileges,
                &mut invoke_context,
            ),
            Err(InstructionError::ReadonlyDataModified)
        );
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 0;

        invoke_context.pop();

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
            invoke_context
                .push(&message, &caller_instruction, &program_indices[..1], None)
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
                    &program_indices[1..],
                    &account_indices,
                    &caller_write_privileges,
                    &mut invoke_context,
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
        let programs: Vec<(_, ProcessInstructionWithContext)> =
            vec![(callee_program_id, mock_process_instruction)];
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

        let ancestors = Ancestors::default();
        let blockhash = Hash::default();
        let fee_calculator = FeeCalculator::default();
        let mut invoke_context = ThisInvokeContext::new(
            Rent::default(),
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
        );
        invoke_context
            .push(&message, &caller_instruction, &program_indices, None)
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
                .push(&message, &caller_instruction, &program_indices, None)
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
            invoke_context.pop();
        }
    }

    #[test]
    fn test_precompile() {
        let mock_program_id = Pubkey::new_unique();
        fn mock_process_instruction(
            _first_instruction_account: usize,
            _data: &[u8],
            _invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            Err(InstructionError::Custom(0xbabb1e))
        }
        let mut instruction_processor = InstructionProcessor::default();
        instruction_processor.add_program(&mock_program_id, mock_process_instruction);

        let secp256k1_account = AccountSharedData::new_ref(1, 0, &native_loader::id());
        secp256k1_account.borrow_mut().set_executable(true);
        let mock_program_account = AccountSharedData::new_ref(1, 0, &native_loader::id());
        mock_program_account.borrow_mut().set_executable(true);
        let accounts = vec![
            (secp256k1_program::id(), secp256k1_account),
            (mock_program_id, mock_program_account),
        ];

        let message = Message::new(
            &[
                new_secp256k1_instruction(
                    &libsecp256k1::SecretKey::random(&mut rand::thread_rng()),
                    b"hello",
                ),
                Instruction::new_with_bytes(mock_program_id, &[], vec![]),
            ],
            None,
        );

        let result = MessageProcessor::process_message(
            &instruction_processor,
            &message,
            &[vec![0], vec![1]],
            &accounts,
            &RentCollector::default(),
            None,
            Rc::new(RefCell::new(Executors::default())),
            None,
            Arc::new(FeatureSet::all_enabled()),
            ComputeBudget::new(),
            Rc::new(RefCell::new(MockComputeMeter::default())),
            &mut ExecuteDetailsTimings::default(),
            Arc::new(Accounts::default_for_tests()),
            &Ancestors::default(),
            Hash::default(),
            FeeCalculator::default(),
        );
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                1,
                InstructionError::Custom(0xbabb1e)
            ))
        );
    }
}
