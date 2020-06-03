use crate::{native_loader::NativeLoader, rent_collector::RentCollector};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    account::{create_keyed_readonly_accounts, Account, KeyedAccount},
    clock::Epoch,
    entrypoint_native::{InvokeContext, ProcessInstruction},
    instruction::{CompiledInstruction, InstructionError},
    message::Message,
    native_loader,
    pubkey::Pubkey,
    rent::Rent,
    system_program,
    transaction::TransactionError,
};
use std::{cell::RefCell, rc::Rc};

// The relevant state of an account before an Instruction executes, used
// to verify account integrity after the Instruction completes
#[derive(Clone, Debug, Default)]
pub struct PreAccount {
    key: Pubkey,
    is_signer: bool,
    is_writable: bool,
    is_executable: bool,
    lamports: u64,
    data: Vec<u8>,
    owner: Pubkey,
    rent_epoch: Epoch,
}
impl PreAccount {
    pub fn new(key: &Pubkey, account: &Account, is_signer: bool, is_writable: bool) -> Self {
        Self {
            key: *key,
            is_signer,
            is_writable,
            lamports: account.lamports,
            data: account.data.clone(),
            owner: account.owner,
            is_executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }

    pub fn verify(
        &self,
        program_id: &Pubkey,
        rent: &Rent,
        post: &Account,
    ) -> Result<(), InstructionError> {
        // Only the owner of the account may change owner and
        //   only if the account is writable and
        //   only if the data is zero-initialized or empty
        if self.owner != post.owner
            && (!self.is_writable // line coverage used to get branch coverage
                || *program_id != self.owner
            || !Self::is_zeroed(&post.data))
        {
            return Err(InstructionError::ModifiedProgramId);
        }

        // An account not assigned to the program cannot have its balance decrease.
        if *program_id != self.owner // line coverage used to get branch coverage
        && self.lamports > post.lamports
        {
            return Err(InstructionError::ExternalAccountLamportSpend);
        }

        // The balance of read-only and executable accounts may not change
        if self.lamports != post.lamports {
            if !self.is_writable {
                return Err(InstructionError::ReadonlyLamportChange);
            }
            if self.is_executable {
                return Err(InstructionError::ExecutableLamportChange);
            }
        }

        // Only the system program can change the size of the data
        //  and only if the system program owns the account
        if self.data.len() != post.data.len()
            && (!system_program::check_id(program_id) // line coverage used to get branch coverage
                || !system_program::check_id(&self.owner))
        {
            return Err(InstructionError::AccountDataSizeChanged);
        }

        // Only the owner may change account data
        //   and if the account is writable
        //   and if the account is not executable
        if !(*program_id == self.owner
            && self.is_writable  // line coverage used to get branch coverage
            && !self.is_executable)
            && self.data != post.data
        {
            if self.is_executable {
                return Err(InstructionError::ExecutableDataModified);
            } else if self.is_writable {
                return Err(InstructionError::ExternalAccountDataModified);
            } else {
                return Err(InstructionError::ReadonlyDataModified);
            }
        }

        // executable is one-way (false->true) and only the account owner may set it.
        if self.is_executable != post.executable {
            if !rent.is_exempt(post.lamports, post.data.len()) {
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }
            if !self.is_writable // line coverage used to get branch coverage
                || self.is_executable
                || *program_id != self.owner
            {
                return Err(InstructionError::ExecutableModified);
            }
        }

        // No one modifies rent_epoch (yet).
        if self.rent_epoch != post.rent_epoch {
            return Err(InstructionError::RentEpochModified);
        }

        Ok(())
    }

    pub fn update(&mut self, account: &Account) {
        self.lamports = account.lamports;
        if self.data.len() != account.data.len() {
            // Only system account can change data size, copy with alloc
            self.data = account.data.clone();
        } else {
            // Copy without allocate
            self.data.clone_from_slice(&account.data);
        }
    }

    pub fn key(&self) -> Pubkey {
        self.key
    }

    pub fn lamports(&self) -> u64 {
        self.lamports
    }

    pub fn is_zeroed(buf: &[u8]) -> bool {
        const ZEROS_LEN: usize = 1024;
        static ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
        let mut chunks = buf.chunks_exact(ZEROS_LEN);

        chunks.all(|chunk| chunk == &ZEROS[..])
            && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
    }
}

#[derive(Default)]
pub struct ThisInvokeContext {
    pub program_ids: Vec<Pubkey>,
    pub rent: Rent,
    pub pre_accounts: Vec<PreAccount>,
    pub programs: Vec<(Pubkey, ProcessInstruction)>,
}
impl ThisInvokeContext {
    const MAX_INVOCATION_DEPTH: usize = 5;
    pub fn new(
        program_id: &Pubkey,
        rent: Rent,
        pre_accounts: Vec<PreAccount>,
        programs: Vec<(Pubkey, ProcessInstruction)>,
    ) -> Self {
        let mut program_ids = Vec::with_capacity(Self::MAX_INVOCATION_DEPTH);
        program_ids.push(*program_id);
        Self {
            program_ids,
            rent,
            pre_accounts,
            programs,
        }
    }
}
impl InvokeContext for ThisInvokeContext {
    fn push(&mut self, key: &Pubkey) -> Result<(), InstructionError> {
        if self.program_ids.len() >= Self::MAX_INVOCATION_DEPTH {
            return Err(InstructionError::CallDepth);
        }
        if self.program_ids.contains(key) && self.program_ids.last() != Some(key) {
            // Reentrancy not allowed unless caller is calling itself
            return Err(InstructionError::ReentrancyNotAllowed);
        }
        self.program_ids.push(*key);
        Ok(())
    }
    fn pop(&mut self) {
        self.program_ids.pop();
    }
    fn verify_and_update(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        match self.program_ids.last() {
            Some(key) => MessageProcessor::verify_and_update(
                message,
                instruction,
                &mut self.pre_accounts,
                accounts,
                key,
                &self.rent,
            ),
            None => Err(InstructionError::GenericError), // Should never happen
        }
    }
    fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        self.program_ids
            .last()
            .ok_or(InstructionError::GenericError)
    }

    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)] {
        &self.programs
    }
}

pub type ProcessInstructionWithContext =
    fn(&Pubkey, &[KeyedAccount], &[u8], &mut dyn InvokeContext) -> Result<(), InstructionError>;

#[derive(Default, Deserialize, Serialize)]
pub struct MessageProcessor {
    #[serde(skip)]
    programs: Vec<(Pubkey, ProcessInstruction)>,
    #[serde(skip)]
    loaders: Vec<(Pubkey, ProcessInstructionWithContext)>,
    #[serde(skip)]
    native_loader: NativeLoader,
}
impl Clone for MessageProcessor {
    fn clone(&self) -> Self {
        MessageProcessor {
            programs: self.programs.clone(),
            loaders: self.loaders.clone(),
            native_loader: NativeLoader::default(),
        }
    }
}
impl MessageProcessor {
    /// Add a static entrypoint to intercept instructions before the dynamic loader.
    pub fn add_program(&mut self, program_id: Pubkey, process_instruction: ProcessInstruction) {
        match self.programs.iter_mut().find(|(key, _)| program_id == *key) {
            Some((_, processor)) => *processor = process_instruction,
            None => self.programs.push((program_id, process_instruction)),
        }
    }

    pub fn add_loader(
        &mut self,
        program_id: Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) {
        match self.loaders.iter_mut().find(|(key, _)| program_id == *key) {
            Some((_, processor)) => *processor = process_instruction,
            None => self.loaders.push((program_id, process_instruction)),
        }
    }

    /// Create the KeyedAccounts that will be passed to the program
    fn create_keyed_accounts<'a>(
        message: &'a Message,
        instruction: &'a CompiledInstruction,
        executable_accounts: &'a [(Pubkey, RefCell<Account>)],
        accounts: &'a [Rc<RefCell<Account>>],
    ) -> Result<Vec<KeyedAccount<'a>>, InstructionError> {
        let mut keyed_accounts = create_keyed_readonly_accounts(&executable_accounts);
        let mut keyed_accounts2: Vec<_> = instruction
            .accounts
            .iter()
            .map(|&index| {
                let is_signer = message.is_signer(index as usize);
                let index = index as usize;
                let key = &message.account_keys[index];
                let account = &accounts[index];
                if message.is_writable(index) {
                    KeyedAccount::new(key, is_signer, account)
                } else {
                    KeyedAccount::new_readonly(key, is_signer, account)
                }
            })
            .collect();
        keyed_accounts.append(&mut keyed_accounts2);
        assert!(keyed_accounts[0].executable()?, "account not executable");
        Ok(keyed_accounts)
    }

    /// Process an instruction
    /// This method calls the instruction's program entrypoint method
    fn process_instruction(
        &self,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        if native_loader::check_id(&keyed_accounts[0].owner()?) {
            let root_id = keyed_accounts[0].unsigned_key();
            for (id, process_instruction) in &self.programs {
                if id == root_id {
                    // Call the builtin program
                    return process_instruction(&root_id, &keyed_accounts[1..], instruction_data);
                }
            }
            // Call the program via the native loader
            return self.native_loader.process_instruction(
                &native_loader::id(),
                keyed_accounts,
                instruction_data,
                invoke_context,
            );
        } else {
            let owner_id = keyed_accounts[0].owner()?;
            for (id, process_instruction) in &self.loaders {
                if *id == owner_id {
                    // Call the program via a builtin loader
                    return process_instruction(
                        &owner_id,
                        keyed_accounts,
                        instruction_data,
                        invoke_context,
                    );
                }
            }
        }
        Err(InstructionError::UnsupportedProgramId)
    }

    /// Process a cross-program instruction
    /// This method calls the instruction's program entrypoint function
    pub fn process_cross_program_instruction(
        &self,
        message: &Message,
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        accounts: &[Rc<RefCell<Account>>],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        let instruction = &message.instructions[0];

        // Verify the calling program hasn't misbehaved
        invoke_context.verify_and_update(message, instruction, accounts)?;

        // Construct keyed accounts
        let keyed_accounts =
            Self::create_keyed_accounts(message, instruction, executable_accounts, accounts)?;

        // Invoke callee
        invoke_context.push(instruction.program_id(&message.account_keys))?;
        let mut result =
            self.process_instruction(&keyed_accounts, &instruction.data, invoke_context);
        if result.is_ok() {
            // Verify the called program has not misbehaved
            result = invoke_context.verify_and_update(message, instruction, accounts);
        }
        invoke_context.pop();

        result
    }

    /// Record the initial state of the accounts so that they can be compared
    /// after the instruction is processed
    pub fn create_pre_accounts(
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[Rc<RefCell<Account>>],
    ) -> Vec<PreAccount> {
        let mut pre_accounts = Vec::with_capacity(accounts.len());
        {
            let mut work = |_unique_index: usize, account_index: usize| {
                let key = &message.account_keys[account_index];
                let is_signer = account_index < message.header.num_required_signatures as usize;
                let is_writable = message.is_writable(account_index);
                let account = accounts[account_index].borrow();
                pre_accounts.push(PreAccount::new(key, &account, is_signer, is_writable));
                Ok(())
            };
            let _ = instruction.visit_each_account(&mut work);
        }
        pre_accounts
    }

    /// Verify there are no outstanding borrows
    pub fn verify_account_references(
        accounts: &[(Pubkey, RefCell<Account>)],
    ) -> Result<(), InstructionError> {
        for (_, account) in accounts.iter() {
            account
                .try_borrow_mut()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
        }
        Ok(())
    }

    /// Verify the results of an instruction
    pub fn verify(
        message: &Message,
        instruction: &CompiledInstruction,
        pre_accounts: &[PreAccount],
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        accounts: &[Rc<RefCell<Account>>],
        rent: &Rent,
    ) -> Result<(), InstructionError> {
        // Verify all executable accounts have zero outstanding refs
        Self::verify_account_references(executable_accounts)?;

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        {
            let program_id = instruction.program_id(&message.account_keys);
            let mut work = |unique_index: usize, account_index: usize| {
                // Verify account has no outstanding references and take one
                let account = accounts[account_index]
                    .try_borrow_mut()
                    .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
                pre_accounts[unique_index].verify(&program_id, rent, &account)?;
                pre_sum += u128::from(pre_accounts[unique_index].lamports());
                post_sum += u128::from(account.lamports);
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

    /// Verify the results of a cross-program instruction
    fn verify_and_update(
        message: &Message,
        instruction: &CompiledInstruction,
        pre_accounts: &mut [PreAccount],
        accounts: &[Rc<RefCell<Account>>],
        program_id: &Pubkey,
        rent: &Rent,
    ) -> Result<(), InstructionError> {
        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        let mut work = |_unique_index: usize, account_index: usize| {
            let key = &message.account_keys[account_index];
            let account = &accounts[account_index];
            // Find the matching PreAccount
            for pre_account in pre_accounts.iter_mut() {
                if *key == pre_account.key() {
                    // Verify account has no outstanding references and take one
                    let account = account
                        .try_borrow_mut()
                        .map_err(|_| InstructionError::AccountBorrowOutstanding)?;

                    pre_account.verify(&program_id, &rent, &account)?;
                    pre_sum += u128::from(pre_account.lamports());
                    post_sum += u128::from(account.lamports);

                    pre_account.update(&account);

                    return Ok(());
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

    /// Execute an instruction
    /// This method calls the instruction's program entrypoint method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if this function returns Ok(_).
    fn execute_instruction(
        &self,
        message: &Message,
        instruction: &CompiledInstruction,
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        accounts: &[Rc<RefCell<Account>>],
        rent_collector: &RentCollector,
    ) -> Result<(), InstructionError> {
        let pre_accounts = Self::create_pre_accounts(message, instruction, accounts);
        let mut invoke_context = ThisInvokeContext::new(
            instruction.program_id(&message.account_keys),
            rent_collector.rent,
            pre_accounts,
            self.programs.clone(),
        );
        let keyed_accounts =
            Self::create_keyed_accounts(message, instruction, executable_accounts, accounts)?;
        self.process_instruction(&keyed_accounts, &instruction.data, &mut invoke_context)?;
        Self::verify(
            message,
            instruction,
            &invoke_context.pre_accounts,
            executable_accounts,
            accounts,
            &rent_collector.rent,
        )?;
        Ok(())
    }

    /// Process a message.
    /// This method calls each instruction in the message over the set of loaded Accounts
    /// The accounts are committed back to the bank only if every instruction succeeds
    pub fn process_message(
        &self,
        message: &Message,
        loaders: &[Vec<(Pubkey, RefCell<Account>)>],
        accounts: &[Rc<RefCell<Account>>],
        rent_collector: &RentCollector,
    ) -> Result<(), TransactionError> {
        for (instruction_index, instruction) in message.instructions.iter().enumerate() {
            let executable_index = message
                .program_position(instruction.program_id_index as usize)
                .ok_or(TransactionError::InvalidAccountIndex)?;
            let executable_accounts = &loaders[executable_index];

            self.execute_instruction(
                message,
                instruction,
                executable_accounts,
                accounts,
                rent_collector,
            )
            .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
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
        native_loader::create_loadable_account,
    };

    #[test]
    fn test_invoke_context() {
        const MAX_DEPTH: usize = 10;
        let mut program_ids = vec![];
        let mut keys = vec![];
        let mut pre_accounts = vec![];
        let mut accounts = vec![];
        for i in 0..MAX_DEPTH {
            program_ids.push(Pubkey::new_rand());
            keys.push(Pubkey::new_rand());
            accounts.push(Rc::new(RefCell::new(Account::new(
                i as u64,
                1,
                &program_ids[i],
            ))));
            pre_accounts.push(PreAccount::new(
                &keys[i],
                &accounts[i].borrow(),
                false,
                true,
            ))
        }
        let mut invoke_context =
            ThisInvokeContext::new(&program_ids[0], Rent::default(), pre_accounts, vec![]);

        // Check call depth increases and has a limit
        let mut depth_reached = 1;
        for program_id in program_ids.iter().skip(1) {
            if Err(InstructionError::CallDepth) == invoke_context.push(program_id) {
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
                AccountMeta::new(keys[not_owned_index], false),
                AccountMeta::new(keys[owned_index], false),
            ];
            let message = Message::new_with_payer(
                &[Instruction::new(program_ids[owned_index], &[0_u8], metas)],
                None,
            );

            // modify account owned by the program
            accounts[owned_index].borrow_mut().data[0] = (MAX_DEPTH + owned_index) as u8;
            invoke_context
                .verify_and_update(
                    &message,
                    &message.instructions[0],
                    &accounts[not_owned_index..owned_index + 1],
                )
                .unwrap();
            assert_eq!(
                invoke_context.pre_accounts[owned_index].data[0],
                (MAX_DEPTH + owned_index) as u8
            );

            // modify account not owned by the program
            let data = accounts[not_owned_index].borrow_mut().data[0];
            accounts[not_owned_index].borrow_mut().data[0] = (MAX_DEPTH + not_owned_index) as u8;
            assert_eq!(
                invoke_context.verify_and_update(
                    &message,
                    &message.instructions[0],
                    &accounts[not_owned_index..owned_index + 1],
                ),
                Err(InstructionError::ExternalAccountDataModified)
            );
            assert_eq!(invoke_context.pre_accounts[not_owned_index].data[0], data);
            accounts[not_owned_index].borrow_mut().data[0] = data;

            invoke_context.pop();
        }
    }

    #[test]
    fn test_is_zeroed() {
        const ZEROS_LEN: usize = 1024;
        let mut buf = [0; ZEROS_LEN];
        assert_eq!(PreAccount::is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(PreAccount::is_zeroed(&buf), false);

        let mut buf = [0; ZEROS_LEN - 1];
        assert_eq!(PreAccount::is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(PreAccount::is_zeroed(&buf), false);

        let mut buf = [0; ZEROS_LEN + 1];
        assert_eq!(PreAccount::is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(PreAccount::is_zeroed(&buf), false);

        let buf = vec![];
        assert_eq!(PreAccount::is_zeroed(&buf), true);
    }

    #[test]
    fn test_verify_account_references() {
        let accounts = vec![(Pubkey::new_rand(), RefCell::new(Account::default()))];

        assert!(MessageProcessor::verify_account_references(&accounts).is_ok());

        let mut _borrowed = accounts[0].1.borrow();
        assert_eq!(
            MessageProcessor::verify_account_references(&accounts),
            Err(InstructionError::AccountBorrowOutstanding)
        );
    }

    struct Change {
        program_id: Pubkey,
        rent: Rent,
        pre: PreAccount,
        post: Account,
    }
    impl Change {
        pub fn new(owner: &Pubkey, program_id: &Pubkey) -> Self {
            Self {
                program_id: *program_id,
                rent: Rent::default(),
                pre: PreAccount::new(
                    &Pubkey::new_rand(),
                    &Account {
                        owner: *owner,
                        lamports: std::u64::MAX,
                        data: vec![],
                        ..Account::default()
                    },
                    false,
                    true,
                ),
                post: Account {
                    owner: *owner,
                    lamports: std::u64::MAX,
                    ..Account::default()
                },
            }
        }
        pub fn read_only(mut self) -> Self {
            self.pre.is_writable = false;
            self
        }
        pub fn executable(mut self, pre: bool, post: bool) -> Self {
            self.pre.is_executable = pre;
            self.post.executable = post;
            self
        }
        pub fn lamports(mut self, pre: u64, post: u64) -> Self {
            self.pre.lamports = pre;
            self.post.lamports = post;
            self
        }
        pub fn owner(mut self, post: &Pubkey) -> Self {
            self.post.owner = *post;
            self
        }
        pub fn data(mut self, pre: Vec<u8>, post: Vec<u8>) -> Self {
            self.pre.data = pre;
            self.post.data = post;
            self
        }
        pub fn rent_epoch(mut self, pre: u64, post: u64) -> Self {
            self.pre.rent_epoch = pre;
            self.post.rent_epoch = post;
            self
        }
        pub fn verify(&self) -> Result<(), InstructionError> {
            self.pre.verify(&self.program_id, &self.rent, &self.post)
        }
    }

    #[test]
    fn test_verify_account_changes_owner() {
        let system_program_id = system_program::id();
        let alice_program_id = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&system_program_id, &system_program_id)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "system program should be able to change the account owner"
        );
        assert_eq!(
            Change::new(&system_program_id, &system_program_id)
                .owner(&alice_program_id)
                .read_only()
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a read-only account"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &system_program_id)
                .owner(&alice_program_id)
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a non-system account"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .data(vec![42], vec![0])
                .verify(),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .data(vec![42], vec![42])
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "mallory should not be able to inject data into the alice program"
        );
    }

    #[test]
    fn test_verify_account_changes_executable() {
        let owner = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();
        let system_program_id = system_program::id();

        assert_eq!(
            Change::new(&owner, &system_program_id)
                .executable(false, true)
                .verify(),
            Err(InstructionError::ExecutableModified),
            "system program can't change executable if system doesn't own the account"
        );
        assert_eq!(
            Change::new(&owner, &system_program_id)
                .executable(true, true)
                .data(vec![1], vec![2])
                .verify(),
            Err(InstructionError::ExecutableDataModified),
            "system program can't change executable data if system doesn't own the account"
        );
        assert_eq!(
            Change::new(&owner, &owner).executable(false, true).verify(),
            Ok(()),
            "owner should be able to change executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .read_only()
                .verify(),
            Err(InstructionError::ExecutableModified),
            "owner can't modify executable of read-only accounts"
        );
        assert_eq!(
            Change::new(&owner, &owner).executable(true, false).verify(),
            Err(InstructionError::ExecutableModified),
            "owner program can't reverse executable"
        );
        assert_eq!(
            Change::new(&owner, &mallory_program_id)
                .executable(false, true)
                .verify(),
            Err(InstructionError::ExecutableModified),
            "malicious Mallory should not be able to change the account executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .data(vec![1], vec![2])
                .verify(),
            Ok(()),
            "account data can change in the same instruction that sets the bit"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .data(vec![1], vec![2])
                .verify(),
            Err(InstructionError::ExecutableDataModified),
            "owner should not be able to change an account's data once its marked executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(1, 2)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to add lamports once makred executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(1, 2)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to add lamports once marked executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(2, 1)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to subtract lamports once marked executable"
        );
        let data = vec![1; 100];
        let min_lamports = Rent::default().minimum_balance(data.len());
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .lamports(0, min_lamports)
                .data(data.clone(), data.clone())
                .verify(),
            Ok(()),
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .lamports(0, min_lamports - 1)
                .data(data.clone(), data)
                .verify(),
            Err(InstructionError::ExecutableAccountNotRentExempt),
            "owner should not be able to change an account's data once its marked executable"
        );
    }

    #[test]
    fn test_verify_account_changes_data_len() {
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Ok(()),
            "system program should be able to change the data len"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
            .data(vec![0], vec![0,0])
            .verify(),
        Err(InstructionError::AccountDataSizeChanged),
        "system program should not be able to change the data length of accounts it does not own"
        );
    }

    #[test]
    fn test_verify_account_changes_data() {
        let alice_program_id = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .verify(),
            Ok(()),
            "alice program should be able to change the data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .verify(),
            Err(InstructionError::ExternalAccountDataModified),
            "non-owner mallory should not be able to change the account data"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .read_only()
                .verify(),
            Err(InstructionError::ReadonlyDataModified),
            "alice isn't allowed to touch a CO account"
        );
    }

    #[test]
    fn test_verify_account_changes_rent_epoch() {
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id()).verify(),
            Ok(()),
            "nothing changed!"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .rent_epoch(0, 1)
                .verify(),
            Err(InstructionError::RentEpochModified),
            "no one touches rent_epoch"
        );
    }

    #[test]
    fn test_verify_account_changes_deduct_lamports_and_reassign_account() {
        let alice_program_id = Pubkey::new_rand();
        let bob_program_id = Pubkey::new_rand();

        // positive test of this capability
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
            .owner(&bob_program_id)
            .lamports(42, 1)
            .data(vec![42], vec![0])
            .verify(),
        Ok(()),
        "alice should be able to deduct lamports and give the account to bob if the data is zeroed",
    );
    }

    #[test]
    fn test_verify_account_changes_lamports() {
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .lamports(42, 0)
                .read_only()
                .verify(),
            Err(InstructionError::ExternalAccountLamportSpend),
            "debit should fail, even if system program"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .lamports(42, 0)
                .read_only()
                .verify(),
            Err(InstructionError::ReadonlyLamportChange),
            "debit should fail, even if owning program"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .lamports(42, 0)
                .owner(&system_program::id())
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program can't debit the account unless it was the pre.owner"
        );
        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .lamports(42, 0)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "system can spend (and change owner)"
        );
    }

    #[test]
    fn test_verify_account_changes_data_size_changed() {
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Err(InstructionError::AccountDataSizeChanged),
            "system program should not be able to change another program's account data size"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![0, 0])
                .verify(),
            Err(InstructionError::AccountDataSizeChanged),
            "non-system programs cannot change their data size"
        );
        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Ok(()),
            "system program should be able to change acount data size"
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
            keyed_accounts: &[KeyedAccount],
            data: &[u8],
        ) -> Result<(), InstructionError> {
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::Correct => Ok(()),
                    MockSystemInstruction::AttemptCredit { lamports } => {
                        keyed_accounts[0].account.borrow_mut().lamports -= lamports;
                        keyed_accounts[1].account.borrow_mut().lamports += lamports;
                        Ok(())
                    }
                    // Change data in a read-only account
                    MockSystemInstruction::AttemptDataChange { data } => {
                        keyed_accounts[1].account.borrow_mut().data = vec![data];
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

        let mut accounts: Vec<Rc<RefCell<Account>>> = Vec::new();
        let account = Account::new_ref(100, 1, &mock_system_program_id);
        accounts.push(account);
        let account = Account::new_ref(0, 1, &mock_system_program_id);
        accounts.push(account);

        let mut loaders: Vec<Vec<(Pubkey, RefCell<Account>)>> = Vec::new();
        let account = RefCell::new(create_loadable_account("mock_system_program"));
        loaders.push(vec![(mock_system_program_id, account)]);

        let from_pubkey = Pubkey::new_rand();
        let to_pubkey = Pubkey::new_rand();
        let account_metas = vec![
            AccountMeta::new(from_pubkey, true),
            AccountMeta::new_readonly(to_pubkey, false),
        ];
        let message = Message::new(&[Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::Correct,
            account_metas.clone(),
        )]);

        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].borrow().lamports, 100);
        assert_eq!(accounts[1].borrow().lamports, 0);

        let message = Message::new(&[Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::AttemptCredit { lamports: 50 },
            account_metas.clone(),
        )]);

        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );

        let message = Message::new(&[Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::AttemptDataChange { data: 50 },
            account_metas,
        )]);

        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
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
            keyed_accounts: &[KeyedAccount],
            data: &[u8],
        ) -> Result<(), InstructionError> {
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::BorrowFail => {
                        let from_account = keyed_accounts[0].try_account_ref_mut()?;
                        let dup_account = keyed_accounts[2].try_account_ref_mut()?;
                        if from_account.lamports != dup_account.lamports {
                            return Err(InstructionError::InvalidArgument);
                        }
                        Ok(())
                    }
                    MockSystemInstruction::MultiBorrowMut => {
                        let from_lamports = {
                            let from_account = keyed_accounts[0].try_account_ref_mut()?;
                            from_account.lamports
                        };
                        let dup_lamports = {
                            let dup_account = keyed_accounts[2].try_account_ref_mut()?;
                            dup_account.lamports
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
                            dup_account.lamports -= lamports;
                            to_account.lamports += lamports;
                            dup_account.data = vec![data];
                        }
                        keyed_accounts[0].try_account_ref_mut()?.lamports -= lamports;
                        keyed_accounts[1].try_account_ref_mut()?.lamports += lamports;
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

        let mut accounts: Vec<Rc<RefCell<Account>>> = Vec::new();
        let account = Account::new_ref(100, 1, &mock_program_id);
        accounts.push(account);
        let account = Account::new_ref(0, 1, &mock_program_id);
        accounts.push(account);

        let mut loaders: Vec<Vec<(Pubkey, RefCell<Account>)>> = Vec::new();
        let account = RefCell::new(create_loadable_account("mock_system_program"));
        loaders.push(vec![(mock_program_id, account)]);

        let from_pubkey = Pubkey::new_rand();
        let to_pubkey = Pubkey::new_rand();
        let dup_pubkey = from_pubkey;
        let account_metas = vec![
            AccountMeta::new(from_pubkey, true),
            AccountMeta::new(to_pubkey, false),
            AccountMeta::new(dup_pubkey, false),
        ];

        // Try to borrow mut the same account
        let message = Message::new(&[Instruction::new(
            mock_program_id,
            &MockSystemInstruction::BorrowFail,
            account_metas.clone(),
        )]);
        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::AccountBorrowFailed
            ))
        );

        // Try to borrow mut the same account in a safe way
        let message = Message::new(&[Instruction::new(
            mock_program_id,
            &MockSystemInstruction::MultiBorrowMut,
            account_metas.clone(),
        )]);
        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
        assert_eq!(result, Ok(()));

        // Do work on the same account but at different location in keyed_accounts[]
        let message = Message::new(&[Instruction::new(
            mock_program_id,
            &MockSystemInstruction::DoWork {
                lamports: 10,
                data: 42,
            },
            account_metas,
        )]);
        let result =
            message_processor.process_message(&message, &loaders, &accounts, &rent_collector);
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].borrow().lamports, 80);
        assert_eq!(accounts[1].borrow().lamports, 20);
        assert_eq!(accounts[0].borrow().data, vec![42]);
    }

    #[test]
    fn test_process_cross_program() {
        #[derive(Serialize, Deserialize)]
        enum MockInstruction {
            NoopSuccess,
            NoopFail,
            ModifyOwned,
            ModifyNotOwned,
        }

        fn mock_process_instruction(
            program_id: &Pubkey,
            keyed_accounts: &[KeyedAccount],
            data: &[u8],
        ) -> Result<(), InstructionError> {
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
                        keyed_accounts[0].try_account_ref_mut()?.data[0] = 1
                    }
                    MockInstruction::ModifyNotOwned => {
                        keyed_accounts[1].try_account_ref_mut()?.data[0] = 1
                    }
                }
            } else {
                return Err(InstructionError::InvalidInstructionData);
            }
            Ok(())
        }

        let caller_program_id = Pubkey::new_rand();
        let callee_program_id = Pubkey::new_rand();
        let mut message_processor = MessageProcessor::default();
        message_processor.add_program(callee_program_id, mock_process_instruction);

        let mut program_account = Account::new(1, 0, &native_loader::id());
        program_account.executable = true;
        let executable_accounts = vec![(callee_program_id, RefCell::new(program_account))];

        let owned_key = Pubkey::new_rand();
        let owned_account = Account::new(42, 1, &callee_program_id);
        let owned_preaccount = PreAccount::new(&owned_key, &owned_account, false, true);

        let not_owned_key = Pubkey::new_rand();
        let not_owned_account = Account::new(84, 1, &Pubkey::new_rand());
        let not_owned_preaccount = PreAccount::new(&not_owned_key, &not_owned_account, false, true);

        let mut accounts = vec![
            Rc::new(RefCell::new(owned_account)),
            Rc::new(RefCell::new(not_owned_account)),
        ];
        let mut invoke_context = ThisInvokeContext::new(
            &caller_program_id,
            Rent::default(),
            vec![owned_preaccount, not_owned_preaccount],
            vec![],
        );
        let metas = vec![
            AccountMeta::new(owned_key, false),
            AccountMeta::new(not_owned_key, false),
        ];

        // not owned account modified by the caller (before the invoke)
        accounts[0].borrow_mut().data[0] = 1;
        let instruction = Instruction::new(
            callee_program_id,
            &MockInstruction::NoopSuccess,
            metas.clone(),
        );
        let message = Message::new_with_payer(&[instruction], None);
        assert_eq!(
            message_processor.process_cross_program_instruction(
                &message,
                &executable_accounts,
                &accounts,
                &mut invoke_context,
            ),
            Err(InstructionError::ExternalAccountDataModified)
        );
        accounts[0].borrow_mut().data[0] = 0;

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
            let instruction = Instruction::new(callee_program_id, &case.0, metas.clone());
            let message = Message::new_with_payer(&[instruction], None);
            assert_eq!(
                message_processor.process_cross_program_instruction(
                    &message,
                    &executable_accounts,
                    &accounts,
                    &mut invoke_context,
                ),
                case.1
            );
        }
    }
}
