//! Successors of instruction_context_context::StackFrame, KeyedAccount and AccountInfo

use crate::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    instruction::InstructionError,
    pubkey::Pubkey,
};
use std::cell::{RefCell, RefMut};

pub type TransactionAccount = (Pubkey, AccountSharedData);

#[derive(Clone, Debug)]
pub struct InstructionAccount {
    pub index: usize,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Loaded transaction shared between runtime and programs.
///
/// This context is valid for the entire duration of a transaction being processed.
pub struct TransactionContext {
    account_keys: Vec<Pubkey>,
    accounts: Vec<RefCell<AccountSharedData>>,
    instruction_context_capacity: usize,
    instruction_context_stack: Vec<InstructionContext>,
    return_data: (Pubkey, Vec<u8>),
}

impl TransactionContext {
    /// Constructs a new TransactionContext
    pub fn new(
        transaction_accounts: Vec<TransactionAccount>,
        instruction_context_capacity: usize,
    ) -> Self {
        let (account_keys, accounts) = transaction_accounts
            .into_iter()
            .map(|(key, account)| (key, RefCell::new(account)))
            .unzip();
        Self {
            account_keys,
            accounts,
            instruction_context_capacity,
            instruction_context_stack: Vec::with_capacity(instruction_context_capacity),
            return_data: (Pubkey::default(), Vec::new()),
        }
    }

    /// Used by the bank in the runtime to write back the processed accounts
    pub fn deconstruct(self) -> Vec<TransactionAccount> {
        self.account_keys
            .into_iter()
            .zip(
                self.accounts
                    .into_iter()
                    .map(|account| account.into_inner()),
            )
            .collect()
    }

    /// Used in mock_process_instruction
    pub fn deconstruct_without_keys(self) -> Result<Vec<AccountSharedData>, InstructionError> {
        if !self.instruction_context_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        Ok(self
            .accounts
            .into_iter()
            .map(|account| account.into_inner())
            .collect())
    }

    /// Returns the total number of accounts loaded in this Transaction
    pub fn get_number_of_accounts(&self) -> usize {
        self.accounts.len()
    }

    /// Searches for an account by its key
    pub fn get_key_of_account_at_index(&self, index_in_transaction: usize) -> &Pubkey {
        &self.account_keys[index_in_transaction]
    }

    /// Searches for an account by its key
    pub fn get_account_at_index(&self, index_in_transaction: usize) -> &RefCell<AccountSharedData> {
        &self.accounts[index_in_transaction]
    }

    /// Searches for an account by its key
    pub fn find_index_of_account(&self, pubkey: &Pubkey) -> Option<usize> {
        self.account_keys.iter().position(|key| key == pubkey)
    }

    /// Searches for a program account by its key
    pub fn find_index_of_program_account(&self, pubkey: &Pubkey) -> Option<usize> {
        self.account_keys.iter().rposition(|key| key == pubkey)
    }

    /// Gets an InstructionContext by its height in the stack
    pub fn get_instruction_context_at(
        &self,
        instruction_context_height: usize,
    ) -> Result<&InstructionContext, InstructionError> {
        if instruction_context_height >= self.instruction_context_stack.len() {
            return Err(InstructionError::CallDepth);
        }
        Ok(&self.instruction_context_stack[instruction_context_height])
    }

    /// Gets the max height of the InstructionContext stack
    pub fn get_instruction_context_capacity(&self) -> usize {
        self.instruction_context_capacity
    }

    /// Gets the height of the current InstructionContext
    pub fn get_instruction_context_height(&self) -> usize {
        self.instruction_context_stack.len().saturating_sub(1)
    }

    /// Returns the current InstructionContext
    pub fn get_current_instruction_context(&self) -> Result<&InstructionContext, InstructionError> {
        self.get_instruction_context_at(self.get_instruction_context_height())
    }

    /// Gets the last program account of the current InstructionContext
    pub fn try_borrow_program_account(&self) -> Result<BorrowedAccount, InstructionError> {
        let instruction_context = self.get_current_instruction_context()?;
        instruction_context.try_borrow_account(
            self,
            instruction_context
                .number_of_program_accounts
                .saturating_sub(1),
        )
    }

    /// Gets an instruction account of the current InstructionContext
    pub fn try_borrow_instruction_account(
        &self,
        index_in_instruction: usize,
    ) -> Result<BorrowedAccount, InstructionError> {
        let instruction_context = self.get_current_instruction_context()?;
        instruction_context.try_borrow_account(
            self,
            instruction_context
                .number_of_program_accounts
                .saturating_add(index_in_instruction),
        )
    }

    /// Pushes a new InstructionContext
    pub fn push(
        &mut self,
        number_of_program_accounts: usize,
        instruction_accounts: &[InstructionAccount],
        instruction_data: Vec<u8>,
    ) -> Result<(), InstructionError> {
        if self.instruction_context_stack.len() >= self.instruction_context_capacity {
            return Err(InstructionError::CallDepth);
        }
        let mut result = InstructionContext {
            instruction_data,
            number_of_program_accounts,
            account_indices: Vec::with_capacity(instruction_accounts.len()),
            account_is_signer: Vec::with_capacity(instruction_accounts.len()),
            account_is_writable: Vec::with_capacity(instruction_accounts.len()),
        };
        for instruction_account in instruction_accounts.iter() {
            result.account_indices.push(instruction_account.index);
            result.account_is_signer.push(instruction_account.is_signer);
            result
                .account_is_writable
                .push(instruction_account.is_writable);
        }
        self.instruction_context_stack.push(result);
        Ok(())
    }

    /// Pops the current InstructionContext
    pub fn pop(&mut self) -> Result<(), InstructionError> {
        if self.instruction_context_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        self.instruction_context_stack.pop();
        Ok(())
    }

    /// Gets the return data of the current InstructionContext or any above
    pub fn get_return_data(&self) -> (&Pubkey, &[u8]) {
        (&self.return_data.0, &self.return_data.1)
    }

    /// Set the return data of the current InstructionContext
    pub fn set_return_data(&mut self, data: Vec<u8>) -> Result<(), InstructionError> {
        let pubkey = *self.try_borrow_program_account()?.get_key();
        self.return_data = (pubkey, data);
        Ok(())
    }
}

/// Loaded instruction shared between runtime and programs.
///
/// This context is valid for the entire duration of a (possibly cross program) instruction being processed.
pub struct InstructionContext {
    number_of_program_accounts: usize,
    account_indices: Vec<usize>,
    account_is_signer: Vec<bool>,
    account_is_writable: Vec<bool>,
    instruction_data: Vec<u8>,
}

impl InstructionContext {
    /// Number of program accounts
    pub fn get_number_of_program_accounts(&self) -> usize {
        self.number_of_program_accounts
    }

    /// Number of accounts in this Instruction (without program accounts)
    pub fn get_number_of_instruction_accounts(&self) -> usize {
        self.account_indices
            .len()
            .saturating_sub(self.number_of_program_accounts)
    }

    /// Total number of accounts in this Instruction (with program accounts)
    pub fn get_total_number_of_accounts(&self) -> usize {
        self.account_indices.len()
    }

    /// Data parameter for the programs `process_instruction` handler
    pub fn get_instruction_data(&self) -> &[u8] {
        &self.instruction_data
    }

    /// Tries to borrow an account from this Instruction
    pub fn try_borrow_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        index_in_instruction: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        if index_in_instruction >= self.account_indices.len() {
            return Err(InstructionError::NotEnoughAccountKeys);
        }
        let index_in_transaction = self.account_indices[index_in_instruction];
        if index_in_transaction >= transaction_context.accounts.len() {
            return Err(InstructionError::MissingAccount);
        }
        let account = transaction_context.accounts[index_in_transaction]
            .try_borrow_mut()
            .map_err(|_| InstructionError::AccountBorrowFailed)?;
        Ok(BorrowedAccount {
            transaction_context,
            instruction_context: self,
            index_in_transaction,
            index_in_instruction,
            account,
        })
    }
}

/// Shared account borrowed from the TransactionContext and an InstructionContext.
pub struct BorrowedAccount<'a> {
    transaction_context: &'a TransactionContext,
    instruction_context: &'a InstructionContext,
    index_in_transaction: usize,
    index_in_instruction: usize,
    account: RefMut<'a, AccountSharedData>,
}

impl<'a> BorrowedAccount<'a> {
    /// Returns the public key of this account (transaction wide)
    pub fn get_key(&self) -> &Pubkey {
        &self.transaction_context.account_keys[self.index_in_transaction]
    }

    /// Returns the owner of this account (transaction wide)
    pub fn get_owner(&self) -> &Pubkey {
        self.account.owner()
    }

    /// Assignes the owner of this account (transaction wide)
    pub fn set_owner(&mut self, pubkey: Pubkey) -> Result<(), InstructionError> {
        if !self.is_writable() {
            return Err(InstructionError::Immutable);
        }
        self.account.set_owner(pubkey);
        Ok(())
    }

    /// Returns the number of lamports of this account (transaction wide)
    pub fn get_lamports(&self) -> u64 {
        self.account.lamports()
    }

    /// Overwrites the number of lamports of this account (transaction wide)
    pub fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        if !self.is_writable() {
            return Err(InstructionError::Immutable);
        }
        self.account.set_lamports(lamports);
        Ok(())
    }

    /// Returns a read-only slice of the account data (transaction wide)
    pub fn get_data(&self) -> &[u8] {
        self.account.data()
    }

    /// Returns a writable slice of the account data (transaction wide)
    pub fn get_data_mut(&mut self) -> Result<&mut [u8], InstructionError> {
        if !self.is_writable() {
            return Err(InstructionError::Immutable);
        }
        Ok(self.account.data_as_mut_slice())
    }

    /*pub fn realloc(&self, new_len: usize, zero_init: bool) {
        // TODO
    }*/

    /// Returns whether this account is executable (transaction wide)
    pub fn is_executable(&self) -> bool {
        self.account.executable()
    }

    /// Configures whether this account is executable (transaction wide)
    pub fn set_executable(&mut self, is_executable: bool) -> Result<(), InstructionError> {
        if !self.is_writable() {
            return Err(InstructionError::Immutable);
        }
        self.account.set_executable(is_executable);
        Ok(())
    }

    /// Returns whether this account is a signer (instruction wide)
    pub fn is_signer(&self) -> bool {
        self.instruction_context.account_is_signer[self.index_in_instruction]
    }

    /// Returns whether this account is writable (instruction wide)
    pub fn is_writable(&self) -> bool {
        self.instruction_context.account_is_writable[self.index_in_instruction]
    }
}
