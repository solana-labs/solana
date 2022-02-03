//! Successors of instruction_context_context::StackFrame, KeyedAccount and AccountInfo

use crate::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
    lamports::LamportsError,
    pubkey::Pubkey,
};
use std::{
    cell::{RefCell, RefMut},
    collections::HashSet,
    pin::Pin,
};

pub type TransactionAccount = (Pubkey, AccountSharedData);

#[derive(Clone, Debug)]
pub struct InstructionAccount {
    pub index_in_transaction: usize,
    pub index_in_caller: usize,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Loaded transaction shared between runtime and programs.
///
/// This context is valid for the entire duration of a transaction being processed.
#[derive(Debug)]
pub struct TransactionContext {
    account_keys: Pin<Box<[Pubkey]>>,
    accounts: Pin<Box<[RefCell<AccountSharedData>]>>,
    instruction_context_capacity: usize,
    instruction_context_stack: Vec<InstructionContext>,
    number_of_instructions_at_transaction_level: usize,
    instruction_trace: InstructionTrace,
    return_data: (Pubkey, Vec<u8>),
}

impl TransactionContext {
    /// Constructs a new TransactionContext
    pub fn new(
        transaction_accounts: Vec<TransactionAccount>,
        instruction_context_capacity: usize,
        number_of_instructions_at_transaction_level: usize,
    ) -> Self {
        let (account_keys, accounts): (Vec<Pubkey>, Vec<RefCell<AccountSharedData>>) =
            transaction_accounts
                .into_iter()
                .map(|(key, account)| (key, RefCell::new(account)))
                .unzip();
        Self {
            account_keys: Pin::new(account_keys.into_boxed_slice()),
            accounts: Pin::new(accounts.into_boxed_slice()),
            instruction_context_capacity,
            instruction_context_stack: Vec::with_capacity(instruction_context_capacity),
            number_of_instructions_at_transaction_level,
            instruction_trace: Vec::with_capacity(number_of_instructions_at_transaction_level),
            return_data: (Pubkey::default(), Vec::new()),
        }
    }

    /// Used by the bank in the runtime to write back the processed accounts and recorded instructions
    pub fn deconstruct(
        self,
    ) -> (
        Vec<TransactionAccount>,
        Vec<Vec<(usize, InstructionContext)>>,
    ) {
        (
            Vec::from(Pin::into_inner(self.account_keys))
                .into_iter()
                .zip(
                    Vec::from(Pin::into_inner(self.accounts))
                        .into_iter()
                        .map(|account| account.into_inner()),
                )
                .collect(),
            self.instruction_trace,
        )
    }

    /// Used in mock_process_instruction
    pub fn deconstruct_without_keys(self) -> Result<Vec<AccountSharedData>, InstructionError> {
        if !self.instruction_context_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        Ok(Vec::from(Pin::into_inner(self.accounts))
            .into_iter()
            .map(|account| account.into_inner())
            .collect())
    }

    /// Returns the total number of accounts loaded in this Transaction
    pub fn get_number_of_accounts(&self) -> usize {
        self.accounts.len()
    }

    /// Searches for an account by its key
    pub fn get_key_of_account_at_index(
        &self,
        index_in_transaction: usize,
    ) -> Result<&Pubkey, InstructionError> {
        self.account_keys
            .get(index_in_transaction)
            .ok_or(InstructionError::NotEnoughAccountKeys)
    }

    /// Searches for an account by its key
    pub fn get_account_at_index(
        &self,
        index_in_transaction: usize,
    ) -> Result<&RefCell<AccountSharedData>, InstructionError> {
        self.accounts
            .get(index_in_transaction)
            .ok_or(InstructionError::NotEnoughAccountKeys)
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
        level: usize,
    ) -> Result<&InstructionContext, InstructionError> {
        if level >= self.instruction_context_stack.len() {
            return Err(InstructionError::CallDepth);
        }
        Ok(&self.instruction_context_stack[level])
    }

    /// Gets the max height of the InstructionContext stack
    pub fn get_instruction_context_capacity(&self) -> usize {
        self.instruction_context_capacity
    }

    /// Gets instruction stack height, top-level instructions are height
    /// `solana_sdk::instruction::TRANSACTION_LEVEL_STACK_HEIGHT`
    pub fn get_instruction_context_stack_height(&self) -> usize {
        self.instruction_context_stack.len()
    }

    /// Returns the current InstructionContext
    pub fn get_current_instruction_context(&self) -> Result<&InstructionContext, InstructionError> {
        let level = self
            .instruction_context_stack
            .len()
            .checked_sub(1)
            .ok_or(InstructionError::CallDepth)?;
        self.get_instruction_context_at(level)
    }

    /// Pushes a new InstructionContext
    pub fn push(
        &mut self,
        program_accounts: &[usize],
        instruction_accounts: &[InstructionAccount],
        instruction_data: &[u8],
    ) -> Result<(), InstructionError> {
        if self.instruction_context_stack.len() >= self.instruction_context_capacity {
            return Err(InstructionError::CallDepth);
        }

        let instruction_context = InstructionContext {
            program_accounts: program_accounts.to_vec(),
            instruction_accounts: instruction_accounts.to_vec(),
            instruction_data: instruction_data.to_vec(),
        };
        if self.instruction_context_stack.is_empty() {
            debug_assert!(
                self.instruction_trace.len() < self.number_of_instructions_at_transaction_level
            );
            self.instruction_trace.push(vec![(
                TRANSACTION_LEVEL_STACK_HEIGHT,
                instruction_context.clone(),
            )]);
        }

        self.instruction_context_stack.push(instruction_context);
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

    /// Returns the key of the current InstructionContexts program account
    pub fn get_program_key(&self) -> Result<&Pubkey, InstructionError> {
        let instruction_context = self.get_current_instruction_context()?;
        let program_account = instruction_context.try_borrow_program_account(self)?;
        Ok(&self.account_keys[program_account.index_in_transaction])
    }

    /// Returns the owner of the current InstructionContexts program account
    pub fn get_loader_key(&self) -> Result<Pubkey, InstructionError> {
        let instruction_context = self.get_current_instruction_context()?;
        let program_account = instruction_context.try_borrow_program_account(self)?;
        Ok(*program_account.get_owner())
    }

    /// Gets the return data of the current InstructionContext or any above
    pub fn get_return_data(&self) -> (&Pubkey, &[u8]) {
        (&self.return_data.0, &self.return_data.1)
    }

    /// Set the return data of the current InstructionContext
    pub fn set_return_data(
        &mut self,
        program_id: Pubkey,
        data: Vec<u8>,
    ) -> Result<(), InstructionError> {
        self.return_data = (program_id, data);
        Ok(())
    }

    /// Used by the runtime when a new CPI instruction begins
    pub fn record_instruction(&mut self, stack_height: usize, instruction: InstructionContext) {
        if let Some(records) = self.instruction_trace.last_mut() {
            records.push((stack_height, instruction));
        }
    }

    /// Returns instruction trace
    pub fn get_instruction_trace(&self) -> &InstructionTrace {
        &self.instruction_trace
    }
}

/// List of (stack height, instruction) for each top-level instruction
pub type InstructionTrace = Vec<Vec<(usize, InstructionContext)>>;

#[derive(Clone, Debug)]
pub struct AccountMeta {
    pub index_in_transaction: usize,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Loaded instruction shared between runtime and programs.
///
/// This context is valid for the entire duration of a (possibly cross program) instruction being processed.
#[derive(Debug, Clone)]
pub struct InstructionContext {
    program_accounts: Vec<usize>,
    instruction_accounts: Vec<InstructionAccount>,
    instruction_data: Vec<u8>,
}

impl InstructionContext {
    /// New
    pub fn new(
        program_accounts: &[usize],
        instruction_accounts: &[InstructionAccount],
        instruction_data: &[u8],
    ) -> Self {
        InstructionContext {
            program_accounts: program_accounts.to_vec(),
            instruction_accounts: instruction_accounts.to_vec(),
            instruction_data: instruction_data.to_vec(),
        }
    }

    /// Number of program accounts
    pub fn get_number_of_program_accounts(&self) -> usize {
        self.program_accounts.len()
    }

    /// Get the index of the instruction's program id
    pub fn get_program_id_index(&self) -> usize {
        self.program_accounts.last().cloned().unwrap_or_default()
    }

    /// Get the instruction's program id
    pub fn get_program_id(&self, transaction_context: &TransactionContext) -> Pubkey {
        transaction_context.account_keys[self.program_accounts.last().cloned().unwrap_or_default()]
    }

    /// Number of accounts in this Instruction (without program accounts)
    pub fn get_number_of_instruction_accounts(&self) -> usize {
        self.instruction_accounts.len()
    }

    pub fn get_instruction_accounts_metas(&self) -> Vec<AccountMeta> {
        self.instruction_accounts
            .iter()
            .map(|instruction_account| AccountMeta {
                index_in_transaction: instruction_account.index_in_transaction,
                is_signer: instruction_account.is_signer,
                is_writable: instruction_account.is_writable,
            })
            .collect()
    }

    /// Number of accounts in this Instruction
    pub fn get_number_of_accounts(&self) -> usize {
        self.program_accounts
            .len()
            .saturating_add(self.instruction_accounts.len())
    }

    /// Data parameter for the programs `process_instruction` handler
    pub fn get_instruction_data(&self) -> &[u8] {
        &self.instruction_data
    }

    /// Searches for a program account by its key
    pub fn find_index_of_program_account(
        &self,
        transaction_context: &TransactionContext,
        pubkey: &Pubkey,
    ) -> Option<usize> {
        self.program_accounts
            .iter()
            .position(|index_in_transaction| {
                &transaction_context.account_keys[*index_in_transaction] == pubkey
            })
    }

    /// Searches for an account by its key
    pub fn find_index_of_account(
        &self,
        transaction_context: &TransactionContext,
        pubkey: &Pubkey,
    ) -> Option<usize> {
        self.instruction_accounts
            .iter()
            .position(|instruction_account| {
                &transaction_context.account_keys[instruction_account.index_in_transaction]
                    == pubkey
            })
            .map(|index| index.saturating_add(self.program_accounts.len()))
    }

    /// Translates the given instruction wide index into a transaction wide index
    pub fn get_index_in_transaction(
        &self,
        index_in_instruction: usize,
    ) -> Result<usize, InstructionError> {
        if index_in_instruction < self.program_accounts.len() {
            Ok(self.program_accounts[index_in_instruction])
        } else if index_in_instruction < self.get_number_of_accounts() {
            Ok(self.instruction_accounts
                [index_in_instruction.saturating_sub(self.program_accounts.len())]
            .index_in_transaction)
        } else {
            Err(InstructionError::NotEnoughAccountKeys)
        }
    }

    /// Tries to borrow an account from this Instruction
    pub fn try_borrow_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        index_in_instruction: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let index_in_transaction = self.get_index_in_transaction(index_in_instruction)?;
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

    /// Gets the last program account of this Instruction
    pub fn try_borrow_program_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        self.try_borrow_account(
            transaction_context,
            self.program_accounts.len().saturating_sub(1),
        )
    }

    /// Gets an instruction account of this Instruction
    pub fn try_borrow_instruction_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        index_in_instruction: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        self.try_borrow_account(
            transaction_context,
            self.program_accounts
                .len()
                .saturating_add(index_in_instruction),
        )
    }

    /// Calculates the set of all keys of signer accounts in this Instruction
    pub fn get_signers(&self, transaction_context: &TransactionContext) -> HashSet<Pubkey> {
        let mut result = HashSet::new();
        for instruction_account in self.instruction_accounts.iter() {
            if instruction_account.is_signer {
                result.insert(
                    transaction_context.account_keys[instruction_account.index_in_transaction],
                );
            }
        }
        result
    }

    /// Returns whether an account is a signer
    pub fn is_signer(&self, index_in_instruction: usize) -> bool {
        if index_in_instruction < self.program_accounts.len() {
            false
        } else {
            self.instruction_accounts
                [index_in_instruction.saturating_sub(self.program_accounts.len())]
            .is_signer
        }
    }

    /// Returns whether an account is writable
    pub fn is_writable(&self, index_in_instruction: usize) -> bool {
        if index_in_instruction < self.program_accounts.len() {
            false
        } else {
            self.instruction_accounts
                [index_in_instruction.saturating_sub(self.program_accounts.len())]
            .is_writable
        }
    }
}

/// Shared account borrowed from the TransactionContext and an InstructionContext.
#[derive(Debug)]
pub struct BorrowedAccount<'a> {
    transaction_context: &'a TransactionContext,
    instruction_context: &'a InstructionContext,
    index_in_transaction: usize,
    index_in_instruction: usize,
    account: RefMut<'a, AccountSharedData>,
}

impl<'a> BorrowedAccount<'a> {
    /// Returns the index of this account (transaction wide)
    pub fn get_index_in_transaction(&self) -> usize {
        self.index_in_transaction
    }

    /// Returns the index of this account (instruction wide)
    pub fn get_index_in_instruction(&self) -> usize {
        self.index_in_instruction
    }

    /// Returns the public key of this account (transaction wide)
    pub fn get_key(&self) -> &Pubkey {
        &self.transaction_context.account_keys[self.index_in_transaction]
    }

    /// Returns the owner of this account (transaction wide)
    pub fn get_owner(&self) -> &Pubkey {
        self.account.owner()
    }

    /// Assignes the owner of this account (transaction wide)
    pub fn set_owner(&mut self, pubkey: &[u8]) -> Result<(), InstructionError> {
        self.account.copy_into_owner_from_slice(pubkey);
        self.verify_writability()
        // TODO: return Err(InstructionError::ModifiedProgramId);
    }

    /// Returns the number of lamports of this account (transaction wide)
    pub fn get_lamports(&self) -> u64 {
        self.account.lamports()
    }

    /// Overwrites the number of lamports of this account (transaction wide)
    pub fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.account.set_lamports(lamports);
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            return Err(InstructionError::ExecutableLamportChange);
        }
        if !self.is_writable() {
            return Err(InstructionError::ReadonlyLamportChange);
        }
        // TODO: return Err(InstructionError::ExternalAccountLamportSpend);
        Ok(())
    }

    /// Adds lamports to this account (transaction wide)
    pub fn checked_add_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_add(lamports)
                .ok_or(LamportsError::ArithmeticOverflow)?,
        )
    }

    /// Subtracts lamports from this account (transaction wide)
    pub fn checked_sub_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_sub(lamports)
                .ok_or(LamportsError::ArithmeticUnderflow)?,
        )
    }

    /// Verifies that this account is writable (instruction wide)
    fn verify_writability(&self) -> Result<(), InstructionError> {
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            return Err(InstructionError::ExecutableDataModified);
        }
        if !self.is_writable() {
            return Err(InstructionError::ReadonlyDataModified);
        }
        // TODO: return Err(InstructionError::ExternalAccountDataModified);
        Ok(())
    }

    /// Returns a read-only slice of the account data (transaction wide)
    pub fn get_data(&self) -> &[u8] {
        self.account.data()
    }

    /// Overwrites the account data and size (transaction wide)
    pub fn set_data(&mut self, data: &[u8]) -> Result<(), InstructionError> {
        if data.len() == self.account.data().len() {
            self.account.data_as_mut_slice().copy_from_slice(data);
        } else {
            self.account.set_data_from_slice(data);
            // TODO: return Err(InstructionError::AccountDataSizeChanged);
        }
        self.verify_writability()
    }

    /*pub fn realloc(&self, new_len: usize, zero_init: bool) {
        // TODO: return Err(InstructionError::InvalidRealloc);
        // TODO: return Err(InstructionError::AccountDataSizeChanged);
    }*/

    /// Deserializes the account data into a state
    pub fn get_state<T: serde::de::DeserializeOwned>(&self) -> Result<T, InstructionError> {
        self.account
            .deserialize_data()
            .map_err(|_| InstructionError::InvalidAccountData)
    }

    /// Serializes a state into the account data
    pub fn set_state<T: serde::Serialize>(&mut self, state: &T) -> Result<(), InstructionError> {
        let data = self.account.data_as_mut_slice();
        let serialized_size =
            bincode::serialized_size(state).map_err(|_| InstructionError::GenericError)?;
        if serialized_size > data.len() as u64 {
            return Err(InstructionError::AccountDataTooSmall);
        }
        bincode::serialize_into(&mut *data, state).map_err(|_| InstructionError::GenericError)?;
        self.verify_writability()
    }

    /// Returns whether this account is executable (transaction wide)
    pub fn is_executable(&self) -> bool {
        self.account.executable()
    }

    /// Configures whether this account is executable (transaction wide)
    pub fn set_executable(&mut self, is_executable: bool) -> Result<(), InstructionError> {
        self.account.set_executable(is_executable);
        self.verify_writability()
        // TODO: return Err(InstructionError::ExecutableAccountNotRentExempt);
        // TODO: return Err(InstructionError::ExecutableModified);
    }

    /// Returns the rent epoch of this account (transaction wide)
    pub fn get_rent_epoch(&self) -> u64 {
        self.account.rent_epoch()
    }

    /// Returns whether this account is a signer (instruction wide)
    pub fn is_signer(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            false
        } else {
            self.instruction_context.instruction_accounts[self
                .index_in_instruction
                .saturating_sub(self.instruction_context.program_accounts.len())]
            .is_signer
        }
    }

    /// Returns whether this account is writable (instruction wide)
    pub fn is_writable(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            false
        } else {
            self.instruction_context.instruction_accounts[self
                .index_in_instruction
                .saturating_sub(self.instruction_context.program_accounts.len())]
            .is_writable
        }
    }
}
