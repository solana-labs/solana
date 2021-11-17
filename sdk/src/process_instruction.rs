#![cfg(feature = "full")]

use solana_sdk::{
    account::AccountSharedData,
    compute_budget::ComputeBudget,
    hash::Hash,
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::KeyedAccount,
    message::Message,
    pubkey::Pubkey,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

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

/// Compute meter
pub trait ComputeMeter {
    /// Consume compute units
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError>;
    /// Get the number of remaining compute units
    fn get_remaining(&self) -> u64;
}
