#![cfg(feature = "full")]

use itertools::Itertools;
use solana_sdk::{
    account::AccountSharedData,
    compute_budget::ComputeBudget,
    hash::Hash,
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::KeyedAccount,
    message::Message,
    pubkey::Pubkey,
};
use std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};

pub type ProcessInstructionWithContext =
    fn(usize, &[u8], &mut dyn InvokeContext) -> Result<(), InstructionError>;

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
    fn get_sysvars(&self) -> &[(Pubkey, Vec<u8>)];
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
