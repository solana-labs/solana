//! Data shared between program runtime and built-in programs as well as SBF programs.
#![deny(clippy::indexing_slicing)]

#[cfg(not(target_os = "solana"))]
use crate::{
    account::WritableAccount,
    rent::Rent,
    system_instruction::{
        MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION, MAX_PERMITTED_DATA_LENGTH,
    },
};
use {
    crate::{
        account::{AccountSharedData, ReadableAccount},
        instruction::InstructionError,
        pubkey::Pubkey,
    },
    std::{
        cell::{RefCell, RefMut},
        collections::HashSet,
        pin::Pin,
    },
};

/// Index of an account inside of the TransactionContext or an InstructionContext.
pub type IndexOfAccount = u16;

/// Contains account meta data which varies between instruction.
///
/// It also contains indices to other structures for faster lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionAccount {
    /// Points to the account and its key in the `TransactionContext`
    pub index_in_transaction: IndexOfAccount,
    /// Points to the first occurrence in the parent `InstructionContext`
    ///
    /// This excludes the program accounts.
    pub index_in_caller: IndexOfAccount,
    /// Points to the first occurrence in the current `InstructionContext`
    ///
    /// This excludes the program accounts.
    pub index_in_callee: IndexOfAccount,
    /// Is this account supposed to sign
    pub is_signer: bool,
    /// Is this account allowed to become writable
    pub is_writable: bool,
}

/// An account key and the matching account
pub type TransactionAccount = (Pubkey, AccountSharedData);

/// Loaded transaction shared between runtime and programs.
///
/// This context is valid for the entire duration of a transaction being processed.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionContext {
    account_keys: Pin<Box<[Pubkey]>>,
    accounts: Pin<Box<[RefCell<AccountSharedData>]>>,
    #[cfg(not(target_os = "solana"))]
    account_touched_flags: RefCell<Pin<Box<[bool]>>>,
    instruction_stack_capacity: usize,
    instruction_trace_capacity: usize,
    instruction_stack: Vec<usize>,
    instruction_trace: Vec<InstructionContext>,
    return_data: TransactionReturnData,
    accounts_resize_delta: RefCell<i64>,
    #[cfg(not(target_os = "solana"))]
    rent: Option<Rent>,
    #[cfg(not(target_os = "solana"))]
    is_cap_accounts_data_allocations_per_transaction_enabled: bool,
}

impl TransactionContext {
    /// Constructs a new TransactionContext
    #[cfg(not(target_os = "solana"))]
    pub fn new(
        transaction_accounts: Vec<TransactionAccount>,
        rent: Option<Rent>,
        instruction_stack_capacity: usize,
        instruction_trace_capacity: usize,
    ) -> Self {
        let (account_keys, accounts): (Vec<Pubkey>, Vec<RefCell<AccountSharedData>>) =
            transaction_accounts
                .into_iter()
                .map(|(key, account)| (key, RefCell::new(account)))
                .unzip();
        let account_touched_flags = vec![false; accounts.len()];
        Self {
            account_keys: Pin::new(account_keys.into_boxed_slice()),
            accounts: Pin::new(accounts.into_boxed_slice()),
            account_touched_flags: RefCell::new(Pin::new(account_touched_flags.into_boxed_slice())),
            instruction_stack_capacity,
            instruction_trace_capacity,
            instruction_stack: Vec::with_capacity(instruction_stack_capacity),
            instruction_trace: vec![InstructionContext::default()],
            return_data: TransactionReturnData::default(),
            accounts_resize_delta: RefCell::new(0),
            rent,
            is_cap_accounts_data_allocations_per_transaction_enabled: false,
        }
    }

    /// Used in mock_process_instruction
    #[cfg(not(target_os = "solana"))]
    pub fn deconstruct_without_keys(self) -> Result<Vec<AccountSharedData>, InstructionError> {
        if !self.instruction_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        Ok(Vec::from(Pin::into_inner(self.accounts))
            .into_iter()
            .map(|account| account.into_inner())
            .collect())
    }

    /// Returns true if `enable_early_verification_of_account_modifications` is active
    #[cfg(not(target_os = "solana"))]
    pub fn is_early_verification_of_account_modifications_enabled(&self) -> bool {
        self.rent.is_some()
    }

    /// Returns the total number of accounts loaded in this Transaction
    pub fn get_number_of_accounts(&self) -> IndexOfAccount {
        self.accounts.len() as IndexOfAccount
    }

    /// Searches for an account by its key
    pub fn get_key_of_account_at_index(
        &self,
        index_in_transaction: IndexOfAccount,
    ) -> Result<&Pubkey, InstructionError> {
        self.account_keys
            .get(index_in_transaction as usize)
            .ok_or(InstructionError::NotEnoughAccountKeys)
    }

    /// Searches for an account by its key
    #[cfg(not(target_os = "solana"))]
    pub fn get_account_at_index(
        &self,
        index_in_transaction: IndexOfAccount,
    ) -> Result<&RefCell<AccountSharedData>, InstructionError> {
        self.accounts
            .get(index_in_transaction as usize)
            .ok_or(InstructionError::NotEnoughAccountKeys)
    }

    /// Searches for an account by its key
    pub fn find_index_of_account(&self, pubkey: &Pubkey) -> Option<IndexOfAccount> {
        self.account_keys
            .iter()
            .position(|key| key == pubkey)
            .map(|index| index as IndexOfAccount)
    }

    /// Searches for a program account by its key
    pub fn find_index_of_program_account(&self, pubkey: &Pubkey) -> Option<IndexOfAccount> {
        self.account_keys
            .iter()
            .rposition(|key| key == pubkey)
            .map(|index| index as IndexOfAccount)
    }

    /// Gets the max length of the InstructionContext trace
    pub fn get_instruction_trace_capacity(&self) -> usize {
        self.instruction_trace_capacity
    }

    /// Returns the instruction trace length.
    ///
    /// Not counting the last empty InstructionContext which is always pre-reserved for the next instruction.
    /// See also `get_next_instruction_context()`.
    pub fn get_instruction_trace_length(&self) -> usize {
        self.instruction_trace.len().saturating_sub(1)
    }

    /// Gets an InstructionContext by its index in the trace
    pub fn get_instruction_context_at_index_in_trace(
        &self,
        index_in_trace: usize,
    ) -> Result<&InstructionContext, InstructionError> {
        self.instruction_trace
            .get(index_in_trace)
            .ok_or(InstructionError::CallDepth)
    }

    /// Gets an InstructionContext by its nesting level in the stack
    pub fn get_instruction_context_at_nesting_level(
        &self,
        nesting_level: usize,
    ) -> Result<&InstructionContext, InstructionError> {
        let index_in_trace = *self
            .instruction_stack
            .get(nesting_level)
            .ok_or(InstructionError::CallDepth)?;
        let instruction_context = self.get_instruction_context_at_index_in_trace(index_in_trace)?;
        debug_assert_eq!(instruction_context.nesting_level, nesting_level);
        Ok(instruction_context)
    }

    /// Gets the max height of the InstructionContext stack
    pub fn get_instruction_stack_capacity(&self) -> usize {
        self.instruction_stack_capacity
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
        self.get_instruction_context_at_nesting_level(level)
    }

    /// Returns the InstructionContext to configure for the next invocation.
    ///
    /// The last InstructionContext is always empty and pre-reserved for the next instruction.
    pub fn get_next_instruction_context(
        &mut self,
    ) -> Result<&mut InstructionContext, InstructionError> {
        self.instruction_trace
            .last_mut()
            .ok_or(InstructionError::CallDepth)
    }

    /// Pushes the next InstructionContext
    #[cfg(not(target_os = "solana"))]
    pub fn push(&mut self) -> Result<(), InstructionError> {
        let nesting_level = self.get_instruction_context_stack_height();
        let caller_instruction_context = self
            .instruction_trace
            .last()
            .ok_or(InstructionError::CallDepth)?;
        let callee_instruction_accounts_lamport_sum =
            self.instruction_accounts_lamport_sum(caller_instruction_context)?;
        if !self.instruction_stack.is_empty()
            && self.is_early_verification_of_account_modifications_enabled()
        {
            let caller_instruction_context = self.get_current_instruction_context()?;
            let original_caller_instruction_accounts_lamport_sum =
                caller_instruction_context.instruction_accounts_lamport_sum;
            let current_caller_instruction_accounts_lamport_sum =
                self.instruction_accounts_lamport_sum(caller_instruction_context)?;
            if original_caller_instruction_accounts_lamport_sum
                != current_caller_instruction_accounts_lamport_sum
            {
                return Err(InstructionError::UnbalancedInstruction);
            }
        }
        {
            let mut instruction_context = self.get_next_instruction_context()?;
            instruction_context.nesting_level = nesting_level;
            instruction_context.instruction_accounts_lamport_sum =
                callee_instruction_accounts_lamport_sum;
        }
        let index_in_trace = self.get_instruction_trace_length();
        if index_in_trace >= self.instruction_trace_capacity {
            return Err(InstructionError::MaxInstructionTraceLengthExceeded);
        }
        self.instruction_trace.push(InstructionContext::default());
        if nesting_level >= self.instruction_stack_capacity {
            return Err(InstructionError::CallDepth);
        }
        self.instruction_stack.push(index_in_trace);
        Ok(())
    }

    /// Pops the current InstructionContext
    #[cfg(not(target_os = "solana"))]
    pub fn pop(&mut self) -> Result<(), InstructionError> {
        if self.instruction_stack.is_empty() {
            return Err(InstructionError::CallDepth);
        }
        // Verify (before we pop) that the total sum of all lamports in this instruction did not change
        let detected_an_unbalanced_instruction =
            if self.is_early_verification_of_account_modifications_enabled() {
                self.get_current_instruction_context()
                    .and_then(|instruction_context| {
                        // Verify all executable accounts have no outstanding refs
                        for account_index in instruction_context.program_accounts.iter() {
                            self.get_account_at_index(*account_index)?
                                .try_borrow_mut()
                                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
                        }
                        self.instruction_accounts_lamport_sum(instruction_context)
                            .map(|instruction_accounts_lamport_sum| {
                                instruction_context.instruction_accounts_lamport_sum
                                    != instruction_accounts_lamport_sum
                            })
                    })
            } else {
                Ok(false)
            };
        // Always pop, even if we `detected_an_unbalanced_instruction`
        self.instruction_stack.pop();
        if detected_an_unbalanced_instruction? {
            Err(InstructionError::UnbalancedInstruction)
        } else {
            Ok(())
        }
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

    /// Calculates the sum of all lamports within an instruction
    #[cfg(not(target_os = "solana"))]
    fn instruction_accounts_lamport_sum(
        &self,
        instruction_context: &InstructionContext,
    ) -> Result<u128, InstructionError> {
        if !self.is_early_verification_of_account_modifications_enabled() {
            return Ok(0);
        }
        let mut instruction_accounts_lamport_sum: u128 = 0;
        for instruction_account_index in 0..instruction_context.get_number_of_instruction_accounts()
        {
            if instruction_context
                .is_instruction_account_duplicate(instruction_account_index)?
                .is_some()
            {
                continue; // Skip duplicate account
            }
            let index_in_transaction = instruction_context
                .get_index_of_instruction_account_in_transaction(instruction_account_index)?;
            instruction_accounts_lamport_sum = (self
                .get_account_at_index(index_in_transaction)?
                .try_borrow()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?
                .lamports() as u128)
                .checked_add(instruction_accounts_lamport_sum)
                .ok_or(InstructionError::ArithmeticOverflow)?;
        }
        Ok(instruction_accounts_lamport_sum)
    }

    /// Returns the accounts resize delta
    pub fn accounts_resize_delta(&self) -> Result<i64, InstructionError> {
        self.accounts_resize_delta
            .try_borrow()
            .map_err(|_| InstructionError::GenericError)
            .map(|value_ref| *value_ref)
    }

    /// Enables enforcing a maximum accounts data allocation size per transaction
    #[cfg(not(target_os = "solana"))]
    pub fn enable_cap_accounts_data_allocations_per_transaction(&mut self) {
        self.is_cap_accounts_data_allocations_per_transaction_enabled = true;
    }
}

/// Return data at the end of a transaction
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TransactionReturnData {
    pub program_id: Pubkey,
    pub data: Vec<u8>,
}

/// Loaded instruction shared between runtime and programs.
///
/// This context is valid for the entire duration of a (possibly cross program) instruction being processed.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct InstructionContext {
    nesting_level: usize,
    instruction_accounts_lamport_sum: u128,
    program_accounts: Vec<IndexOfAccount>,
    instruction_accounts: Vec<InstructionAccount>,
    instruction_data: Vec<u8>,
}

impl InstructionContext {
    /// Used together with TransactionContext::get_next_instruction_context()
    #[cfg(not(target_os = "solana"))]
    pub fn configure(
        &mut self,
        program_accounts: &[IndexOfAccount],
        instruction_accounts: &[InstructionAccount],
        instruction_data: &[u8],
    ) {
        self.program_accounts = program_accounts.to_vec();
        self.instruction_accounts = instruction_accounts.to_vec();
        self.instruction_data = instruction_data.to_vec();
    }

    /// How many Instructions were on the stack after this one was pushed
    ///
    /// That is the number of nested parent Instructions plus one (itself).
    pub fn get_stack_height(&self) -> usize {
        self.nesting_level.saturating_add(1)
    }

    /// Returns the sum of lamports of the instruction accounts in this Instruction
    pub fn get_instruction_accounts_lamport_sum(&self) -> u128 {
        self.instruction_accounts_lamport_sum
    }

    /// Number of program accounts
    pub fn get_number_of_program_accounts(&self) -> IndexOfAccount {
        self.program_accounts.len() as IndexOfAccount
    }

    /// Number of accounts in this Instruction (without program accounts)
    pub fn get_number_of_instruction_accounts(&self) -> IndexOfAccount {
        self.instruction_accounts.len() as IndexOfAccount
    }

    /// Assert that enough account were supplied to this Instruction
    pub fn check_number_of_instruction_accounts(
        &self,
        expected_at_least: IndexOfAccount,
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
    ) -> Option<IndexOfAccount> {
        self.program_accounts
            .iter()
            .position(|index_in_transaction| {
                transaction_context
                    .account_keys
                    .get(*index_in_transaction as usize)
                    == Some(pubkey)
            })
            .map(|index| index as IndexOfAccount)
    }

    /// Searches for an instruction account by its key
    pub fn find_index_of_instruction_account(
        &self,
        transaction_context: &TransactionContext,
        pubkey: &Pubkey,
    ) -> Option<IndexOfAccount> {
        self.instruction_accounts
            .iter()
            .position(|instruction_account| {
                transaction_context
                    .account_keys
                    .get(instruction_account.index_in_transaction as usize)
                    == Some(pubkey)
            })
            .map(|index| index as IndexOfAccount)
    }

    /// Translates the given instruction wide program_account_index into a transaction wide index
    pub fn get_index_of_program_account_in_transaction(
        &self,
        program_account_index: IndexOfAccount,
    ) -> Result<IndexOfAccount, InstructionError> {
        Ok(*self
            .program_accounts
            .get(program_account_index as usize)
            .ok_or(InstructionError::NotEnoughAccountKeys)?)
    }

    /// Translates the given instruction wide instruction_account_index into a transaction wide index
    pub fn get_index_of_instruction_account_in_transaction(
        &self,
        instruction_account_index: IndexOfAccount,
    ) -> Result<IndexOfAccount, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index as usize)
            .ok_or(InstructionError::NotEnoughAccountKeys)?
            .index_in_transaction as IndexOfAccount)
    }

    /// Returns `Some(instruction_account_index)` if this is a duplicate
    /// and `None` if it is the first account with this key
    pub fn is_instruction_account_duplicate(
        &self,
        instruction_account_index: IndexOfAccount,
    ) -> Result<Option<IndexOfAccount>, InstructionError> {
        let index_in_callee = self
            .instruction_accounts
            .get(instruction_account_index as usize)
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
        self.get_index_of_program_account_in_transaction(
            self.get_number_of_program_accounts().saturating_sub(1),
        )
        .and_then(|index_in_transaction| {
            transaction_context.get_key_of_account_at_index(index_in_transaction)
        })
    }

    fn try_borrow_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        index_in_transaction: IndexOfAccount,
        index_in_instruction: IndexOfAccount,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let account = transaction_context
            .accounts
            .get(index_in_transaction as usize)
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
            self.get_number_of_program_accounts().saturating_sub(1),
        );
        debug_assert!(result.is_ok());
        result
    }

    /// Tries to borrow a program account from this Instruction
    pub fn try_borrow_program_account<'a, 'b: 'a>(
        &'a self,
        transaction_context: &'b TransactionContext,
        program_account_index: IndexOfAccount,
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
        instruction_account_index: IndexOfAccount,
    ) -> Result<BorrowedAccount<'a>, InstructionError> {
        let index_in_transaction =
            self.get_index_of_instruction_account_in_transaction(instruction_account_index)?;
        self.try_borrow_account(
            transaction_context,
            index_in_transaction,
            self.get_number_of_program_accounts()
                .saturating_add(instruction_account_index),
        )
    }

    /// Returns whether an instruction account is a signer
    pub fn is_instruction_account_signer(
        &self,
        instruction_account_index: IndexOfAccount,
    ) -> Result<bool, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index as usize)
            .ok_or(InstructionError::MissingAccount)?
            .is_signer)
    }

    /// Returns whether an instruction account is writable
    pub fn is_instruction_account_writable(
        &self,
        instruction_account_index: IndexOfAccount,
    ) -> Result<bool, InstructionError> {
        Ok(self
            .instruction_accounts
            .get(instruction_account_index as usize)
            .ok_or(InstructionError::MissingAccount)?
            .is_writable)
    }

    /// Calculates the set of all keys of signer instruction accounts in this Instruction
    pub fn get_signers(
        &self,
        transaction_context: &TransactionContext,
    ) -> Result<HashSet<Pubkey>, InstructionError> {
        let mut result = HashSet::new();
        for instruction_account in self.instruction_accounts.iter() {
            if instruction_account.is_signer {
                result.insert(
                    *transaction_context
                        .get_key_of_account_at_index(instruction_account.index_in_transaction)?,
                );
            }
        }
        Ok(result)
    }
}

/// Shared account borrowed from the TransactionContext and an InstructionContext.
#[derive(Debug)]
pub struct BorrowedAccount<'a> {
    transaction_context: &'a TransactionContext,
    instruction_context: &'a InstructionContext,
    index_in_transaction: IndexOfAccount,
    index_in_instruction: IndexOfAccount,
    account: RefMut<'a, AccountSharedData>,
}

impl<'a> BorrowedAccount<'a> {
    /// Returns the index of this account (transaction wide)
    #[inline]
    pub fn get_index_in_transaction(&self) -> IndexOfAccount {
        self.index_in_transaction
    }

    /// Returns the public key of this account (transaction wide)
    #[inline]
    pub fn get_key(&self) -> &Pubkey {
        self.transaction_context
            .get_key_of_account_at_index(self.index_in_transaction)
            .unwrap()
    }

    /// Returns the owner of this account (transaction wide)
    #[inline]
    pub fn get_owner(&self) -> &Pubkey {
        self.account.owner()
    }

    /// Assignes the owner of this account (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn set_owner(&mut self, pubkey: &[u8]) -> Result<(), InstructionError> {
        if self
            .transaction_context
            .is_early_verification_of_account_modifications_enabled()
        {
            // Only the owner can assign a new owner
            if !self.is_owned_by_current_program() {
                return Err(InstructionError::ModifiedProgramId);
            }
            // and only if the account is writable
            if !self.is_writable() {
                return Err(InstructionError::ModifiedProgramId);
            }
            // and only if the account is not executable
            if self.is_executable() {
                return Err(InstructionError::ModifiedProgramId);
            }
            // and only if the data is zero-initialized or empty
            if !is_zeroed(self.get_data()) {
                return Err(InstructionError::ModifiedProgramId);
            }
            // don't touch the account if the owner does not change
            if self.get_owner().to_bytes() == pubkey {
                return Ok(());
            }
            self.touch()?;
        }
        self.account.copy_into_owner_from_slice(pubkey);
        Ok(())
    }

    /// Returns the number of lamports of this account (transaction wide)
    #[inline]
    pub fn get_lamports(&self) -> u64 {
        self.account.lamports()
    }

    /// Overwrites the number of lamports of this account (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        if self
            .transaction_context
            .is_early_verification_of_account_modifications_enabled()
        {
            // An account not owned by the program cannot have its balance decrease
            if !self.is_owned_by_current_program() && lamports < self.get_lamports() {
                return Err(InstructionError::ExternalAccountLamportSpend);
            }
            // The balance of read-only may not change
            if !self.is_writable() {
                return Err(InstructionError::ReadonlyLamportChange);
            }
            // The balance of executable accounts may not change
            if self.is_executable() {
                return Err(InstructionError::ExecutableLamportChange);
            }
            // don't touch the account if the lamports do not change
            if self.get_lamports() == lamports {
                return Ok(());
            }
            self.touch()?;
        }
        self.account.set_lamports(lamports);
        Ok(())
    }

    /// Adds lamports to this account (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn checked_add_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_add(lamports)
                .ok_or(InstructionError::ArithmeticOverflow)?,
        )
    }

    /// Subtracts lamports from this account (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn checked_sub_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.set_lamports(
            self.get_lamports()
                .checked_sub(lamports)
                .ok_or(InstructionError::ArithmeticOverflow)?,
        )
    }

    /// Returns a read-only slice of the account data (transaction wide)
    #[inline]
    pub fn get_data(&self) -> &[u8] {
        self.account.data()
    }

    /// Returns a writable slice of the account data (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn get_data_mut(&mut self) -> Result<&mut [u8], InstructionError> {
        self.can_data_be_changed()?;
        self.touch()?;
        Ok(self.account.data_as_mut_slice())
    }

    /// Overwrites the account data and size (transaction wide).
    ///
    /// Call this when you have an owned buffer and want to replace the account
    /// data with it.
    ///
    /// If you have a slice, use set_data_from_slice().
    #[cfg(not(target_os = "solana"))]
    pub fn set_data(&mut self, data: Vec<u8>) -> Result<(), InstructionError> {
        self.can_data_be_resized(data.len())?;
        self.can_data_be_changed()?;
        self.touch()?;
        self.update_accounts_resize_delta(data.len())?;
        self.account.set_data(data);

        Ok(())
    }

    /// Overwrites the account data and size (transaction wide).
    ///
    /// Call this when you have a slice of data you do not own and want to
    /// replace the account data with it.
    ///
    /// If you have an owned buffer (eg Vec<u8>), use set_data().
    #[cfg(not(target_os = "solana"))]
    pub fn set_data_from_slice(&mut self, data: &[u8]) -> Result<(), InstructionError> {
        self.can_data_be_resized(data.len())?;
        self.can_data_be_changed()?;
        self.touch()?;
        self.update_accounts_resize_delta(data.len())?;
        self.account.set_data_from_slice(data);

        Ok(())
    }

    /// Resizes the account data (transaction wide)
    ///
    /// Fills it with zeros at the end if is extended or truncates at the end otherwise.
    #[cfg(not(target_os = "solana"))]
    pub fn set_data_length(&mut self, new_length: usize) -> Result<(), InstructionError> {
        self.can_data_be_resized(new_length)?;
        self.can_data_be_changed()?;
        // don't touch the account if the length does not change
        if self.get_data().len() == new_length {
            return Ok(());
        }
        self.touch()?;
        self.update_accounts_resize_delta(new_length)?;
        self.account.data_mut().resize(new_length, 0);
        Ok(())
    }

    /// Appends all elements in a slice to the account
    #[cfg(not(target_os = "solana"))]
    pub fn extend_from_slice(&mut self, data: &[u8]) -> Result<(), InstructionError> {
        let new_len = self.get_data().len().saturating_add(data.len());
        self.can_data_be_resized(new_len)?;
        self.can_data_be_changed()?;

        if data.is_empty() {
            return Ok(());
        }

        self.touch()?;
        self.update_accounts_resize_delta(new_len)?;
        self.account.data_mut().extend_from_slice(data);
        Ok(())
    }

    /// Deserializes the account data into a state
    #[cfg(not(target_os = "solana"))]
    pub fn get_state<T: serde::de::DeserializeOwned>(&self) -> Result<T, InstructionError> {
        self.account
            .deserialize_data()
            .map_err(|_| InstructionError::InvalidAccountData)
    }

    /// Serializes a state into the account data
    #[cfg(not(target_os = "solana"))]
    pub fn set_state<T: serde::Serialize>(&mut self, state: &T) -> Result<(), InstructionError> {
        let data = self.get_data_mut()?;
        let serialized_size =
            bincode::serialized_size(state).map_err(|_| InstructionError::GenericError)?;
        if serialized_size > data.len() as u64 {
            return Err(InstructionError::AccountDataTooSmall);
        }
        bincode::serialize_into(&mut *data, state).map_err(|_| InstructionError::GenericError)?;
        Ok(())
    }

    /// Returns whether this account is executable (transaction wide)
    #[inline]
    pub fn is_executable(&self) -> bool {
        self.account.executable()
    }

    /// Configures whether this account is executable (transaction wide)
    #[cfg(not(target_os = "solana"))]
    pub fn set_executable(&mut self, is_executable: bool) -> Result<(), InstructionError> {
        if let Some(rent) = self.transaction_context.rent {
            // To become executable an account must be rent exempt
            if !rent.is_exempt(self.get_lamports(), self.get_data().len()) {
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }
            // Only the owner can set the executable flag
            if !self.is_owned_by_current_program() {
                return Err(InstructionError::ExecutableModified);
            }
            // and only if the account is writable
            if !self.is_writable() {
                return Err(InstructionError::ExecutableModified);
            }
            // one can not clear the executable flag
            if self.is_executable() && !is_executable {
                return Err(InstructionError::ExecutableModified);
            }
            // don't touch the account if the executable flag does not change
            if self.is_executable() == is_executable {
                return Ok(());
            }
            self.touch()?;
        }
        self.account.set_executable(is_executable);
        Ok(())
    }

    /// Returns the rent epoch of this account (transaction wide)
    #[cfg(not(target_os = "solana"))]
    #[inline]
    pub fn get_rent_epoch(&self) -> u64 {
        self.account.rent_epoch()
    }

    /// Returns whether this account is a signer (instruction wide)
    pub fn is_signer(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.get_number_of_program_accounts() {
            return false;
        }
        self.instruction_context
            .is_instruction_account_signer(
                self.index_in_instruction
                    .saturating_sub(self.instruction_context.get_number_of_program_accounts()),
            )
            .unwrap_or_default()
    }

    /// Returns whether this account is writable (instruction wide)
    pub fn is_writable(&self) -> bool {
        if self.index_in_instruction < self.instruction_context.get_number_of_program_accounts() {
            return false;
        }
        self.instruction_context
            .is_instruction_account_writable(
                self.index_in_instruction
                    .saturating_sub(self.instruction_context.get_number_of_program_accounts()),
            )
            .unwrap_or_default()
    }

    /// Returns true if the owner of this account is the current `InstructionContext`s last program (instruction wide)
    pub fn is_owned_by_current_program(&self) -> bool {
        self.instruction_context
            .get_last_program_key(self.transaction_context)
            .map(|key| key == self.get_owner())
            .unwrap_or_default()
    }

    /// Returns an error if the account data can not be mutated by the current program
    #[cfg(not(target_os = "solana"))]
    pub fn can_data_be_changed(&self) -> Result<(), InstructionError> {
        if !self
            .transaction_context
            .is_early_verification_of_account_modifications_enabled()
        {
            return Ok(());
        }
        // Only non-executable accounts data can be changed
        if self.is_executable() {
            return Err(InstructionError::ExecutableDataModified);
        }
        // and only if the account is writable
        if !self.is_writable() {
            return Err(InstructionError::ReadonlyDataModified);
        }
        // and only if we are the owner
        if !self.is_owned_by_current_program() {
            return Err(InstructionError::ExternalAccountDataModified);
        }
        Ok(())
    }

    /// Returns an error if the account data can not be resized to the given length
    #[cfg(not(target_os = "solana"))]
    pub fn can_data_be_resized(&self, new_length: usize) -> Result<(), InstructionError> {
        if !self
            .transaction_context
            .is_early_verification_of_account_modifications_enabled()
        {
            return Ok(());
        }
        let old_length = self.get_data().len();
        // Only the owner can change the length of the data
        if new_length != old_length && !self.is_owned_by_current_program() {
            return Err(InstructionError::AccountDataSizeChanged);
        }
        // The new length can not exceed the maximum permitted length
        if new_length > MAX_PERMITTED_DATA_LENGTH as usize {
            return Err(InstructionError::InvalidRealloc);
        }
        if self
            .transaction_context
            .is_cap_accounts_data_allocations_per_transaction_enabled
        {
            // The resize can not exceed the per-transaction maximum
            let length_delta = (new_length as i64).saturating_sub(old_length as i64);
            if self
                .transaction_context
                .accounts_resize_delta()?
                .saturating_add(length_delta)
                > MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION
            {
                return Err(InstructionError::MaxAccountsDataAllocationsExceeded);
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "solana"))]
    fn touch(&self) -> Result<(), InstructionError> {
        if self
            .transaction_context
            .is_early_verification_of_account_modifications_enabled()
        {
            *self
                .transaction_context
                .account_touched_flags
                .try_borrow_mut()
                .map_err(|_| InstructionError::GenericError)?
                .get_mut(self.index_in_transaction as usize)
                .ok_or(InstructionError::NotEnoughAccountKeys)? = true;
        }
        Ok(())
    }

    #[cfg(not(target_os = "solana"))]
    fn update_accounts_resize_delta(&mut self, new_len: usize) -> Result<(), InstructionError> {
        let mut accounts_resize_delta = self
            .transaction_context
            .accounts_resize_delta
            .try_borrow_mut()
            .map_err(|_| InstructionError::GenericError)?;
        *accounts_resize_delta = accounts_resize_delta
            .saturating_add((new_len as i64).saturating_sub(self.get_data().len() as i64));
        Ok(())
    }
}

/// Everything that needs to be recorded from a TransactionContext after execution
#[cfg(not(target_os = "solana"))]
pub struct ExecutionRecord {
    pub accounts: Vec<TransactionAccount>,
    pub return_data: TransactionReturnData,
    pub touched_account_count: u64,
    pub accounts_resize_delta: i64,
}

/// Used by the bank in the runtime to write back the processed accounts and recorded instructions
#[cfg(not(target_os = "solana"))]
impl From<TransactionContext> for ExecutionRecord {
    fn from(context: TransactionContext) -> Self {
        let account_touched_flags = context
            .account_touched_flags
            .try_borrow()
            .expect("borrowing transaction_context.account_touched_flags failed");
        let touched_account_count = account_touched_flags
            .iter()
            .fold(0u64, |accumulator, was_touched| {
                accumulator.saturating_add(*was_touched as u64)
            });
        Self {
            accounts: Vec::from(Pin::into_inner(context.account_keys))
                .into_iter()
                .zip(
                    Vec::from(Pin::into_inner(context.accounts))
                        .into_iter()
                        .map(|account| account.into_inner()),
                )
                .collect(),
            return_data: context.return_data,
            touched_account_count,
            accounts_resize_delta: RefCell::into_inner(context.accounts_resize_delta),
        }
    }
}

#[cfg(not(target_os = "solana"))]
fn is_zeroed(buf: &[u8]) -> bool {
    const ZEROS_LEN: usize = 1024;
    const ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
    let mut chunks = buf.chunks_exact(ZEROS_LEN);

    #[allow(clippy::indexing_slicing)]
    {
        chunks.all(|chunk| chunk == &ZEROS[..])
            && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
    }
}
