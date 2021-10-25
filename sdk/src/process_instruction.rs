#![cfg(feature = "full")]

use itertools::Itertools;
use solana_sdk::{
    account::AccountSharedData,
    compute_budget::ComputeBudget,
    feature_set::remove_native_loader,
    hash::Hash,
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::{create_keyed_accounts_unified, KeyedAccount},
    message::Message,
    pubkey::Pubkey,
    sysvar::Sysvar,
};
use std::{cell::RefCell, collections::HashSet, fmt::Debug, rc::Rc, sync::Arc};

/// Prototype of a native loader entry point
///
/// program_id: Program ID of the currently executing program
/// keyed_accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
/// invoke_context: Invocation context
pub type LoaderEntrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    instruction_data: &[u8],
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError>;

pub type ProcessInstructionWithContext =
    fn(usize, &[u8], &mut dyn InvokeContext) -> Result<(), InstructionError>;

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

/// Invocation context passed to loaders
pub trait InvokeContext {
    /// Push a stack frame onto the invocation stack
    fn push(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
        account_indices: Option<&[usize]>,
    ) -> Result<(), InstructionError>;
    /// Pop a stack frame from the invocation stack
    fn pop(&mut self);
    /// Current depth of the invocation stake
    fn invoke_depth(&self) -> usize;
    /// Verify the results of an instruction
    fn verify(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        program_indices: &[usize],
    ) -> Result<(), InstructionError>;
    /// Verify and update PreAccount state based on program execution
    fn verify_and_update(
        &mut self,
        instruction: &CompiledInstruction,
        account_indices: &[usize],
        write_privileges: &[bool],
    ) -> Result<(), InstructionError>;
    /// Get the program ID of the currently executing program
    fn get_caller(&self) -> Result<&Pubkey, InstructionError>;
    /// Removes the first keyed account
    #[deprecated(
        since = "1.9.0",
        note = "To be removed together with remove_native_loader"
    )]
    fn remove_first_keyed_account(&mut self) -> Result<(), InstructionError>;
    /// Get the list of keyed accounts
    fn get_keyed_accounts(&self) -> Result<&[KeyedAccount], InstructionError>;
    /// Get a list of built-in programs
    fn get_programs(&self) -> &[(Pubkey, ProcessInstructionWithContext)];
    /// Get this invocation's logger
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>>;
    /// Get this invocation's compute meter
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>>;
    /// Loaders may need to do work in order to execute a program.  Cache
    /// the work that can be re-used across executions
    fn add_executor(&self, pubkey: &Pubkey, executor: Arc<dyn Executor>);
    /// Get the completed loader work that can be re-used across executions
    fn get_executor(&self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>>;
    /// Set which instruction in the message is currently being recorded
    fn set_instruction_index(&mut self, instruction_index: usize);
    /// Record invoked instruction
    fn record_instruction(&self, instruction: &Instruction);
    /// Get the bank's active feature set
    fn is_feature_active(&self, feature_id: &Pubkey) -> bool;
    /// Find an account_index and account by its key
    fn get_account(&self, pubkey: &Pubkey) -> Option<(usize, Rc<RefCell<AccountSharedData>>)>;
    /// Update timing
    fn update_timing(
        &mut self,
        serialize_us: u64,
        create_vm_us: u64,
        execute_us: u64,
        deserialize_us: u64,
    );
    /// Get sysvars
    #[allow(clippy::type_complexity)]
    fn get_sysvars(&self) -> &RefCell<Vec<(Pubkey, Option<Rc<Vec<u8>>>)>>;
    /// Get sysvar data
    fn get_sysvar_data(&self, id: &Pubkey) -> Option<Rc<Vec<u8>>>;
    /// Get this invocation's compute budget
    fn get_compute_budget(&self) -> &ComputeBudget;
    /// Set this invocation's blockhash
    fn set_blockhash(&mut self, hash: Hash);
    /// Get this invocation's blockhash
    fn get_blockhash(&self) -> &Hash;
    /// Set this invocation's lamports_per_signature value
    fn set_lamports_per_signature(&mut self, lamports_per_signature: u64);
    /// Get this invocation's lamports_per_signature value
    fn get_lamports_per_signature(&self) -> u64;
    /// Set the return data
    fn set_return_data(&mut self, data: Vec<u8>) -> Result<(), InstructionError>;
    /// Get the return data
    fn get_return_data(&self) -> (Pubkey, &[u8]);
}

/// Convenience macro to log a message with an `Rc<RefCell<dyn Logger>>`
#[macro_export]
macro_rules! ic_logger_msg {
    ($logger:expr, $message:expr) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log($message);
            }
        }
    };
    ($logger:expr, $fmt:expr, $($arg:tt)*) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log(&format!($fmt, $($arg)*));
            }
        }
    };
}

/// Convenience macro to log a message with an `InvokeContext`
#[macro_export]
macro_rules! ic_msg {
    ($invoke_context:expr, $message:expr) => {
        $crate::ic_logger_msg!($invoke_context.get_logger(), $message)
    };
    ($invoke_context:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::ic_logger_msg!($invoke_context.get_logger(), $fmt, $($arg)*)
    };
}

pub fn get_sysvar<T: Sysvar>(
    invoke_context: &dyn InvokeContext,
    id: &Pubkey,
) -> Result<T, InstructionError> {
    let sysvar_data = invoke_context.get_sysvar_data(id).ok_or_else(|| {
        ic_msg!(invoke_context, "Unable to get sysvar {}", id);
        InstructionError::UnsupportedSysvar
    })?;

    bincode::deserialize(&sysvar_data).map_err(|err| {
        ic_msg!(invoke_context, "Unable to get sysvar {}: {:?}", id, err);
        InstructionError::UnsupportedSysvar
    })
}

/// Compute meter
pub trait ComputeMeter {
    /// Consume compute units
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError>;
    /// Get the number of remaining compute units
    fn get_remaining(&self) -> u64;
}

/// Log messages
pub trait Logger {
    fn log_enabled(&self) -> bool;

    /// Log a message.
    ///
    /// Unless explicitly stated, log messages are not considered stable and may change in the
    /// future as necessary
    fn log(&self, message: &str);
}

///
/// Stable program log messages
///
/// The format of these log messages should not be modified to avoid breaking downstream consumers
/// of program logging
///
pub mod stable_log {
    use super::*;

    /// Log a program invoke.
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program <address> invoke [<depth>]"
    /// ```
    pub fn program_invoke(
        logger: &Rc<RefCell<dyn Logger>>,
        program_id: &Pubkey,
        invoke_depth: usize,
    ) {
        ic_logger_msg!(logger, "Program {} invoke [{}]", program_id, invoke_depth);
    }

    /// Log a message from the program itself.
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program log: <program-generated output>"
    /// ```
    ///
    /// That is, any program-generated output is guaranteed to be prefixed by "Program log: "
    pub fn program_log(logger: &Rc<RefCell<dyn Logger>>, message: &str) {
        ic_logger_msg!(logger, "Program log: {}", message);
    }

    /// Emit a program data.
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program data: <binary-data-in-base64>*"
    /// ```
    ///
    /// That is, any program-generated output is guaranteed to be prefixed by "Program data: "
    pub fn program_data(logger: &Rc<RefCell<dyn Logger>>, data: &[&[u8]]) {
        ic_logger_msg!(
            logger,
            "Program data: {}",
            data.iter().map(base64::encode).join(" ")
        );
    }

    /// Log return data as from the program itself. This line will not be present if no return
    /// data was set, or if the return data was set to zero length.
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program return: <program-id> <program-generated-data-in-base64>"
    /// ```
    ///
    /// That is, any program-generated output is guaranteed to be prefixed by "Program return: "
    pub fn program_return(logger: &Rc<RefCell<dyn Logger>>, program_id: &Pubkey, data: &[u8]) {
        ic_logger_msg!(
            logger,
            "Program return: {} {}",
            program_id,
            base64::encode(data)
        );
    }

    /// Log successful program execution.
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program <address> success"
    /// ```
    pub fn program_success(logger: &Rc<RefCell<dyn Logger>>, program_id: &Pubkey) {
        ic_logger_msg!(logger, "Program {} success", program_id);
    }

    /// Log program execution failure
    ///
    /// The general form is:
    ///
    /// ```notrust
    /// "Program <address> failed: <program error details>"
    /// ```
    pub fn program_failure(
        logger: &Rc<RefCell<dyn Logger>>,
        program_id: &Pubkey,
        err: &InstructionError,
    ) {
        ic_logger_msg!(logger, "Program {} failed: {}", program_id, err);
    }
}

/// Program executor
pub trait Executor: Debug + Send + Sync {
    /// Execute the program
    fn execute(
        &self,
        first_instruction_account: usize,
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
        use_jit: bool,
    ) -> Result<(), InstructionError>;
}

#[derive(Debug, Default, Clone)]
pub struct MockComputeMeter {
    pub remaining: u64,
}
impl ComputeMeter for MockComputeMeter {
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

#[derive(Debug, Default, Clone)]
pub struct MockLogger {
    pub log: Rc<RefCell<Vec<String>>>,
}
impl Logger for MockLogger {
    fn log_enabled(&self) -> bool {
        true
    }
    fn log(&self, message: &str) {
        self.log.borrow_mut().push(message.to_string());
    }
}

#[allow(deprecated)]
pub struct MockInvokeContext<'a> {
    pub invoke_stack: Vec<InvokeContextStackFrame<'a>>,
    pub logger: MockLogger,
    pub compute_budget: ComputeBudget,
    pub compute_meter: Rc<RefCell<dyn ComputeMeter>>,
    pub programs: Vec<(Pubkey, ProcessInstructionWithContext)>,
    pub accounts: Vec<(Pubkey, Rc<RefCell<AccountSharedData>>)>,
    #[allow(clippy::type_complexity)]
    pub sysvars: RefCell<Vec<(Pubkey, Option<Rc<Vec<u8>>>)>>,
    pub disabled_features: HashSet<Pubkey>,
    pub blockhash: Hash,
    pub lamports_per_signature: u64,
    pub return_data: (Pubkey, Vec<u8>),
}

impl<'a> MockInvokeContext<'a> {
    pub fn new(program_id: &Pubkey, keyed_accounts: Vec<KeyedAccount<'a>>) -> Self {
        let compute_budget = ComputeBudget::default();
        let mut invoke_context = MockInvokeContext {
            invoke_stack: Vec::with_capacity(compute_budget.max_invoke_depth),
            logger: MockLogger::default(),
            compute_budget,
            compute_meter: Rc::new(RefCell::new(MockComputeMeter {
                remaining: std::i64::MAX as u64,
            })),
            programs: vec![],
            accounts: vec![],
            sysvars: RefCell::new(Vec::new()),
            disabled_features: HashSet::default(),
            blockhash: Hash::default(),
            lamports_per_signature: 0,
            return_data: (Pubkey::default(), Vec::new()),
        };
        let number_of_program_accounts = keyed_accounts
            .iter()
            .position(|keyed_account| keyed_account.unsigned_key() == program_id)
            .unwrap_or(0)
            .saturating_add(1);
        invoke_context
            .invoke_stack
            .push(InvokeContextStackFrame::new(
                number_of_program_accounts,
                keyed_accounts,
            ));
        invoke_context
    }
}

impl<'a> InvokeContext for MockInvokeContext<'a> {
    fn push(
        &mut self,
        _message: &Message,
        _instruction: &CompiledInstruction,
        _program_indices: &[usize],
        _account_indices: Option<&[usize]>,
    ) -> Result<(), InstructionError> {
        self.invoke_stack.push(InvokeContextStackFrame::new(
            0,
            create_keyed_accounts_unified(&[]),
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
        _message: &Message,
        _instruction: &CompiledInstruction,
        _program_indices: &[usize],
    ) -> Result<(), InstructionError> {
        Ok(())
    }
    fn verify_and_update(
        &mut self,
        _instruction: &CompiledInstruction,
        _account_indices: &[usize],
        _write_pivileges: &[bool],
    ) -> Result<(), InstructionError> {
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
        &self.programs
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        Rc::new(RefCell::new(self.logger.clone()))
    }
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
        self.compute_meter.clone()
    }
    fn add_executor(&self, _pubkey: &Pubkey, _executor: Arc<dyn Executor>) {}
    fn get_executor(&self, _pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        None
    }
    fn set_instruction_index(&mut self, _instruction_index: usize) {}
    fn record_instruction(&self, _instruction: &Instruction) {}
    fn is_feature_active(&self, feature_id: &Pubkey) -> bool {
        !self.disabled_features.contains(feature_id)
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
        _serialize_us: u64,
        _create_vm_us: u64,
        _execute_us: u64,
        _deserialize_us: u64,
    ) {
    }
    #[allow(clippy::type_complexity)]
    fn get_sysvars(&self) -> &RefCell<Vec<(Pubkey, Option<Rc<Vec<u8>>>)>> {
        &self.sysvars
    }
    fn get_sysvar_data(&self, id: &Pubkey) -> Option<Rc<Vec<u8>>> {
        self.sysvars
            .borrow()
            .iter()
            .find_map(|(key, sysvar)| if id == key { sysvar.clone() } else { None })
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
