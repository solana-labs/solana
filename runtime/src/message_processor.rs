use crate::native_loader;
use crate::system_instruction_processor;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    account::{create_keyed_readonly_accounts, Account, KeyedAccount},
    clock::Epoch,
    entrypoint_native,
    instruction::{CompiledInstruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    system_program,
    transaction::TransactionError,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::RwLock};

#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;

// The relevant state of an account before an Instruction executes, used
// to verify account integrity after the Instruction completes
pub struct PreInstructionAccount {
    pub is_writable: bool,
    pub lamports: u64,
    pub data_len: usize,
    pub data: Option<Vec<u8>>,
    pub owner: Pubkey,
    pub executable: bool,
    pub rent_epoch: Epoch,
}
impl PreInstructionAccount {
    pub fn new(account: &Account, is_writable: bool, copy_data: bool) -> Self {
        Self {
            is_writable,
            lamports: account.lamports,
            data_len: account.data.len(),
            data: if copy_data {
                Some(account.data.clone())
            } else {
                None
            },
            owner: account.owner,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }
}
pub fn need_account_data_checked(program_id: &Pubkey, owner: &Pubkey, is_writable: bool) -> bool {
    // For accounts not assigned to the program, the data may not change.
    program_id != owner
    // Read-only account data may not change.
    || !is_writable
}
pub fn verify_account_changes(
    program_id: &Pubkey,
    pre: &PreInstructionAccount,
    post: &Account,
) -> Result<(), InstructionError> {
    // Verify the transaction

    // Only the owner of the account may change owner and
    //   only if the account is writable and
    //   only if the data is zero-initialized or empty
    if pre.owner != post.owner
        && (!pre.is_writable // line coverage used to get branch coverage
            || *program_id != pre.owner // line coverage used to get branch coverage
            || !is_zeroed(&post.data))
    {
        return Err(InstructionError::ModifiedProgramId);
    }

    // An account not assigned to the program cannot have its balance decrease.
    if *program_id != pre.owner // line coverage used to get branch coverage
        && pre.lamports > post.lamports
    {
        return Err(InstructionError::ExternalAccountLamportSpend);
    }

    // The balance of read-only accounts may not change.
    if !pre.is_writable // line coverage used to get branch coverage
        && pre.lamports != post.lamports
    {
        return Err(InstructionError::ReadonlyLamportChange);
    }

    // Only the system program can change the size of the data
    //  and only if the system program owns the account
    if pre.data_len != post.data.len()
        && (!system_program::check_id(program_id) // line coverage used to get branch coverage
            || !system_program::check_id(&pre.owner))
    {
        return Err(InstructionError::AccountDataSizeChanged);
    }

    if need_account_data_checked(&pre.owner, program_id, pre.is_writable) {
        match &pre.data {
            Some(data) if *data == post.data => (),
            _ => {
                if !pre.is_writable {
                    return Err(InstructionError::ReadonlyDataModified);
                } else {
                    return Err(InstructionError::ExternalAccountDataModified);
                }
            }
        }
    }

    // executable is one-way (false->true) and only the account owner may set it.
    if pre.executable != post.executable
        && (!pre.is_writable // line coverage used to get branch coverage
            || pre.executable // line coverage used to get branch coverage
            || *program_id != pre.owner)
    {
        return Err(InstructionError::ExecutableModified);
    }

    // No one modifies rent_epoch (yet).
    if pre.rent_epoch != post.rent_epoch {
        return Err(InstructionError::RentEpochModified);
    }

    Ok(())
}

pub type ProcessInstruction = fn(&Pubkey, &[KeyedAccount], &[u8]) -> Result<(), InstructionError>;

pub type SymbolCache = RwLock<HashMap<Vec<u8>, Symbol<entrypoint_native::Entrypoint>>>;

#[derive(Serialize, Deserialize)]
pub struct MessageProcessor {
    #[serde(skip)]
    instruction_processors: Vec<(Pubkey, ProcessInstruction)>,
    #[serde(skip)]
    symbol_cache: SymbolCache,
}

impl Default for MessageProcessor {
    fn default() -> Self {
        let instruction_processors: Vec<(Pubkey, ProcessInstruction)> = vec![(
            system_program::id(),
            system_instruction_processor::process_instruction,
        )];

        Self {
            instruction_processors,
            symbol_cache: RwLock::new(HashMap::new()),
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
        program_accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        let program_id = instruction.program_id(&message.account_keys);
        let mut keyed_accounts = create_keyed_readonly_accounts(executable_accounts);
        let mut keyed_accounts2: Vec<_> = instruction
            .accounts
            .iter()
            .map(|&index| {
                let index = index as usize;
                let key = &message.account_keys[index];
                let is_writable = message.is_writable(index);
                (
                    key,
                    index < message.header.num_required_signatures as usize,
                    is_writable,
                )
            })
            .zip(program_accounts.iter())
            .map(|((key, is_signer, is_writable), account)| {
                if is_writable {
                    KeyedAccount::new(key, is_signer, account)
                } else {
                    KeyedAccount::new_readonly(key, is_signer, account)
                }
            })
            .collect();
        keyed_accounts.append(&mut keyed_accounts2);

        assert!(
            keyed_accounts[0].try_account_ref()?.executable,
            "loader not executable"
        );

        let loader_id = keyed_accounts[0].unsigned_key();
        for (id, process_instruction) in &self.instruction_processors {
            if id == loader_id {
                return process_instruction(&program_id, &keyed_accounts[1..], &instruction.data);
            }
        }

        native_loader::invoke_entrypoint(
            &program_id,
            &keyed_accounts,
            &instruction.data,
            &self.symbol_cache,
        )
    }

    pub fn verify_account_references(
        executable_accounts: &[(Pubkey, RefCell<Account>)],
        program_accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        for account in program_accounts.iter() {
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

    pub fn sum_account_lamports(accounts: &[Rc<RefCell<Account>>]) -> u128 {
        // Note: This is an O(n^2) algorithm,
        // but performed on a very small slice and requires no heap allocations
        accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                for account in accounts.iter().skip(i + 1) {
                    if Rc::ptr_eq(a, account) {
                        return 0; // don't double count duplicates
                    }
                }
                u128::from(a.borrow().lamports)
            })
            .sum()
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
        program_accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        assert_eq!(instruction.accounts.len(), program_accounts.len());
        let program_id = instruction.program_id(&message.account_keys);
        // Copy only what we need to verify after instruction processing
        let pre_accounts: Vec<_> = program_accounts
            .iter()
            .enumerate()
            .map(|(i, account)| {
                let is_writable = message.is_writable(instruction.accounts[i] as usize);
                let account = account.borrow();
                PreInstructionAccount::new(
                    &account,
                    is_writable,
                    need_account_data_checked(&account.owner, program_id, is_writable),
                )
            })
            .collect();
        // Sum total lamports before instruction processing
        let pre_total = Self::sum_account_lamports(program_accounts);

        self.process_instruction(message, instruction, executable_accounts, program_accounts)?;

        // Verify all accounts have zero outstanding refs
        Self::verify_account_references(executable_accounts, program_accounts)?;
        // Verify the instruction
        for (pre_account, post_account) in pre_accounts.iter().zip(program_accounts.iter()) {
            let post_account = post_account
                .try_borrow()
                .map_err(|_| InstructionError::AccountBorrowFailed)?;
            verify_account_changes(&program_id, pre_account, &post_account)?;
        }
        // Verify total sum of all the lamports did not change
        let post_total = Self::sum_account_lamports(program_accounts);
        if pre_total != post_total {
            return Err(InstructionError::UnbalancedInstruction);
        }
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
    ) -> Result<(), TransactionError> {
        for (instruction_index, instruction) in message.instructions.iter().enumerate() {
            let executable_index = message
                .program_position(instruction.program_id_index as usize)
                .ok_or(TransactionError::InvalidAccountIndex)?;
            let executable_accounts = &loaders[executable_index];

            // TODO: panics on an index out of bounds if an executable
            // account is also included as a regular account for an instruction, because the
            // executable account is not passed in as part of the accounts slice
            let program_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .map(|i| accounts[*i as usize].clone())
                .collect();

            self.execute_instruction(message, instruction, executable_accounts, &program_accounts)
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
        }
        Ok(())
    }
}

pub const ZEROS_LEN: usize = 1024;
static ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
pub fn is_zeroed(buf: &[u8]) -> bool {
    let mut chunks = buf.chunks_exact(ZEROS_LEN);

    chunks.all(|chunk| chunk == &ZEROS[..])
        && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
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
    fn test_is_zeroed() {
        let mut buf = [0; ZEROS_LEN];
        assert_eq!(is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(is_zeroed(&buf), false);

        let mut buf = [0; ZEROS_LEN - 1];
        assert_eq!(is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(is_zeroed(&buf), false);

        let mut buf = [0; ZEROS_LEN + 1];
        assert_eq!(is_zeroed(&buf), true);
        buf[0] = 1;
        assert_eq!(is_zeroed(&buf), false);

        let buf = vec![];
        assert_eq!(is_zeroed(&buf), true);
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
    fn test_sum_account_lamports() {
        let owner_pubkey = Pubkey::new_rand();
        let account1 = Rc::new(RefCell::new(Account::new(1, 1, &owner_pubkey)));
        let account2 = Rc::new(RefCell::new(Account::new(2, 1, &owner_pubkey)));
        let account3 = Rc::new(RefCell::new(Account::new(3, 1, &owner_pubkey)));

        assert_eq!(0, MessageProcessor::sum_account_lamports(&vec![]));
        assert_eq!(
            6,
            MessageProcessor::sum_account_lamports(&vec![
                account1.clone(),
                account2.clone(),
                account3.clone()
            ])
        );
        assert_eq!(
            3,
            MessageProcessor::sum_account_lamports(&vec![
                account1.clone(),
                account2.clone(),
                account1.clone()
            ])
        );
        assert_eq!(
            1,
            MessageProcessor::sum_account_lamports(&vec![
                account1.clone(),
                account1.clone(),
                account1.clone()
            ])
        );
        assert_eq!(
            6,
            MessageProcessor::sum_account_lamports(&vec![
                account1.clone(),
                account2.clone(),
                account3.clone(),
                account1.clone(),
                account2.clone(),
                account3.clone(),
            ])
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
            verify_account_changes(
                &ix,
                &PreInstructionAccount::new(
                    &Account::new(0, 0, pre),
                    is_writable,
                    need_account_data_checked(pre, ix, is_writable),
                ),
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
            verify_account_changes(
                &mallory_program_id,
                &PreInstructionAccount::new(
                    &Account::new_data(0, &[42], &mallory_program_id).unwrap(),
                    true,
                    need_account_data_checked(&mallory_program_id, &mallory_program_id, true),
                ),
                &Account::new_data(0, &[0], &alice_program_id,).unwrap(),
            ),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            verify_account_changes(
                &mallory_program_id,
                &PreInstructionAccount::new(
                    &Account::new_data(0, &[42], &mallory_program_id).unwrap(),
                    true,
                    need_account_data_checked(&mallory_program_id, &mallory_program_id, true),
                ),
                &Account::new_data(0, &[42], &alice_program_id,).unwrap(),
            ),
            Err(InstructionError::ModifiedProgramId),
            "mallory should not be able to inject data into the alice program"
        );
    }

    #[test]
    fn test_verify_account_changes_executable() {
        let owner = Pubkey::new_rand();
        let change_executable = |program_id: &Pubkey,
                                 is_writable: bool,
                                 pre_executable: bool,
                                 post_executable: bool|
         -> Result<(), InstructionError> {
            let pre = PreInstructionAccount::new(
                &Account {
                    owner,
                    executable: pre_executable,
                    ..Account::default()
                },
                is_writable,
                need_account_data_checked(&owner, &program_id, is_writable),
            );

            let post = Account {
                owner,
                executable: post_executable,
                ..Account::default()
            };
            verify_account_changes(&program_id, &pre, &post)
        };

        let mallory_program_id = Pubkey::new_rand();
        let system_program_id = system_program::id();

        assert_eq!(
            change_executable(&system_program_id, true, false, true),
            Err(InstructionError::ExecutableModified),
            "system program can't change executable if system doesn't own the account"
        );
        assert_eq!(
            change_executable(&owner, true, false, true),
            Ok(()),
            "alice program should be able to change executable"
        );
        assert_eq!(
            change_executable(&owner, false, false, true),
            Err(InstructionError::ExecutableModified),
            "system program can't modify executable of read-only accounts"
        );
        assert_eq!(
            change_executable(&owner, true, true, false),
            Err(InstructionError::ExecutableModified),
            "system program can't reverse executable"
        );
        assert_eq!(
            change_executable(&mallory_program_id, true, false, true),
            Err(InstructionError::ExecutableModified),
            "malicious Mallory should not be able to change the account executable"
        );
    }

    #[test]
    fn test_verify_account_changes_data_len() {
        assert_eq!(
            verify_account_changes(
                &system_program::id(),
                &PreInstructionAccount::new(
                    &Account::new_data(0, &[0], &system_program::id()).unwrap(),
                    true,
                    need_account_data_checked(&system_program::id(), &system_program::id(), true),
                ),
                &Account::new_data(0, &[0, 0], &system_program::id()).unwrap(),
            ),
            Ok(()),
            "system program should be able to change the data len"
        );
        let alice_program_id = Pubkey::new_rand();

        assert_eq!(
            verify_account_changes(
                &system_program::id(),
                &PreInstructionAccount::new(
                    &Account::new_data(0, &[0], &alice_program_id).unwrap(),
                    true,
                    need_account_data_checked(&alice_program_id, &system_program::id(), true),
                ),
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
                let pre = PreInstructionAccount::new(
                    &Account::new_data(0, &[0], &alice_program_id).unwrap(),
                    is_writable,
                    need_account_data_checked(&alice_program_id, &program_id, is_writable),
                );
                let post = Account::new_data(0, &[42], &alice_program_id).unwrap();
                verify_account_changes(&program_id, &pre, &post)
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
        let pre = PreInstructionAccount::new(
            &Account::new(0, 0, &alice_program_id),
            false,
            need_account_data_checked(&alice_program_id, &system_program::id(), false),
        );
        let mut post = Account::new(0, 0, &alice_program_id);

        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Ok(()),
            "nothing changed!"
        );

        post.rent_epoch += 1;
        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Err(InstructionError::RentEpochModified),
            "no one touches rent_epoch"
        );
    }

    #[test]
    fn test_verify_account_changes_deduct_lamports_and_reassign_account() {
        let alice_program_id = Pubkey::new_rand();
        let bob_program_id = Pubkey::new_rand();
        let pre = PreInstructionAccount::new(
            &Account::new_data(42, &[42], &alice_program_id).unwrap(),
            true,
            need_account_data_checked(&alice_program_id, &alice_program_id, true),
        );
        let post = Account::new_data(1, &[0], &bob_program_id).unwrap();

        // positive test of this capability
        assert_eq!(
            verify_account_changes(&alice_program_id, &pre, &post),
            Ok(()),
            "alice should be able to deduct lamports and give the account to bob if the data is zeroed",
        );
    }

    #[test]
    fn test_verify_account_changes_lamports() {
        let alice_program_id = Pubkey::new_rand();
        let pre = PreInstructionAccount::new(
            &Account::new(42, 0, &alice_program_id),
            false,
            need_account_data_checked(&alice_program_id, &system_program::id(), false),
        );
        let post = Account::new(0, 0, &alice_program_id);

        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Err(InstructionError::ExternalAccountLamportSpend),
            "debit should fail, even if system program"
        );

        let pre = PreInstructionAccount::new(
            &Account::new(42, 0, &alice_program_id),
            false,
            need_account_data_checked(&alice_program_id, &alice_program_id, false),
        );

        assert_eq!(
            verify_account_changes(&alice_program_id, &pre, &post,),
            Err(InstructionError::ReadonlyLamportChange),
            "debit should fail, even if owning program"
        );

        let pre = PreInstructionAccount::new(
            &Account::new(42, 0, &alice_program_id),
            true,
            need_account_data_checked(&alice_program_id, &system_program::id(), true),
        );
        let post = Account::new(0, 0, &system_program::id());
        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Err(InstructionError::ModifiedProgramId),
            "system program can't debit the account unless it was the pre.owner"
        );

        let pre = PreInstructionAccount::new(
            &Account::new(42, 0, &system_program::id()),
            true,
            need_account_data_checked(&system_program::id(), &system_program::id(), true),
        );
        let post = Account::new(0, 0, &alice_program_id);
        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Ok(()),
            "system can spend (and change owner)"
        );
    }

    #[test]
    fn test_verify_account_changes_data_size_changed() {
        let alice_program_id = Pubkey::new_rand();
        let pre = PreInstructionAccount::new(
            &Account::new_data(42, &[0], &alice_program_id).unwrap(),
            true,
            need_account_data_checked(&alice_program_id, &system_program::id(), true),
        );
        let post = Account::new_data(42, &[0, 0], &alice_program_id).unwrap();
        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
            Err(InstructionError::AccountDataSizeChanged),
            "system program should not be able to change another program's account data size"
        );
        let pre = PreInstructionAccount::new(
            &Account::new_data(42, &[0], &alice_program_id).unwrap(),
            true,
            need_account_data_checked(&alice_program_id, &alice_program_id, true),
        );
        assert_eq!(
            verify_account_changes(&alice_program_id, &pre, &post),
            Err(InstructionError::AccountDataSizeChanged),
            "non-system programs cannot change their data size"
        );
        let pre = PreInstructionAccount::new(
            &Account::new_data(42, &[0], &system_program::id()).unwrap(),
            true,
            need_account_data_checked(&system_program::id(), &system_program::id(), true),
        );
        assert_eq!(
            verify_account_changes(&system_program::id(), &pre, &post),
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
        let mut message_processor = MessageProcessor::default();
        message_processor
            .add_instruction_processor(mock_system_program_id, mock_system_process_instruction);

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
        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::Correct,
            account_metas.clone(),
        )]);

        let result = message_processor.process_message(&message, &loaders, &accounts);
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].borrow().lamports, 100);
        assert_eq!(accounts[1].borrow().lamports, 0);

        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::AttemptCredit { lamports: 50 },
            account_metas.clone(),
        )]);

        let result = message_processor.process_message(&message, &loaders, &accounts);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ReadonlyLamportChange
            ))
        );

        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::AttemptDataChange { data: 50 },
            account_metas,
        )]);

        let result = message_processor.process_message(&message, &loaders, &accounts);
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
        let mut message_processor = MessageProcessor::default();
        message_processor
            .add_instruction_processor(mock_program_id, mock_system_process_instruction);

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
        let dup_pubkey = from_pubkey.clone();
        let account_metas = vec![
            AccountMeta::new(from_pubkey, true),
            AccountMeta::new(to_pubkey, false),
            AccountMeta::new(dup_pubkey, false),
        ];

        // Try to borrow mut the same account
        let message = Message::new(vec![Instruction::new(
            mock_program_id,
            &MockSystemInstruction::BorrowFail,
            account_metas.clone(),
        )]);
        let result = message_processor.process_message(&message, &loaders, &accounts);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::AccountBorrowFailed
            ))
        );

        // Try to borrow mut the same account in a safe way
        let message = Message::new(vec![Instruction::new(
            mock_program_id,
            &MockSystemInstruction::MultiBorrowMut,
            account_metas.clone(),
        )]);
        let result = message_processor.process_message(&message, &loaders, &accounts);
        assert_eq!(result, Ok(()));

        // Do work on the same account but at different location in keyed_accounts[]
        let message = Message::new(vec![Instruction::new(
            mock_program_id,
            &MockSystemInstruction::DoWork {
                lamports: 10,
                data: 42,
            },
            account_metas,
        )]);
        let result = message_processor.process_message(&message, &loaders, &accounts);
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].borrow().lamports, 80);
        assert_eq!(accounts[1].borrow().lamports, 20);
        assert_eq!(accounts[0].borrow().data, vec![42]);
    }
}
