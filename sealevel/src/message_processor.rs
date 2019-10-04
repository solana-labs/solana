use crate::native_loader;
use crate::system_instruction_processor;
use serde::{Deserialize, Serialize};
use solana_sdk::account::{
    create_keyed_credit_only_accounts, Account, KeyedAccount, LamportCredit,
};
use solana_sdk::instruction::{CompiledInstruction, InstructionError};
use solana_sdk::instruction_processor_utils;
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;
use solana_sdk::transaction::TransactionError;
use std::collections::HashMap;
use std::io::Write;
use std::sync::RwLock;

#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;

/// Return true if the slice has any duplicate elements
pub fn has_duplicates<T: PartialEq>(xs: &[T]) -> bool {
    // Note: This is an O(n^2) algorithm, but requires no heap allocations. The benchmark
    // `bench_has_duplicates` in benches/message_processor.rs shows that this implementation is
    // ~50 times faster than using HashSet for very short slices.
    for i in 1..xs.len() {
        if xs[i..].contains(&xs[i - 1]) {
            return true;
        }
    }
    false
}

/// Get mut references to a subset of elements.
fn get_subset_unchecked_mut<'a, T>(
    xs: &'a mut [T],
    indexes: &[u8],
) -> Result<Vec<&'a mut T>, InstructionError> {
    // Since the compiler doesn't know the indexes are unique, dereferencing
    // multiple mut elements is assumed to be unsafe. If, however, all
    // indexes are unique, it's perfectly safe. The returned elements will share
    // the liftime of the input slice.

    // Make certain there are no duplicate indexes. If there are, return an error
    // because we can't return multiple mut references to the same element.
    if has_duplicates(indexes) {
        return Err(InstructionError::DuplicateAccountIndex);
    }

    Ok(indexes
        .iter()
        .map(|i| {
            let ptr = &mut xs[*i as usize] as *mut T;
            unsafe { &mut *ptr }
        })
        .collect())
}

fn verify_instruction(
    is_debitable: bool,
    program_id: &Pubkey,
    pre: &Account,
    post: &Account,
) -> Result<(), InstructionError> {
    // Verify the transaction

    // Make sure that program_id is still the same or this was just assigned by the system program,
    //  but even the system program can't touch a credit-only account
    if pre.owner != post.owner && (!is_debitable || !system_program::check_id(&program_id)) {
        return Err(InstructionError::ModifiedProgramId);
    }
    // For accounts unassigned to the program, the individual balance of each accounts cannot decrease.
    if *program_id != post.owner && pre.lamports > post.lamports {
        return Err(InstructionError::ExternalAccountLamportSpend);
    }
    // The balance of credit-only accounts may only increase
    if !is_debitable && pre.lamports > post.lamports {
        return Err(InstructionError::CreditOnlyLamportSpend);
    }
    // For accounts unassigned to the program, the data may not change.
    if *program_id != post.owner && !system_program::check_id(&program_id) && pre.data != post.data
    {
        return Err(InstructionError::ExternalAccountDataModified);
    }
    // Credit-only account data may not change.
    if !is_debitable && pre.data != post.data {
        return Err(InstructionError::CreditOnlyDataModified);
    }

    // executable is one-way (false->true) and
    //  only system or the account owner may modify
    if pre.executable != post.executable
        && (!is_debitable
            || pre.executable
            || *program_id != post.owner && !system_program::check_id(&program_id))
    {
        return Err(InstructionError::ExecutableModified);
    }

    // no one modifies rent_epoch (yet)
    if pre.rent_epoch != post.rent_epoch {
        return Err(InstructionError::RentEpochModified);
    }

    Ok(())
}

/// Return instruction data to pass to process_instruction().
/// When a loader is detected, the instruction data is wrapped with a LoaderInstruction
/// to signal to the loader that the instruction data should be used as arguments when
/// invoking a "main()" function.
fn get_loader_instruction_data<'a>(
    loaders: &[(Pubkey, Account)],
    ix_data: &'a [u8],
    loader_ix_data: &'a mut Vec<u8>,
) -> &'a [u8] {
    if loaders.len() > 1 {
        let ix = LoaderInstruction::InvokeMain {
            data: ix_data.to_vec(),
        };
        let ix_data = bincode::serialize(&ix).unwrap();
        loader_ix_data.write_all(&ix_data).unwrap();
        loader_ix_data
    } else {
        ix_data
    }
}

pub type ProcessInstruction =
    fn(&Pubkey, &mut [KeyedAccount], &[u8]) -> Result<(), InstructionError>;

pub type SymbolCache = RwLock<HashMap<Vec<u8>, Symbol<instruction_processor_utils::Entrypoint>>>;

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
        executable_accounts: &mut [(Pubkey, Account)],
        program_accounts: &mut [&mut Account],
    ) -> Result<(), InstructionError> {
        let program_id = instruction.program_id(&message.account_keys);

        let mut loader_ix_data = vec![];
        let ix_data = get_loader_instruction_data(
            executable_accounts,
            &instruction.data,
            &mut loader_ix_data,
        );

        let mut keyed_accounts = create_keyed_credit_only_accounts(executable_accounts);
        let mut keyed_accounts2: Vec<_> = instruction
            .accounts
            .iter()
            .map(|&index| {
                let index = index as usize;
                let key = &message.account_keys[index];
                let is_debitable = message.is_debitable(index);
                (
                    key,
                    index < message.header.num_required_signatures as usize,
                    is_debitable,
                )
            })
            .zip(program_accounts.iter_mut())
            .map(|((key, is_signer, is_debitable), account)| {
                if is_debitable {
                    KeyedAccount::new(key, is_signer, account)
                } else {
                    KeyedAccount::new_credit_only(key, is_signer, account)
                }
            })
            .collect();
        keyed_accounts.append(&mut keyed_accounts2);

        assert!(
            keyed_accounts[0].account.executable,
            "loader not executable"
        );

        let loader_id = keyed_accounts[0].unsigned_key();
        for (id, process_instruction) in &self.instruction_processors {
            if id == loader_id {
                return process_instruction(&program_id, &mut keyed_accounts[1..], &ix_data);
            }
        }

        native_loader::invoke_entrypoint(
            &program_id,
            &mut keyed_accounts,
            ix_data,
            &self.symbol_cache,
        )
    }

    /// Execute an instruction
    /// This method calls the instruction's program entrypoint method and verifies that the result of
    /// the call does not violate the bank's accounting rules.
    /// The accounts are committed back to the bank only if this function returns Ok(_).
    fn execute_instruction(
        &self,
        message: &Message,
        instruction: &CompiledInstruction,
        executable_accounts: &mut [(Pubkey, Account)],
        program_accounts: &mut [&mut Account],
        credits: &mut [&mut LamportCredit],
    ) -> Result<(), InstructionError> {
        let program_id = instruction.program_id(&message.account_keys);
        assert_eq!(instruction.accounts.len(), program_accounts.len());
        // TODO: the runtime should be checking read/write access to memory
        // we are trusting the hard-coded programs not to clobber or allocate
        let pre_total: u128 = program_accounts
            .iter()
            .map(|a| u128::from(a.lamports))
            .sum();
        #[allow(clippy::map_clone)]
        let pre_accounts: Vec<_> = program_accounts
            .iter_mut()
            .map(|account| account.clone()) // cloned() doesn't work on & &
            .collect();

        self.process_instruction(message, instruction, executable_accounts, program_accounts)?;
        // Verify the instruction
        for (pre_account, (i, post_account, is_debitable)) in
            pre_accounts
                .iter()
                .zip(program_accounts.iter().enumerate().map(|(i, account)| {
                    (
                        i,
                        account,
                        message.is_debitable(instruction.accounts[i] as usize),
                    )
                }))
        {
            verify_instruction(is_debitable, &program_id, pre_account, post_account)?;
            if !is_debitable {
                *credits[i] += post_account.lamports - pre_account.lamports;
            }
        }
        // The total sum of all the lamports in all the accounts cannot change.
        let post_total: u128 = program_accounts
            .iter()
            .map(|a| u128::from(a.lamports))
            .sum();

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
        loaders: &mut [Vec<(Pubkey, Account)>],
        accounts: &mut [Account],
        credits: &mut [LamportCredit],
    ) -> Result<(), TransactionError> {
        for (instruction_index, instruction) in message.instructions.iter().enumerate() {
            let executable_index = message
                .program_position(instruction.program_id_index as usize)
                .ok_or(TransactionError::InvalidAccountIndex)?;
            let executable_accounts = &mut loaders[executable_index];
            let mut program_accounts = get_subset_unchecked_mut(accounts, &instruction.accounts)
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
            // TODO: `get_subset_unchecked_mut` panics on an index out of bounds if an executable
            // account is also included as a regular account for an instruction, because the
            // executable account is not passed in as part of the accounts slice
            let mut instruction_credits = get_subset_unchecked_mut(credits, &instruction.accounts)
                .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
            self.execute_instruction(
                message,
                instruction,
                executable_accounts,
                &mut program_accounts,
                &mut instruction_credits,
            )
            .map_err(|err| TransactionError::InstructionError(instruction_index as u8, err))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
    use solana_sdk::message::Message;
    use solana_sdk::native_loader::create_loadable_account;

    #[test]
    fn test_has_duplicates() {
        assert!(!has_duplicates(&[1, 2]));
        assert!(has_duplicates(&[1, 2, 1]));
    }

    #[test]
    fn test_get_subset_unchecked_mut() {
        assert_eq!(
            get_subset_unchecked_mut(&mut [7, 8], &[0]).unwrap(),
            vec![&mut 7]
        );
        assert_eq!(
            get_subset_unchecked_mut(&mut [7, 8], &[0, 1]).unwrap(),
            vec![&mut 7, &mut 8]
        );
    }

    #[test]
    fn test_get_subset_unchecked_mut_duplicate_index() {
        // This panics, because it assumes duplicate detection is done elsewhere.
        assert_eq!(
            get_subset_unchecked_mut(&mut [7, 8], &[0, 0]).unwrap_err(),
            InstructionError::DuplicateAccountIndex
        );
    }

    #[test]
    #[should_panic]
    fn test_get_subset_unchecked_mut_out_of_bounds() {
        // This panics, because it assumes bounds validation is done elsewhere.
        get_subset_unchecked_mut(&mut [7, 8], &[2]).unwrap();
    }

    #[test]
    fn test_verify_instruction_change_program_id() {
        fn change_program_id(
            ix: &Pubkey,
            pre: &Pubkey,
            post: &Pubkey,
            is_debitable: bool,
        ) -> Result<(), InstructionError> {
            verify_instruction(
                is_debitable,
                &ix,
                &Account::new(0, 0, pre),
                &Account::new(0, 0, post),
            )
        }

        let system_program_id = system_program::id();
        let alice_program_id = Pubkey::new_rand();
        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            change_program_id(
                &system_program_id,
                &system_program_id,
                &alice_program_id,
                true
            ),
            Ok(()),
            "system program should be able to change the account owner"
        );
        assert_eq!(
            change_program_id(&system_program_id, &system_program_id, &alice_program_id, false),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a credit only account"
        );

        assert_eq!(
            change_program_id(
                &mallory_program_id,
                &system_program_id,
                &alice_program_id,
                true
            ),
            Err(InstructionError::ModifiedProgramId),
            "malicious Mallory should not be able to change the account owner"
        );
    }

    #[test]
    fn test_verify_instruction_change_executable() {
        let alice_program_id = Pubkey::new_rand();
        let change_executable = |program_id: &Pubkey,
                                 is_debitable: bool,
                                 pre_executable: bool,
                                 post_executable: bool|
         -> Result<(), InstructionError> {
            let pre = Account {
                owner: alice_program_id,
                executable: pre_executable,
                ..Account::default()
            };

            let post = Account {
                owner: alice_program_id,
                executable: post_executable,
                ..Account::default()
            };
            verify_instruction(is_debitable, &program_id, &pre, &post)
        };

        let mallory_program_id = Pubkey::new_rand();
        let system_program_id = system_program::id();

        assert_eq!(
            change_executable(&system_program_id, true, false, true),
            Ok(()),
            "system program should be able to change executable"
        );
        assert_eq!(
            change_executable(&alice_program_id, true, false, true),
            Ok(()),
            "alice program should be able to change executable"
        );
        assert_eq!(
            change_executable(&system_program_id, false, false, true),
            Err(InstructionError::ExecutableModified),
            "system program can't modify executable of credit-only accounts"
        );
        assert_eq!(
            change_executable(&system_program_id, true, true, false),
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
    fn test_verify_instruction_change_data() {
        let alice_program_id = Pubkey::new_rand();

        let change_data =
            |program_id: &Pubkey, is_debitable: bool| -> Result<(), InstructionError> {
                let pre = Account::new(0, 0, &alice_program_id);
                let post = Account::new_data(0, &[42], &alice_program_id).unwrap();
                verify_instruction(is_debitable, &program_id, &pre, &post)
            };

        let system_program_id = system_program::id();
        let mallory_program_id = Pubkey::new_rand();

        assert_eq!(
            change_data(&system_program_id, true),
            Ok(()),
            "system program should be able to change the data"
        );
        assert_eq!(
            change_data(&alice_program_id, true),
            Ok(()),
            "alice program should be able to change the data"
        );
        assert_eq!(
            change_data(&mallory_program_id, true),
            Err(InstructionError::ExternalAccountDataModified),
            "malicious Mallory should not be able to change the account data"
        );

        assert_eq!(
            change_data(&system_program_id, false),
            Err(InstructionError::CreditOnlyDataModified),
            "system program should not be able to change the data if credit-only"
        );
    }

    #[test]
    fn test_verify_instruction_rent_epoch() {
        let alice_program_id = Pubkey::new_rand();
        let pre = Account::new(0, 0, &alice_program_id);
        let mut post = Account::new(0, 0, &alice_program_id);

        assert_eq!(
            verify_instruction(false, &system_program::id(), &pre, &post),
            Ok(()),
            "nothing changed!"
        );

        post.rent_epoch += 1;
        assert_eq!(
            verify_instruction(false, &system_program::id(), &pre, &post),
            Err(InstructionError::RentEpochModified),
            "no one touches rent_epoch"
        );
    }

    #[test]
    fn test_verify_instruction_credit_only() {
        let alice_program_id = Pubkey::new_rand();
        let pre = Account::new(42, 0, &alice_program_id);
        let post = Account::new(0, 0, &alice_program_id);
        assert_eq!(
            verify_instruction(false, &system_program::id(), &pre, &post),
            Err(InstructionError::ExternalAccountLamportSpend),
            "debit should fail, even if system program"
        );
        assert_eq!(
            verify_instruction(false, &alice_program_id, &pre, &post,),
            Err(InstructionError::CreditOnlyLamportSpend),
            "debit should fail, even if owning program"
        );
    }

    #[test]
    fn test_process_message_credit_only_handling() {
        #[derive(Serialize, Deserialize)]
        enum MockSystemInstruction {
            Correct { lamports: u64 },
            AttemptDebit { lamports: u64 },
            Misbehave { lamports: u64 },
        }

        fn mock_system_process_instruction(
            _program_id: &Pubkey,
            keyed_accounts: &mut [KeyedAccount],
            data: &[u8],
        ) -> Result<(), InstructionError> {
            if let Ok(instruction) = bincode::deserialize(data) {
                match instruction {
                    MockSystemInstruction::Correct { lamports } => {
                        keyed_accounts[0].account.lamports -= lamports;
                        keyed_accounts[1].account.lamports += lamports;
                        Ok(())
                    }
                    MockSystemInstruction::AttemptDebit { lamports } => {
                        keyed_accounts[0].account.lamports += lamports;
                        keyed_accounts[1].account.lamports -= lamports;
                        Ok(())
                    }
                    // Credit a credit-only account for more lamports than debited
                    MockSystemInstruction::Misbehave { lamports } => {
                        keyed_accounts[0].account.lamports -= lamports;
                        keyed_accounts[1].account.lamports = 2 * lamports;
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

        let mut accounts: Vec<Account> = Vec::new();
        let account = Account::new(100, 1, &mock_system_program_id);
        accounts.push(account);
        let account = Account::new(0, 1, &mock_system_program_id);
        accounts.push(account);

        let mut loaders: Vec<Vec<(Pubkey, Account)>> = Vec::new();
        let account = create_loadable_account("mock_system_program");
        loaders.push(vec![(mock_system_program_id, account)]);

        let from_pubkey = Pubkey::new_rand();
        let to_pubkey = Pubkey::new_rand();
        let account_metas = vec![
            AccountMeta::new(from_pubkey, true),
            AccountMeta::new_credit_only(to_pubkey, false),
        ];
        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::Correct { lamports: 50 },
            account_metas.clone(),
        )]);
        let mut deltas = vec![0, 0];

        let result =
            message_processor.process_message(&message, &mut loaders, &mut accounts, &mut deltas);
        assert_eq!(result, Ok(()));
        assert_eq!(accounts[0].lamports, 50);
        assert_eq!(accounts[1].lamports, 50);
        assert_eq!(deltas, vec![0, 50]);

        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::AttemptDebit { lamports: 50 },
            account_metas.clone(),
        )]);
        let mut deltas = vec![0, 0];

        let result =
            message_processor.process_message(&message, &mut loaders, &mut accounts, &mut deltas);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::CreditOnlyLamportSpend
            ))
        );

        let message = Message::new(vec![Instruction::new(
            mock_system_program_id,
            &MockSystemInstruction::Misbehave { lamports: 50 },
            account_metas,
        )]);
        let mut deltas = vec![0, 0];

        let result =
            message_processor.process_message(&message, &mut loaders, &mut accounts, &mut deltas);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::UnbalancedInstruction
            ))
        );
    }

    #[test]
    fn test_get_loader_instruction_data() {
        // First ensure the ix_data is unaffected if not invoking via a loader.
        let ix_data = [1];
        let mut loader_ix_data = vec![];

        let native_pubkey = Pubkey::new_rand();
        let native_loader = (native_pubkey, Account::new(0, 0, &native_pubkey));
        assert_eq!(
            get_loader_instruction_data(&[native_loader.clone()], &ix_data, &mut loader_ix_data),
            &ix_data
        );

        // Now ensure the ix_data is wrapped when there's a loader present.
        let acme_pubkey = Pubkey::new_rand();
        let acme_loader = (acme_pubkey, Account::new(0, 0, &native_pubkey));
        let expected_ix = LoaderInstruction::InvokeMain {
            data: ix_data.to_vec(),
        };
        let expected_ix_data = bincode::serialize(&expected_ix).unwrap();
        assert_eq!(
            get_loader_instruction_data(
                &[native_loader.clone(), acme_loader.clone()],
                &ix_data,
                &mut loader_ix_data
            ),
            &expected_ix_data[..]
        );

        // Note there was an allocation in the input vector.
        assert_eq!(loader_ix_data, expected_ix_data);
    }
}
