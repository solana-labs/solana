use crate::{
    instruction_processor::{ExecuteDetailsTimings, Executors, PreAccount},
    instruction_recorder::InstructionRecorder,
    log_collector::LogCollector,
};
use log::*;
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount},
    compute_budget::ComputeBudget,
    feature_set::{
        demote_program_write_locks, do_support_realloc, remove_native_loader, tx_wide_compute_cap,
        FeatureSet,
    },
    hash::Hash,
    ic_logger_msg, ic_msg,
    instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
    keyed_account::{create_keyed_accounts_unified, KeyedAccount},
    message::Message,
    process_instruction::{
        ComputeMeter, Executor, InvokeContext, Logger, ProcessInstructionWithContext,
    },
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
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
impl ThisComputeMeter {
    pub fn new_ref(remaining: u64) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { remaining }))
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
impl ThisLogger {
    pub fn new_ref(log_collector: Option<Rc<LogCollector>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { log_collector }))
    }
}

pub struct InvokeContextStackFrame<'a> {
    pub number_of_program_accounts: usize,
    pub keyed_accounts: Vec<KeyedAccount<'a>>,
    pub keyed_accounts_range: std::ops::Range<usize>,
}

impl<'a> InvokeContextStackFrame<'a> {
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

pub struct ThisInvokeContext<'a> {
    instruction_index: usize,
    invoke_stack: Vec<InvokeContextStackFrame<'a>>,
    rent: Rent,
    pre_accounts: Vec<PreAccount>,
    accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
    programs: &'a [(Pubkey, ProcessInstructionWithContext)],
    sysvars: &'a [(Pubkey, Vec<u8>)],
    logger: Rc<RefCell<dyn Logger>>,
    compute_budget: ComputeBudget,
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    executors: Rc<RefCell<Executors>>,
    instruction_recorders: Option<&'a [InstructionRecorder]>,
    feature_set: Arc<FeatureSet>,
    pub timings: ExecuteDetailsTimings,
    blockhash: Hash,
    lamports_per_signature: u64,
    return_data: (Pubkey, Vec<u8>),
}
impl<'a> ThisInvokeContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rent: Rent,
        accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
        programs: &'a [(Pubkey, ProcessInstructionWithContext)],
        sysvars: &'a [(Pubkey, Vec<u8>)],
        log_collector: Option<Rc<LogCollector>>,
        compute_budget: ComputeBudget,
        compute_meter: Rc<RefCell<dyn ComputeMeter>>,
        executors: Rc<RefCell<Executors>>,
        instruction_recorders: Option<&'a [InstructionRecorder]>,
        feature_set: Arc<FeatureSet>,
        blockhash: Hash,
        lamports_per_signature: u64,
    ) -> Self {
        Self {
            instruction_index: 0,
            invoke_stack: Vec::with_capacity(compute_budget.max_invoke_depth),
            rent,
            pre_accounts: Vec::new(),
            accounts,
            programs,
            sysvars,
            logger: ThisLogger::new_ref(log_collector),
            compute_budget,
            compute_meter,
            executors,
            instruction_recorders,
            feature_set,
            timings: ExecuteDetailsTimings::default(),
            blockhash,
            lamports_per_signature,
            return_data: (Pubkey::default(), Vec::new()),
        }
    }

    pub fn new_mock_with_sysvars_and_features(
        accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
        programs: &'a [(Pubkey, ProcessInstructionWithContext)],
        sysvars: &'a [(Pubkey, Vec<u8>)],
        feature_set: Arc<FeatureSet>,
    ) -> Self {
        Self::new(
            Rent::default(),
            accounts,
            programs,
            sysvars,
            None,
            ComputeBudget::default(),
            ThisComputeMeter::new_ref(std::i64::MAX as u64),
            Rc::new(RefCell::new(Executors::default())),
            None,
            feature_set,
            Hash::default(),
            0,
        )
    }

    pub fn new_mock(
        accounts: &'a [(Pubkey, Rc<RefCell<AccountSharedData>>)],
        programs: &'a [(Pubkey, ProcessInstructionWithContext)],
    ) -> Self {
        Self::new_mock_with_sysvars_and_features(
            accounts,
            programs,
            &[],
            Arc::new(FeatureSet::all_enabled()),
        )
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
            if !self.feature_set.is_active(&tx_wide_compute_cap::id()) {
                self.compute_meter = ThisComputeMeter::new_ref(self.compute_budget.max_units);
            }

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
    fn get_sysvars(&self) -> &[(Pubkey, Vec<u8>)] {
        self.sysvars
    }
    fn get_compute_budget(&self) -> &ComputeBudget {
        &self.compute_budget
    }
    fn set_blockhash(&mut self, hash: Hash) {
        self.blockhash = hash;
    }
    fn get_blockhash(&self) -> &Hash {
        &self.blockhash
    }
    fn set_lamports_per_signature(&mut self, lamports_per_signature: u64) {
        self.lamports_per_signature = lamports_per_signature;
    }
    fn get_lamports_per_signature(&self) -> u64 {
        self.lamports_per_signature
    }
    fn set_return_data(&mut self, data: Vec<u8>) -> Result<(), InstructionError> {
        self.return_data = (*self.get_caller()?, data);
        Ok(())
    }
    fn get_return_data(&self) -> (Pubkey, &[u8]) {
        (self.return_data.0, &self.return_data.1)
    }
}

// This method which has a generic parameter is outside of the InvokeContext,
// because the InvokeContext is a dyn Trait.
pub fn get_sysvar<T: Sysvar>(
    invoke_context: &dyn InvokeContext,
    id: &Pubkey,
) -> Result<T, InstructionError> {
    invoke_context
        .get_sysvars()
        .iter()
        .find_map(|(key, data)| {
            if id == key {
                bincode::deserialize(data).ok()
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ic_msg!(invoke_context, "Unable to get sysvar {}", id);
            InstructionError::UnsupportedSysvar
        })
}

pub struct MockInvokeContextPreparation {
    pub accounts: Vec<(Pubkey, Rc<RefCell<AccountSharedData>>)>,
    pub message: Message,
    pub account_indices: Vec<usize>,
}

pub fn prepare_mock_invoke_context(
    program_indices: &[usize],
    instruction_data: &[u8],
    keyed_accounts: &[(bool, bool, Pubkey, Rc<RefCell<AccountSharedData>>)],
) -> MockInvokeContextPreparation {
    #[allow(clippy::type_complexity)]
    let (accounts, mut metas): (
        Vec<(Pubkey, Rc<RefCell<AccountSharedData>>)>,
        Vec<AccountMeta>,
    ) = keyed_accounts
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

pub fn with_mock_invoke_context<R, F: FnMut(&mut ThisInvokeContext) -> R>(
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
    let mut invoke_context = ThisInvokeContext::new_mock(&preparation.accounts, &[]);
    invoke_context
        .push(
            &preparation.message,
            &preparation.message.instructions[0],
            &program_indices,
            Some(&preparation.account_indices),
        )
        .unwrap();
    callback(&mut invoke_context)
}

pub fn mock_process_instruction(
    loader_id: &Pubkey,
    mut program_indices: Vec<usize>,
    instruction_data: &[u8],
    keyed_accounts: &[(bool, bool, Pubkey, Rc<RefCell<AccountSharedData>>)],
    process_instruction: ProcessInstructionWithContext,
) -> Result<(), InstructionError> {
    let mut preparation =
        prepare_mock_invoke_context(&program_indices, instruction_data, keyed_accounts);
    let processor_account = AccountSharedData::new_ref(0, 0, &solana_sdk::native_loader::id());
    program_indices.insert(0, preparation.accounts.len());
    preparation.accounts.push((*loader_id, processor_account));
    let mut invoke_context = ThisInvokeContext::new_mock(&preparation.accounts, &[]);
    invoke_context.push(
        &preparation.message,
        &preparation.message.instructions[0],
        &program_indices,
        Some(&preparation.account_indices),
    )?;
    process_instruction(1, instruction_data, &mut invoke_context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction_processor::InstructionProcessor;
    use serde::{Deserialize, Serialize};
    use solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        instruction::{AccountMeta, Instruction, InstructionError},
        keyed_account::keyed_account_at_index,
        message::Message,
        native_loader,
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
        let mut invoke_context = ThisInvokeContext::new_mock(&accounts, &[]);

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
        let mut invoke_context = ThisInvokeContext::new_mock(&accounts, &[]);
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

        let mut invoke_context = ThisInvokeContext::new_mock(&accounts, programs.as_slice());
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

        let mut invoke_context = ThisInvokeContext::new_mock(&accounts, programs.as_slice());
        invoke_context
            .push(&message, &caller_instruction, &program_indices, None)
            .unwrap();

        // not owned account modified by the invoker
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::native_invoke(
                &mut invoke_context,
                callee_instruction.clone(),
                &[]
            ),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].1.borrow_mut().data_as_mut_slice()[0] = 0;

        // readonly account modified by the invoker
        accounts[2].1.borrow_mut().data_as_mut_slice()[0] = 1;
        assert_eq!(
            InstructionProcessor::native_invoke(&mut invoke_context, callee_instruction, &[]),
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
                InstructionProcessor::native_invoke(&mut invoke_context, callee_instruction, &[]),
                case.1
            );
            invoke_context.pop();
        }
    }
}
