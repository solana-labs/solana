use bincode::serialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::{SystemError, SystemInstruction};
use solana_sdk::system_program;

const FROM_ACCOUNT_INDEX: usize = 0;
const TO_ACCOUNT_INDEX: usize = 1;

fn create_system_account(
    keyed_accounts: &mut [KeyedAccount],
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Result<(), SystemError> {
    if !system_program::check_id(&keyed_accounts[FROM_ACCOUNT_INDEX].account.owner) {
        info!("CreateAccount: invalid account[from] owner");
        Err(SystemError::SourceNotSystemAccount)?;
    }

    if !keyed_accounts[TO_ACCOUNT_INDEX].account.data.is_empty()
        || !system_program::check_id(&keyed_accounts[TO_ACCOUNT_INDEX].account.owner)
    {
        info!(
            "CreateAccount: invalid argument; account {} already in use",
            keyed_accounts[TO_ACCOUNT_INDEX].unsigned_key()
        );
        Err(SystemError::AccountAlreadyInUse)?;
    }
    if lamports > keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports {
        info!(
            "CreateAccount: insufficient lamports ({}, need {})",
            keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports, lamports
        );
        Err(SystemError::ResultWithNegativeLamports)?;
    }
    keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports -= lamports;
    keyed_accounts[TO_ACCOUNT_INDEX].account.lamports += lamports;
    keyed_accounts[TO_ACCOUNT_INDEX].account.owner = *program_id;
    keyed_accounts[TO_ACCOUNT_INDEX].account.data = vec![0; space as usize];
    keyed_accounts[TO_ACCOUNT_INDEX].account.executable = false;
    Ok(())
}

fn assign_account_to_program(
    keyed_accounts: &mut [KeyedAccount],
    program_id: &Pubkey,
) -> Result<(), SystemError> {
    keyed_accounts[FROM_ACCOUNT_INDEX].account.owner = *program_id;
    Ok(())
}
fn move_lamports(keyed_accounts: &mut [KeyedAccount], lamports: u64) -> Result<(), SystemError> {
    if lamports > keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports {
        info!(
            "Move: insufficient lamports ({}, need {})",
            keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports, lamports
        );
        Err(SystemError::ResultWithNegativeLamports)?;
    }
    keyed_accounts[FROM_ACCOUNT_INDEX].account.lamports -= lamports;
    keyed_accounts[TO_ACCOUNT_INDEX].account.lamports += lamports;
    Ok(())
}

pub fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    if let Ok(instruction) = bincode::deserialize(data) {
        trace!("process_instruction: {:?}", instruction);
        trace!("keyed_accounts: {:?}", keyed_accounts);

        // All system instructions require that accounts_keys[0] be a signer
        if keyed_accounts[FROM_ACCOUNT_INDEX].signer_key().is_none() {
            info!("account[from] is unsigned");
            Err(ProgramError::MissingRequiredSignature)?;
        }

        match instruction {
            SystemInstruction::CreateAccount {
                lamports,
                space,
                program_id,
            } => create_system_account(keyed_accounts, lamports, space, &program_id),
            SystemInstruction::Assign { program_id } => {
                if !system_program::check_id(&keyed_accounts[FROM_ACCOUNT_INDEX].account.owner) {
                    Err(ProgramError::IncorrectProgramId)?;
                }
                assign_account_to_program(keyed_accounts, &program_id)
            }
            SystemInstruction::Move { lamports } => move_lamports(keyed_accounts, lamports),
        }
        .map_err(|e| ProgramError::CustomError(serialize(&e).unwrap()))
    } else {
        info!("Invalid instruction data: {:?}", data);
        Err(ProgramError::InvalidInstructionData)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Bank;
    use crate::bank_client::BankClient;
    use solana_sdk::account::Account;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::script::Script;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::system_program;
    use solana_sdk::transaction::{Instruction, InstructionError, TransactionError};

    #[test]
    fn test_create_system_account() {
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let to = Keypair::new().pubkey();
        let mut to_account = Account::new(0, 0, &Pubkey::default());

        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&to, false, &mut to_account),
        ];
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
        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let to = Keypair::new().pubkey();
        let mut to_account = Account::new(0, 0, &Pubkey::default());
        let unchanged_account = to_account.clone();

        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&to, false, &mut to_account),
        ];
        let result = create_system_account(&mut keyed_accounts, 150, 2, &new_program_owner);
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports));
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(to_account, unchanged_account);
    }

    #[test]
    fn test_create_already_owned() {
        // Attempt to create system account in account already owned by another program
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let original_program_owner = Pubkey::new(&[5; 32]);
        let owned_key = Keypair::new().pubkey();
        let mut owned_account = Account::new(0, 0, &original_program_owner);
        let unchanged_account = owned_account.clone();

        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&owned_key, false, &mut owned_account),
        ];
        let result = create_system_account(&mut keyed_accounts, 50, 2, &new_program_owner);
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse));
        let from_lamports = from_account.lamports;
        assert_eq!(from_lamports, 100);
        assert_eq!(owned_account, unchanged_account);
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_program_owner = Pubkey::new(&[9; 32]);
        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &system_program::id());

        let populated_key = Keypair::new().pubkey();
        let mut populated_account = Account {
            lamports: 0,
            data: vec![0, 1, 2, 3],
            owner: Pubkey::default(),
            executable: false,
        };
        let unchanged_account = populated_account.clone();

        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&populated_key, false, &mut populated_account),
        ];
        let result = create_system_account(&mut keyed_accounts, 50, 2, &new_program_owner);
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse));
        assert_eq!(from_account.lamports, 100);
        assert_eq!(populated_account, unchanged_account);
    }

    #[test]
    fn test_create_not_system_account() {
        let other_program = Pubkey::new(&[9; 32]);

        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &other_program);
        let to = Keypair::new().pubkey();
        let mut to_account = Account::new(0, 0, &Pubkey::default());
        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&to, false, &mut to_account),
        ];
        let result = create_system_account(&mut keyed_accounts, 50, 2, &other_program);
        assert_eq!(result, Err(SystemError::SourceNotSystemAccount));
    }

    #[test]
    fn test_assign_account_to_program() {
        let new_program_owner = Pubkey::new(&[9; 32]);

        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &system_program::id());
        let mut keyed_accounts = [KeyedAccount::new(&from, true, &mut from_account)];
        assign_account_to_program(&mut keyed_accounts, &new_program_owner).unwrap();
        let from_owner = from_account.owner;
        assert_eq!(from_owner, new_program_owner);

        // Attempt to assign account not owned by system program
        let another_program_owner = Pubkey::new(&[8; 32]);
        keyed_accounts = [KeyedAccount::new(&from, true, &mut from_account)];
        let instruction = SystemInstruction::Assign {
            program_id: another_program_owner,
        };
        let data = serialize(&instruction).unwrap();
        let result = entrypoint(&system_program::id(), &mut keyed_accounts, &data, 0);
        assert_eq!(result, Err(ProgramError::IncorrectProgramId));
        assert_eq!(from_account.owner, new_program_owner);
    }

    #[test]
    fn test_move_lamports() {
        let from = Keypair::new().pubkey();
        let mut from_account = Account::new(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Keypair::new().pubkey();
        let mut to_account = Account::new(1, 0, &Pubkey::new(&[3; 32])); // account owner should not matter
        let mut keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&to, false, &mut to_account),
        ];
        move_lamports(&mut keyed_accounts, 50).unwrap();
        let from_lamports = from_account.lamports;
        let to_lamports = to_account.lamports;
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 51);

        // Attempt to move more lamports than remaining in from_account
        keyed_accounts = [
            KeyedAccount::new(&from, true, &mut from_account),
            KeyedAccount::new(&to, false, &mut to_account),
        ];
        let result = move_lamports(&mut keyed_accounts, 100);
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports));
        assert_eq!(from_account.lamports, 50);
        assert_eq!(to_account.lamports, 51);
    }

    #[test]
    fn test_system_unsigned_transaction() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);

        let alice_client = BankClient::new(&bank, mint_keypair);
        let alice_pubkey = alice_client.pubkey();

        let mallory_client = BankClient::new(&bank, Keypair::new());
        let mallory_pubkey = mallory_client.pubkey();

        // Fund to account to bypass AccountNotFound error
        alice_client.transfer(50, &mallory_pubkey).unwrap();

        // Erroneously sign transaction with recipient account key
        // No signature case is tested by bank `test_zero_signatures()`
        let malicious_script = Script::new(vec![Instruction::new(
            system_program::id(),
            &SystemInstruction::Move { lamports: 10 },
            vec![(alice_pubkey, false), (mallory_pubkey, true)],
        )]);
        assert_eq!(
            mallory_client.process_script(malicious_script),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ProgramError(ProgramError::MissingRequiredSignature)
            ))
        );
        assert_eq!(bank.get_balance(&alice_pubkey), 50);
        assert_eq!(bank.get_balance(&mallory_pubkey), 50);
    }
}
