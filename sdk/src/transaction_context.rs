//! Successors of instruction_context_context::StackFrame, KeyedAccount and AccountInfo

use {
    crate::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        instruction::InstructionError,
        pubkey::Pubkey,
    },
    std::{
        cell::{RefCell, RefMut},
        collections::HashSet,
        pin::Pin,
    },
};

pub type TransactionAccount = (Pubkey, AccountSharedData);

/// Contains account meta data which varies between instruction.
///
/// It also contains indices to other structures for faster lookup.
#[derive(Clone, Debug)]
pub struct InstructionAccount {
    /// Points to the account and its key in the `TransactionContext`
    pub index_in_transaction: usize,
    /// Points to the first occurrence in the parent `InstructionContext`
    ///
    /// This excludes the program accounts.
    pub index_in_caller: usize,
    /// Points to the first occurrence in the current `InstructionContext`
    ///
    /// This excludes the program accounts.
    pub index_in_callee: usize,
    /// Is this account supposed to sign
    pub is_signer: bool,
    /// Is this account allowed to become writable
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
    instruction_stack: Vec<usize>,
    number_of_instructions_at_transaction_level: usize,
    instruction_trace: InstructionTrace,
    return_data: TransactionReturnData,
    total_resize_limit: u64,
    total_resize_delta: RefCell<i64>,
}

impl TransactionContext {
    /// Constructs a new TransactionContext
    pub fn new(
        transaction_accounts: Vec<TransactionAccount>,
        instruction_context_capacity: usize,
        number_of_instructions_at_transaction_level: usize,
        total_resize_limit: u64,
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
            instruction_stack: Vec::with_capacity(instruction_context_capacity),
            number_of_instructions_at_transaction_level,
            instruction_trace: Vec::with_capacity(number_of_instructions_at_transaction_level),
            return_data: TransactionReturnData::default(),
            total_resize_limit,
            total_resize_delta: RefCell::new(0),
        }
    }

    /// Used in mock_process_instruction
    pub fn deconstruct_without_keys(self) -> Result<Vec<AccountSharedData>, InstructionError> {
        if !self.instruction_stack.is_empty() {
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

    /// Returns the keys for the accounts loaded in this Transaction
    pub fn get_keys_of_accounts(&self) -> &[Pubkey] {
        &self.account_keys
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

    /// Gets an InstructionContext by its nesting level in the stack
    pub fn get_instruction_context_at(
        &self,
        level: usize,
    ) -> Result<&InstructionContext, InstructionError> {
        let top_level_index = *self
            .instruction_stack
            .first()
            .ok_or(InstructionError::CallDepth)?;
        let cpi_index = if level == 0 {
            0
        } else {
            *self
                .instruction_stack
                .get(level)
                .ok_or(InstructionError::CallDepth)?
        };
        let instruction_context = self
            .instruction_trace
            .get(top_level_index)
            .and_then(|instruction_trace| instruction_trace.get(cpi_index))
            .ok_or(InstructionError::CallDepth)?;
        debug_assert_eq!(instruction_context.nesting_level, level);
        Ok(instruction_context)
    }

    /// Gets the max height of the InstructionContext stack
    pub fn get_instruction_context_capacity(&self) -> usize {
        self.instruction_context_capacity
    }

    /// Gets instruction stack height, top-level instructions are height
    /// `solana_sdk::instruction::TRANSACTION_LEVEL_STACK_HEIGHT`
    pub fn get_instruction_context_stack_height(&self) -> usize {
        self.instruction_stack.len()
    }

    /// Returns the current InstructionContext
    pub fn get_current_instruction_context(&self) -> Result<&InstructionContext, InstructionError> {
        let level = self
            .get_instruction_context_stack_height()
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
        record_instruction_in_transaction_context_push: bool,
    ) -> Result<(), InstructionError> {
        let index_in_trace = if self.instruction_stack.is_empty() {
            debug_assert!(
                self.instruction_trace.len() < self.number_of_instructions_at_transaction_level
            );
            let instruction_context = InstructionContext {
                nesting_level: self.instruction_stack.len(),
                program_accounts: program_accounts.to_vec(),
                instruction_accounts: instruction_accounts.to_vec(),
                instruction_data: instruction_data.to_vec(),
            };
            self.instruction_trace.push(vec![instruction_context]);
            self.instruction_trace.len().saturating_sub(1)
        } else if let Some(instruction_trace) = self.instruction_trace.last_mut() {
            if record_instruction_in_transaction_context_push {
                let instruction_context = InstructionContext {
                    nesting_level: self.instruction_stack.len(),
                    program_accounts: program_accounts.to_vec(),
                    instruction_accounts: instruction_accounts.to_vec(),
                    instruction_data: instruction_data.to_vec(),
                };
                instruction_trace.push(instruction_context);
            }
            instruction_trace.len().saturating_sub(1)
        } else {
            return Err(InstructionError::CallDepth);
        };
        if self.instruction_stack.len() >= self.instruction_context_capacity {
            return Err(InstructionError::CallDepth);
        }
        self.instruction_stack.push(index_in_trace);
        Ok(())
    }

    /// Pops the current InstructionContext
    pub fn pop(&mut self) -> Result<(), InstructionError> {
        if self.instruction_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        self.instruction_stack.pop();
        Ok(())
    }

    /// Gets the return data of the current InstructionContext or any above
    pub fn get_return_data(&self) -> (&Pubkey, &[u8]) {
        (&self.return_data.program_id, &self.return_data.data)
    }

    /// Set the return data of the current InstructionContext
    pub fn set_return_data(
        &mut self,
        program_id: Pubkey,
        data: Vec<u8>,
    ) -> Result<(), InstructionError> {
        self.return_data = TransactionReturnData { program_id, data };
        Ok(())
    }

    /// Used by the runtime when a new CPI instruction begins
    ///
    /// Deprecated, automatically done in push()
    /// once record_instruction_in_transaction_context_push is activated.
    pub fn record_instruction(&mut self, instruction: InstructionContext) {
        if let Some(records) = self.instruction_trace.last_mut() {
            records.push(instruction);
        }
    }

    /// Returns instruction trace
    pub fn get_instruction_trace(&self) -> &InstructionTrace {
        &self.instruction_trace
    }

    /// Returns (in bytes) how much data can still be allocated
    pub fn get_total_resize_remaining(&self) -> u64 {
        let total_resize_delta = *self.total_resize_delta.borrow();
        if total_resize_delta >= 0 {
            self.total_resize_limit
                .saturating_sub(total_resize_delta as u64)
        } else {
            self.total_resize_limit
                .saturating_add(total_resize_delta.saturating_neg() as u64)
        }
    }
}

/// Return data at the end of a transaction
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TransactionReturnData {
    pub program_id: Pubkey,
    pub data: Vec<u8>,
}

/// List of (stack height, instruction) for each top-level instruction
pub type InstructionTrace = Vec<Vec<InstructionContext>>;

/// Loaded instruction shared between runtime and programs.
///
/// This context is valid for the entire duration of a (possibly cross program) instruction being processed.
#[derive(Debug, Clone)]
pub struct InstructionContext {
    nesting_level: usize,
    program_accounts: Vec<usize>,
    instruction_accounts: Vec<InstructionAccount>,
    instruction_data: Vec<u8>,
}

impl InstructionContext {
    /// New
    pub fn new(
        nesting_level: usize,
        program_accounts: &[usize],
        instruction_accounts: &[InstructionAccount],
        instruction_data: &[u8],
    ) -> Self {
        InstructionContext {
            nesting_level,
            program_accounts: program_accounts.to_vec(),
            instruction_accounts: instruction_accounts.to_vec(),
            instruction_data: instruction_data.to_vec(),
        }
    }

    /// How many Instructions were on the stack after this one was pushed
    ///
    /// That is the number of nested parent Instructions plus one (itself).
    pub fn get_stack_height(&self) -> usize {
        self.nesting_level.saturating_add(1)
    }

    /// Number of program accounts
    pub fn get_number_of_program_accounts(&self) -> usize {
        self.program_accounts.len()
    }

    /// Number of accounts in this Instruction (without program accounts)
    pub fn get_number_of_instruction_accounts(&self) -> usize {
        self.instruction_accounts.len()
    }

    /// Assert that enough account were supplied to this Instruction
    pub fn check_number_of_instruction_accounts(
        &self,
        expected_at_least: usize,
    ) -> Result<(), InstructionError> {
        if self.get_number_of_instruction_accounts() < expected_at_least {
            Err(InstructionError::NotEnoughAccountKeys)
        } else {
            Ok(())
        }
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

    /// Searches for an instruction account by its key
    pub fn find_index_of_instruction_account(
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
    }

    /// Translates the given instruction wide program_account_index into a transaction wide index
    pub fn get_index_of_program_account_in_transaction(
        &self,
        program_account_index: usize,
    ) -> Result<usize, InstructionError> {
        Ok(*self
            .program_accounts
            .get(program_account_index)
            .ok_or(InstructionError::NotEnoughAccountKeys)?)
    }

    /// Translates the given instruction wide instruction_account_index into a transaction wide index
    pub fn get_index_of_instruction_account_in_transaction(
        &self,
        instruction_account_index: usize,
    ) -> Result<usize, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index)
            .ok_or(InstructionError::NotEnoughAccountKeys)?
            .index_in_transaction)
    }

    /// Returns `Some(instruction_account_index)` if this is a duplicate
    /// and `None` if it is the first account with this key
    pub fn is_instruction_account_duplicate(
        &self,
        instruction_account_index: usize,
    ) -> Result<Option<usize>, InstructionError> {
        let index_in_callee = self
            .instruction_accounts
            .get(instruction_account_index)
            .ok_or(InstructionError::NotEnoughAccountKeys)?
            .index_in_callee;
        Ok(if index_in_callee == instruction_account_index {
            None
        } else {
            Some(index_in_callee)
        })
    }

    /// Gets the key of the last program account of this Instruction
    pub fn get_last_program_key<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
    ) -> Result<&'b Pubkey, InstructionError> {
        let result = self
            .get_index_of_program_account_in_transaction(
                self.program_accounts.len().saturating_sub(1),
            )
            .and_then(|index_in_transaction| {
                transaction_context.get_key_of_account_at_index(index_in_transaction)
            });
        debug_assert!(result.is_ok());
        result
    }

    fn try_borrow_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        index_in_transaction: usize,
        index_in_instruction: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let account = transaction_context
            .accounts
            .get(index_in_transaction)
            .ok_or(InstructionError::MissingAccount)?
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
    pub fn try_borrow_last_program_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let result = self.try_borrow_program_account(
            transaction_context,
            self.program_accounts.len().saturating_sub(1),
        );
        debug_assert!(result.is_ok());
        result
    }

    /// Tries to borrow a program account from this Instruction
    pub fn try_borrow_program_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        program_account_index: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let index_in_transaction =
            self.get_index_of_program_account_in_transaction(program_account_index)?;
        self.try_borrow_account(
            transaction_context,
            index_in_transaction,
            program_account_index,
        )
    }

    /// Gets an instruction account of this Instruction
    pub fn try_borrow_instruction_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        instruction_account_index: usize,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let index_in_transaction =
            self.get_index_of_instruction_account_in_transaction(instruction_account_index)?;
        self.try_borrow_account(
            transaction_context,
            index_in_transaction,
            self.program_accounts
                .len()
                .saturating_add(instruction_account_index),
        )
    }

    /// Returns whether an instruction account is a signer
    pub fn is_instruction_account_signer(
        &self,
        instruction_account_index: usize,
    ) -> Result<bool, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index)
            .ok_or(InstructionError::MissingAccount)?
            .is_signer)
    }

    /// Returns whether an instruction account is writable
    pub fn is_instruction_account_writable(
        &self,
        instruction_account_index: usize,
    ) -> Result<bool, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index)
            .ok_or(InstructionError::MissingAccount)?
            .is_writable)
    }

    /// Calculates the set of all keys of signer instruction accounts in this Instruction
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
        Ok(())
    }

    /// Returns the number of lamports of this account (transaction wide)
    pub fn get_lamports(&self) -> u64 {
        self.account.lamports()
    }

    /// Overwrites the number of lamports of this account (transaction wide)
    pub fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.account.set_lamports(lamports);
        Ok(())
    }

    /// Adds lamports to this account (transaction wide)
    pub fn checked_add_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_add(lamports)
                .ok_or(InstructionError::ArithmeticOverflow)?,
        )
    }

    /// Subtracts lamports from this account (transaction wide)
    pub fn checked_sub_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_sub(lamports)
                .ok_or(InstructionError::ArithmeticOverflow)?,
        )
    }

    /// Returns a read-only slice of the account data (transaction wide)
    pub fn get_data(&self) -> &[u8] {
        self.account.data()
    }

    /// Returns a writable slice of the account data (transaction wide)
    pub fn get_data_mut(&mut self) -> Result<&mut [u8], InstructionError> {
        Ok(self.account.data_as_mut_slice())
    }

    /// Overwrites the account data and size (transaction wide)
    pub fn set_data(&mut self, data: &[u8]) -> Result<(), InstructionError> {
        if data.len() == self.account.data().len() {
            self.account.data_as_mut_slice().copy_from_slice(data);
        } else {
            let mut total_resize_delta = self.transaction_context.total_resize_delta.borrow_mut();
            *total_resize_delta = total_resize_delta
                .saturating_add((data.len() as i64).saturating_sub(self.get_data().len() as i64));
            self.account.set_data_from_slice(data);
        }
        Ok(())
    }

    /// Resizes the account data (transaction wide)
    ///
    /// Fills it with zeros at the end if is extended or truncates at the end otherwise.
    pub fn set_data_length(&mut self, new_length: usize) -> Result<(), InstructionError> {
        let mut total_resize_delta = self.transaction_context.total_resize_delta.borrow_mut();
        *total_resize_delta = total_resize_delta
            .saturating_add((new_length as i64).saturating_sub(self.get_data().len() as i64));
        self.account.data_mut().resize(new_length, 0);
        Ok(())
    }

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
        Ok(())
    }

    /// Returns whether this account is executable (transaction wide)
    pub fn is_executable(&self) -> bool {
        self.account.executable()
    }

    /// Configures whether this account is executable (transaction wide)
    pub fn set_executable(&mut self, is_executable: bool) -> Result<(), InstructionError> {
        self.account.set_executable(is_executable);
        Ok(())
    }

    /// Returns the rent epoch of this account (transaction wide)
    pub fn get_rent_epoch(&self) -> u64 {
        self.account.rent_epoch()
    }

    /// Returns whether this account is a signer (instruction wide)
    pub fn is_signer(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            return false;
        }
        self.instruction_context
            .is_instruction_account_signer(
                self.index_in_instruction
                    .saturating_sub(self.instruction_context.program_accounts.len()),
            )
            .unwrap_or_default()
    }

    /// Returns whether this account is writable (instruction wide)
    pub fn is_writable(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.program_accounts.len() {
            return false;
        }
        self.instruction_context
            .is_instruction_account_writable(
                self.index_in_instruction
                    .saturating_sub(self.instruction_context.program_accounts.len()),
            )
            .unwrap_or_default()
    }
}

/// Everything that needs to be recorded from a TransactionContext after execution
pub struct ExecutionRecord {
    pub accounts: Vec<TransactionAccount>,
    pub instruction_trace: InstructionTrace,
    pub return_data: TransactionReturnData,
    pub total_resize_delta: i64,
}

/// Used by the bank in the runtime to write back the processed accounts and recorded instructions
impl From<TransactionContext> for ExecutionRecord {
    fn from(context: TransactionContext) -> Self {
        Self {
            accounts: Vec::from(Pin::into_inner(context.account_keys))
                .into_iter()
                .zip(
                    Vec::from(Pin::into_inner(context.accounts))
                        .into_iter()
                        .map(|account| account.into_inner()),
                )
                .collect(),
            instruction_trace: context.instruction_trace,
            return_data: context.return_data,
            total_resize_delta: RefCell::into_inner(context.total_resize_delta),
        }
    }
}
