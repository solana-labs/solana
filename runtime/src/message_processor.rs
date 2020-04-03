use crate::{
    native_loader::NativeLoader, rent_collector::RentCollector, system_instruction_processor,
};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    account::{create_keyed_readonly_accounts, Account, KeyedAccount},
    clock::Epoch,
    instruction::{CompiledInstruction, InstructionError},
    message::Message,
    native_loader,
    pubkey::Pubkey,
    system_program,
    transaction::TransactionError,
};
use std::{cell::RefCell, rc::Rc};

// The relevant state of an account before an Instruction executes, used
// to verify account integrity after the Instruction completes
#[derive(Debug)]
pub struct PreAccount {
    pub is_writable: bool,
    pub lamports: u64,
    pub data_len: usize,
    pub data: Option<Vec<u8>>,
    pub owner: Pubkey,
    pub executable: bool,
    pub rent_epoch: Epoch,
}
impl PreAccount {
    pub fn new(account: &Account, is_writable: bool, program_id: &Pubkey) -> Self {
        Self {
            is_writable,
            lamports: account.lamports,
            data_len: account.data.len(),
            data: if Self::should_verify_data(
                &account.owner,
                program_id,
                is_writable,
                account.executable,
            ) {
                Some(account.data.clone())
            } else {
                None
            },
            owner: account.owner,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }

    fn should_verify_data(
        owner: &Pubkey,
        program_id: &Pubkey,
        is_writable: bool,
        is_executable: bool,
    ) -> bool {
        // For accounts not assigned to the program, the data may not change.
        program_id != owner
        // Read-only account data may not change.
        || !is_writable
        // Executable account data may not change.
        || is_executable
    }

    pub fn verify(
        &self,
        program_id: &Pubkey,
        rent_collector: &RentCollector,
        post: &Account,
    ) -> Result<(), InstructionError> {
        // Only the owner of the account may change owner and
        //   only if the account is writable and
        //   only if the data is zero-initialized or empty
        if self.owner != post.owner
            && (!self.is_writable // line coverage used to get branch coverage
            || *program_id != self.owner // line coverage used to get branch coverage
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
            if self.executable {
                return Err(InstructionError::ExecutableLamportChange);
            }
        }

        // Only the system program can change the size of the data
        //  and only if the system program owns the account
        if self.data_len != post.data.len()
            && (!system_program::check_id(program_id) // line coverage used to get branch coverage
            || !system_program::check_id(&self.owner))
        {
            return Err(InstructionError::AccountDataSizeChanged);
        }

        if Self::should_verify_data(&self.owner, program_id, self.is_writable, self.executable) {
            match &self.data {
                Some(data) if *data == post.data => (),
                _ => {
                    if self.executable {
                        return Err(InstructionError::ExecutableDataModified);
                    } else if self.is_writable {
                        return Err(InstructionError::ExternalAccountDataModified);
                    } else {
                        return Err(InstructionError::ReadonlyDataModified);
                    }
                }
            }
        }

        // executable is one-way (false->true) and only the account owner may set it.
        if self.executable != post.executable {
            if !rent_collector
                .rent
                .is_exempt(post.lamports, post.data.len())
            {
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }
            if !self.is_writable // line coverage used to get branch coverage
                || self.executable // line coverage used to get branch coverage
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

    pub fn is_zeroed(buf: &[u8]) -> bool {
        const ZEROS_LEN: usize = 1024;
        static ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
        let mut chunks = buf.chunks_exact(ZEROS_LEN);

        chunks.all(|chunk| chunk == &ZEROS[..])
            && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
    }
}

pub type ProcessInstruction = fn(&Pubkey, &[KeyedAccount], &[u8]) -> Result<(), InstructionError>;

#[derive(Serialize, Deserialize)]
pub struct MessageProcessor {
    #[serde(skip)]
    instruction_processors: Vec<(Pubkey, ProcessInstruction)>,
    #[serde(skip)]
    native_loader: NativeLoader,
}
impl Default for MessageProcessor {
    fn default() -> Self {
        let instruction_processors: Vec<(Pubkey, ProcessInstruction)> = vec![(
            system_program::id(),
            system_instruction_processor::process_instruction,
        )];

        Self {
            instruction_processors,
            native_loader: NativeLoader::default(),
        }
    }
}
impl MessageProcessor {
    /// Add a static entrypoint to intercept instructions before the dynamic loader.
    pub fn add_instruction_processor(
        &mut self,
        program_id: Pubkey,
        process_instruction: ProcessInstruction,
    ) {
        self.instruction_processors
            .push((program_id, process_instruction));
    }

    /// Process an instruction
    /// This method calls the instruction's program entrypoint method
    fn process_instruction(
        &self,
        message: &Message,
        instruction: &CompiledInstruction,
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts = create_keyed_readonly_accounts(executable_accounts);
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

        for (id, process_instruction) in &self.instruction_processors {
            let root_program_id = keyed_accounts[0].unsigned_key();
            if id == root_program_id {
                return process_instruction(
                    &root_program_id,
                    &keyed_accounts[1..],
                    &instruction.data,
                );
            }
        }

        if native_loader::check_id(&keyed_accounts[0].owner()?) {
            self.native_loader.process_instruction(
                &native_loader::id(),
                &keyed_accounts,
                &instruction.data,
            )
        } else {
            Err(InstructionError::UnsupportedProgramId)
        }
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
            let program_id = instruction.program_id(&message.account_keys);
            let mut work = |_unique_index: usize, account_index: usize| {
                let is_writable = message.is_writable(account_index);
                let account = accounts[account_index].borrow();
                pre_accounts.push(PreAccount::new(&account, is_writable, program_id));
                Ok(())
            };
            let _ = instruction.visit_each_account(&mut work);
        }
        pre_accounts
    }

    /// Verify there are no outstanding borrows
    pub fn verify_account_references(
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        for account in accounts.iter() {
            account
                .try_borrow_mut()
                .map_err(|_| InstructionError::AccountBorrowOutstanding)?;
        }
        for (_, account) in executable_accounts.iter() {
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
        rent_collector: &RentCollector,
    ) -> Result<(), InstructionError> {
        // Verify all accounts have zero outstanding refs
        Self::verify_account_references(executable_accounts, accounts)?;

        // Verify the per-account instruction results
        let (mut pre_sum, mut post_sum) = (0_u128, 0_u128);
        {
            let program_id = instruction.program_id(&message.account_keys);
            let mut work = |unique_index: usize, account_index: usize| {
                let account = accounts[account_index].borrow();
                pre_accounts[unique_index].verify(&program_id, rent_collector, &account)?;
                pre_sum += u128::from(pre_accounts[unique_index].lamports);
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
        self.process_instruction(message, instruction, executable_accounts, accounts)?;
        Self::verify(
            message,
            instruction,
            &pre_accounts,
            executable_accounts,
            accounts,
            rent_collector,
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
        native_program_info,
    };

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
        let executable_accounts = vec![(Pubkey::new_rand(), RefCell::new(Account::default()))];
        let program_accounts = vec![Rc::new(RefCell::new(Account::default()))];

        assert!(MessageProcessor::verify_account_references(
            &executable_accounts,
            &program_accounts,
        )
        .is_ok());

        let cloned = program_accounts[0].clone();
        let _borrowed = cloned.borrow();
        assert_eq!(
            MessageProcessor::verify_account_references(&executable_accounts, &program_accounts,),
            Err(InstructionError::AccountBorrowOutstanding)
        );

        let cloned = executable_accounts[0].1.clone();
        let _borrowed = cloned.borrow();
        assert_eq!(
            MessageProcessor::verify_account_references(&executable_accounts, &program_accounts,),
            Err(InstructionError::AccountBorrowOutstanding)
        );
    }

    #[test]
    fn test_verify_account_changes_owner() {
        fn change_owner(
            ix: &Pubkey,
            pre: &Pubkey,
            post: &Pubkey,
            is_writable: bool,
        ) -> Result<(), InstructionError> {
            PreAccount::new(&Account::new(0, 0, pre), is_writable, ix).verify(
                ix,
                &RentCollector::default(),
                &Account::new(0, 0, post),
            )
        }

        let system_program_id = system_program::id();
        let alice_program_id = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            change_owner(
                &system_program_id,
                &system_program_id,
                &alice_program_id,
                true
            ),
            Ok(()),
            "system program should be able to change the account owner"
        );
        assert_eq!(
            change_owner(
                &system_program_id,
                &system_program_id,
                &alice_program_id,
                false
            ),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a read-only account"
        );
        assert_eq!(
            change_owner(
                &system_program_id,
                &mallory_program_id,
                &alice_program_id,
                true
            ),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a non-system account"
        );
        assert_eq!(
            change_owner(
                &mallory_program_id,
                &mallory_program_id,
                &alice_program_id,
                true
            ),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            PreAccount::new(
                &Account::new_data(0, &[42], &mallory_program_id).unwrap(),
                true,
                &mallory_program_id,
            )
            .verify(
                &mallory_program_id,
                &RentCollector::default(),
                &Account::new_data(0, &[0], &alice_program_id).unwrap(),
            ),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            PreAccount::new(
                &Account::new_data(0, &[42], &mallory_program_id).unwrap(),
                true,
                &mallory_program_id,
            )
            .verify(
                &mallory_program_id,
                &RentCollector::default(),
                &Account::new_data(0, &[42], &alice_program_id).unwrap(),
            ),
            Err(InstructionError::ModifiedProgramId),
            "mallory should not be able to inject data into the alice program"
        );
    }

    #[test]
    fn test_verify_account_changes_executable() {
        let owner = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();
        let system_program_id = system_program::id();

        struct Change {
            pre: PreAccount,
            post: Account,
            program_id: Pubkey,
        }
        impl Change {
            pub fn new(owner: &Pubkey, program_id: &Pubkey) -> Self {
                Self {
                    pre: PreAccount::new(
                        &Account {
                            owner: *owner,
                            lamports: std::u64::MAX,
                            data: vec![],
                            ..Account::default()
                        },
                        true,
                        &Pubkey::new_rand(), // Force some data, ignored if not needed
                    ),
                    post: Account {
                        owner: *owner,
                        lamports: std::u64::MAX,
                        ..Account::default()
                    },
                    program_id: *program_id,
                }
            }
            pub fn read_only(mut self) -> Self {
                self.pre.is_writable = false;
                self
            }
            pub fn executable(mut self, pre: bool, post: bool) -> Self {
                self.pre.executable = pre;
                self.post.executable = post;
                self
            }
            pub fn lamports(mut self, pre: u64, post: u64) -> Self {
                self.pre.lamports = pre;
                self.post.lamports = post;
                self
            }
            pub fn data(mut self, pre: Vec<u8>, post: Vec<u8>) -> Self {
                self.pre.data_len = pre.len();
                self.pre.data = Some(pre);
                self.post.data = post;
                self
            }
            pub fn verify(self) -> Result<(), InstructionError> {
                self.pre
                    .verify(&self.program_id, &RentCollector::default(), &self.post)
            }
        }

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
        let min_lamports = RentCollector::default().rent.minimum_balance(data.len());
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
                .data(data.clone(), data.clone())
                .verify(),
            Err(InstructionError::ExecutableAccountNotRentExempt),
            "owner should not be able to change an account's data once its marked executable"
        );
    }

    #[test]
    fn test_verify_account_changes_data_len() {
        assert_eq!(
            PreAccount::new(
                &Account::new_data(0, &[0], &system_program::id()).unwrap(),
                true,
                &system_program::id()
            )
            .verify(
                &system_program::id(),
                &RentCollector::default(),
                &Account::new_data(0, &[0, 0], &system_program::id()).unwrap()
            ),
            Ok(()),
            "system program should be able to change the data len"
        );
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
                PreAccount::new(
                    &Account::new_data(0, &[0], &alice_program_id).unwrap(),
                    true,
                    &system_program::id(),
                ).verify(
                &system_program::id(), &RentCollector::default(),
                &Account::new_data(0, &[0, 0], &alice_program_id).unwrap(),
            ),
            Err(InstructionError::AccountDataSizeChanged),
            "system program should not be able to change the data length of accounts it does not own"
        );
    }

    #[test]
    fn test_verify_account_changes_data() {
        let alice_program_id = Pubkey::new_rand();
        let change_data =
            |program_id: &Pubkey, is_writable: bool| -> Result<(), InstructionError> {
                let pre = PreAccount::new(
                    &Account::new_data(0, &[0], &alice_program_id).unwrap(),
                    is_writable,
                    &program_id,
                );
                let post = Account::new_data(0, &[42], &alice_program_id).unwrap();
                pre.verify(&program_id, &RentCollector::default(), &post)
            };

        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            change_data(&alice_program_id, true),
            Ok(()),
            "alice program should be able to change the data"
        );
        assert_eq!(
            change_data(&mallory_program_id, true),
            Err(InstructionError::ExternalAccountDataModified),
            "non-owner mallory should not be able to change the account data"
        );

        assert_eq!(
            change_data(&alice_program_id, false),
            Err(InstructionError::ReadonlyDataModified),
            "alice isn't allowed to touch a CO account"
        );
    }

    #[test]
    fn test_verify_account_changes_rent_epoch() {
        let alice_program_id = Pubkey::new_rand();
        let rent_collector = RentCollector::default();
        let pre = PreAccount::new(
            &Account::new(0, 0, &alice_program_id),
            false,
            &system_program::id(),
        );
        let mut post = Account::new(0, 0, &alice_program_id);

        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Ok(()),
            "nothing changed!"
        );

        post.rent_epoch += 1;
        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Err(InstructionError::RentEpochModified),
            "no one touches rent_epoch"
        );
    }

    #[test]
    fn test_verify_account_changes_deduct_lamports_and_reassign_account() {
        let alice_program_id = Pubkey::new_rand();
        let bob_program_id = Pubkey::new_rand();
        let pre = PreAccount::new(
            &Account::new_data(42, &[42], &alice_program_id).unwrap(),
            true,
            &alice_program_id,
        );
        let post = Account::new_data(1, &[0], &bob_program_id).unwrap();

        // positive test of this capability
        assert_eq!(
            pre.verify(&alice_program_id, &RentCollector::default(), &post),
            Ok(()),
            "alice should be able to deduct lamports and give the account to bob if the data is zeroed",
        );
    }

    #[test]
    fn test_verify_account_changes_lamports() {
        let alice_program_id = Pubkey::new_rand();
        let rent_collector = RentCollector::default();
        let pre = PreAccount::new(
            &Account::new(42, 0, &alice_program_id),
            false,
            &system_program::id(),
        );
        let post = Account::new(0, 0, &alice_program_id);

        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Err(InstructionError::ExternalAccountLamportSpend),
            "debit should fail, even if system program"
        );

        let pre = PreAccount::new(
            &Account::new(42, 0, &alice_program_id),
            false,
            &alice_program_id,
        );

        assert_eq!(
            pre.verify(&alice_program_id, &rent_collector, &post),
            Err(InstructionError::ReadonlyLamportChange),
            "debit should fail, even if owning program"
        );

        let pre = PreAccount::new(
            &Account::new(42, 0, &alice_program_id),
            true,
            &system_program::id(),
        );
        let post = Account::new(0, 0, &system_program::id());
        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Err(InstructionError::ModifiedProgramId),
            "system program can't debit the account unless it was the pre.owner"
        );

        let pre = PreAccount::new(
            &Account::new(42, 0, &system_program::id()),
            true,
            &system_program::id(),
        );
        let post = Account::new(0, 0, &alice_program_id);
        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Ok(()),
            "system can spend (and change owner)"
        );
    }

    #[test]
    fn test_verify_account_changes_data_size_changed() {
        let rent_collector = RentCollector::default();
        let alice_program_id = Pubkey::new_rand();
        let pre = PreAccount::new(
            &Account::new_data(42, &[0], &alice_program_id).unwrap(),
            true,
            &system_program::id(),
        );
        let post = Account::new_data(42, &[0, 0], &alice_program_id).unwrap();
        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
            Err(InstructionError::AccountDataSizeChanged),
            "system program should not be able to change another program's account data size"
        );
        let pre = PreAccount::new(
            &Account::new_data(42, &[0], &alice_program_id).unwrap(),
            true,
            &alice_program_id,
        );
        assert_eq!(
            pre.verify(&alice_program_id, &rent_collector, &post),
            Err(InstructionError::AccountDataSizeChanged),
            "non-system programs cannot change their data size"
        );
        let pre = PreAccount::new(
            &Account::new_data(42, &[0], &system_program::id()).unwrap(),
            true,
            &system_program::id(),
        );
        assert_eq!(
            pre.verify(&system_program::id(), &rent_collector, &post),
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
        message_processor
            .add_instruction_processor(mock_system_program_id, mock_system_process_instruction);

        let mut accounts: Vec<Rc<RefCell<Account>>> = Vec::new();
        let account = Account::new_ref(100, 1, &mock_system_program_id);
        accounts.push(account);
        let account = Account::new_ref(0, 1, &mock_system_program_id);
        accounts.push(account);

        let mut loaders: Vec<Vec<(Pubkey, RefCell<Account>)>> = Vec::new();
        let account = RefCell::new(create_loadable_account(&native_program_info!(
            "mock_system_program"
        )));
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
        message_processor
            .add_instruction_processor(mock_program_id, mock_system_process_instruction);

        let mut accounts: Vec<Rc<RefCell<Account>>> = Vec::new();
        let account = Account::new_ref(100, 1, &mock_program_id);
        accounts.push(account);
        let account = Account::new_ref(0, 1, &mock_program_id);
        accounts.push(account);

        let mut loaders: Vec<Vec<(Pubkey, RefCell<Account>)>> = Vec::new();
        let account = RefCell::new(create_loadable_account(&native_program_info!(
            "mock_system_program"
        )));
        loaders.push(vec![(mock_program_id, account)]);

        let from_pubkey = Pubkey::new_rand();
        let to_pubkey = Pubkey::new_rand();
        let dup_pubkey = from_pubkey.clone();
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
}
