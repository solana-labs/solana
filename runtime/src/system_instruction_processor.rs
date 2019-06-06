use log::*;
use solana_sdk::account_api::AccountApi;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::{SystemError, SystemInstruction};
use solana_sdk::system_program;

const FROM_ACCOUNT_INDEX: usize = 0;
const TO_ACCOUNT_INDEX: usize = 1;

fn create_system_account(
    keyed_accounts: &mut [&mut AccountApi],
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Result<(), InstructionError> {
    let e = if !system_program::check_id(&keyed_accounts[FROM_ACCOUNT_INDEX].owner()) {
        debug!("CreateAccount: invalid account[from] owner");
        Some(SystemError::SourceNotSystemAccount)
    } else if !keyed_accounts[TO_ACCOUNT_INDEX].get_data().is_empty()
        || !system_program::check_id(&keyed_accounts[TO_ACCOUNT_INDEX].owner())
    {
        debug!(
            "CreateAccount: invalid argument; account {} already in use",
            keyed_accounts[TO_ACCOUNT_INDEX].unsigned_key()
        );
        Some(SystemError::AccountAlreadyInUse)
    } else if lamports > keyed_accounts[FROM_ACCOUNT_INDEX].lamports() {
        debug!(
            "CreateAccount: insufficient lamports ({}, need {})",
            keyed_accounts[FROM_ACCOUNT_INDEX].lamports(),
            lamports
        );
        Some(SystemError::ResultWithNegativeLamports)
    } else {
        None
    };
    if e.is_some() {
        Err(InstructionError::CustomError(e.unwrap() as u32))?;
    }
    keyed_accounts[FROM_ACCOUNT_INDEX].debit(lamports)?;
    keyed_accounts[TO_ACCOUNT_INDEX].credit(lamports)?;
    keyed_accounts[TO_ACCOUNT_INDEX].set_owner(program_id)?;
    keyed_accounts[TO_ACCOUNT_INDEX].initialize_data(space)?;
    keyed_accounts[TO_ACCOUNT_INDEX].set_executable(false)?;
    Ok(())
}

fn assign_account_to_program(
    keyed_accounts: &mut [&mut AccountApi],
    program_id: &Pubkey,
) -> Result<(), InstructionError> {
    keyed_accounts[FROM_ACCOUNT_INDEX].set_owner(program_id)?;
    Ok(())
}
fn transfer_lamports(
    keyed_accounts: &mut [&mut AccountApi],
    lamports: u64,
) -> Result<(), InstructionError> {
    if lamports > keyed_accounts[FROM_ACCOUNT_INDEX].lamports() {
        debug!(
            "Transfer: insufficient lamports ({}, need {})",
            keyed_accounts[FROM_ACCOUNT_INDEX].lamports(),
            lamports
        );
        let e = SystemError::ResultWithNegativeLamports;
        Err(InstructionError::CustomError(e as u32))?;
    }
    keyed_accounts[FROM_ACCOUNT_INDEX].debit(lamports)?;
    keyed_accounts[TO_ACCOUNT_INDEX].credit(lamports)?;
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [&mut AccountApi],
    data: &[u8],
) -> Result<(), InstructionError> {
    if let Ok(instruction) = bincode::deserialize(data) {
        trace!("process_instruction: {:?}", instruction);
        trace!("keyed_accounts: {:?}", keyed_accounts);

        // All system instructions require that accounts_keys[0] be a signer
        if keyed_accounts[FROM_ACCOUNT_INDEX].signer_key().is_none() {
            debug!("account[from] is unsigned");
            Err(InstructionError::MissingRequiredSignature)?;
        }

        match instruction {
            SystemInstruction::CreateAccount {
                lamports,
                space,
                program_id,
            } => create_system_account(keyed_accounts, lamports, space, &program_id),
            SystemInstruction::Assign { program_id } => {
                if !system_program::check_id(&keyed_accounts[FROM_ACCOUNT_INDEX].owner()) {
                    Err(InstructionError::IncorrectProgramId)?;
                }
                assign_account_to_program(keyed_accounts, &program_id)
            }
            SystemInstruction::Transfer { lamports } => transfer_lamports(keyed_accounts, lamports),
        }
    } else {
        debug!("Invalid instruction data: {:?}", data);
        Err(InstructionError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Bank;
    use crate::bank_client::BankClient;
    use bincode::serialize;
    use solana_sdk::client::SyncClient;
    use solana_sdk::credit_debit_account::{CreditDebitAccount, KeyedCreditDebitAccount};
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_program;
    use solana_sdk::transaction::TransactionError;

    #[test]
    fn test_create_system_account() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &system_program::id());

        let to = Pubkey::new_rand();
        let mut to_account = CreditDebitAccount::new(0, 0, &Pubkey::default());

        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&to, false, &mut to_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        create_system_account(&mut keyed_accounts, 50, 2, &new_program_owner).unwrap();
        let from_lamports = from_account.lamports;
        let to_lamports = to_account.lamports;
        let to_owner = to_account.owner;
        let to_data = to_account.data.clone();
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 50);
        assert_eq!(to_owner, new_program_owner);
        assert_eq!(to_data, [0, 0]);
    }

    #[test]
    fn test_create_negative_lamports() {
        // Attempt to create account with more lamports than remaining in from_account
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &system_program::id());

        let to = Pubkey::new_rand();
        let mut to_account = CreditDebitAccount::new(0, 0, &Pubkey::default());
        let unchanged_account = to_account.clone();

        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&to, false, &mut to_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        let result = create_system_account(&mut keyed_accounts, 150, 2, &new_program_owner);
        assert_eq!(
            result,
            Err(InstructionError::CustomError(
                SystemError::ResultWithNegativeLamports as u32
            ))
        );
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(to_account, unchanged_account);
    }

    #[test]
    fn test_create_already_owned() {
        // Attempt to create system account in account already owned by another program
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &system_program::id());

        let original_program_owner = Pubkey::new(&[5; 32]);
        let owned_key = Pubkey::new_rand();
        let mut owned_account = CreditDebitAccount::new(0, 0, &original_program_owner);
        let unchanged_account = owned_account.clone();

        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&owned_key, false, &mut owned_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        let result = create_system_account(&mut keyed_accounts, 50, 2, &new_program_owner);
        assert_eq!(
            result,
            Err(InstructionError::CustomError(
                SystemError::AccountAlreadyInUse as u32
            ))
        );
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(owned_account, unchanged_account);
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &system_program::id());

        let populated_key = Pubkey::new_rand();
        let mut populated_account = CreditDebitAccount {
            lamports: 0,
            data: vec![0, 1, 2, 3],
            owner: Pubkey::default(),
            executable: false,
        };
        let unchanged_account = populated_account.clone();

        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&populated_key, false, &mut populated_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        let result = create_system_account(&mut keyed_accounts, 50, 2, &new_program_owner);
        assert_eq!(
            result,
            Err(InstructionError::CustomError(
                SystemError::AccountAlreadyInUse as u32
            ))
        );
        assert_eq!(from_account.lamports, 100);
        assert_eq!(populated_account, unchanged_account);
    }

    #[test]
    fn test_create_not_system_account() {
        let other_program = Pubkey::new(&[9; 32]);

        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &other_program);
        let to = Pubkey::new_rand();
        let mut to_account = CreditDebitAccount::new(0, 0, &Pubkey::default());
        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&to, false, &mut to_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        let result = create_system_account(&mut keyed_accounts, 50, 2, &other_program);
        assert_eq!(
            result,
            Err(InstructionError::CustomError(
                SystemError::SourceNotSystemAccount as u32
            ))
        );
    }

    #[test]
    fn test_assign_account_to_program() {
        let new_program_owner = Pubkey::new(&[9; 32]);

        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &system_program::id());
        let mut keyed_account = KeyedCreditDebitAccount::new(&from, true, &mut from_account);
        let mut keyed_accounts: Vec<&mut AccountApi> = vec![&mut keyed_account];
        assign_account_to_program(&mut keyed_accounts, &new_program_owner).unwrap();
        let from_owner = keyed_accounts[0].owner();
        assert_eq!(from_owner, &new_program_owner);

        // Attempt to assign account not owned by system program
        let another_program_owner = Pubkey::new(&[8; 32]);
        let instruction = SystemInstruction::Assign {
            program_id: another_program_owner,
        };
        let data = serialize(&instruction).unwrap();
        let result = process_instruction(&system_program::id(), &mut keyed_accounts, &data);
        assert_eq!(result, Err(InstructionError::IncorrectProgramId));
        assert_eq!(from_account.owner, new_program_owner);
    }

    #[test]
    fn test_transfer_lamports() {
        let from = Pubkey::new_rand();
        let mut from_account = CreditDebitAccount::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new_rand();
        let mut to_account = CreditDebitAccount::new(1, 0, &Pubkey::new(&[3; 32])); // account owner should not matter
        let mut keyed_accounts = [
            KeyedCreditDebitAccount::new(&from, true, &mut from_account),
            KeyedCreditDebitAccount::new(&to, false, &mut to_account),
        ];
        let mut keyed_accounts: Vec<&mut AccountApi> = keyed_accounts
            .iter_mut()
            .map(|account| account as &mut AccountApi)
            .collect();
        transfer_lamports(&mut keyed_accounts, 50).unwrap();
        let from_lamports = keyed_accounts[0].lamports();
        let to_lamports = keyed_accounts[1].lamports();
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 51);

        let result = transfer_lamports(&mut keyed_accounts, 100);
        assert_eq!(
            result,
            Err(InstructionError::CustomError(
                SystemError::ResultWithNegativeLamports as u32
            ))
        );
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 51);
    }

    #[test]
    fn test_system_unsigned_transaction() {
        let (genesis_block, alice_keypair) = create_genesis_block(100);
        let alice_pubkey = alice_keypair.pubkey();
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();

        // Fund to account to bypass AccountNotFound error
        let bank = Bank::new(&genesis_block);
        let bank_client = BankClient::new(bank);
        bank_client
            .transfer(50, &alice_keypair, &mallory_pubkey)
            .unwrap();

        // Erroneously sign transaction with recipient account key
        // No signature case is tested by bank `test_zero_signatures()`
        let account_metas = vec![
            AccountMeta::new(alice_pubkey, false),
            AccountMeta::new(mallory_pubkey, true),
        ];
        let malicious_instruction = Instruction::new(
            system_program::id(),
            &SystemInstruction::Transfer { lamports: 10 },
            account_metas,
        );
        assert_eq!(
            bank_client
                .send_instruction(&mallory_keypair, malicious_instruction)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 50);
        assert_eq!(bank_client.get_balance(&mallory_pubkey).unwrap(), 50);
    }
}
