//! Config program

use log::*;
use solana_config_api::check_id;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;

fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), ProgramError> {
    if !check_id(&keyed_accounts[0].account.owner) {
        error!("account[0] is not assigned to the config program");
        Err(ProgramError::IncorrectProgramId)?;
    }

    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] should sign the transaction");
        Err(ProgramError::MissingRequiredSignature)?;
    }

    if keyed_accounts[0].account.data.len() < data.len() {
        error!("instruction data too large");
        Err(ProgramError::InvalidInstructionData)?;
    }

    keyed_accounts[0].account.data[0..data.len()].copy_from_slice(data);
    Ok(())
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(program_id, keyed_accounts, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialized_size};
    use serde_derive::{Deserialize, Serialize};
    use solana_config_api::{id, ConfigInstruction, ConfigState, ConfigTransaction};
    use solana_runtime::runtime;
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::system_program;
    use solana_sdk::transaction::Transaction;

    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    struct MyConfig {
        pub item: u64,
    }
    impl MyConfig {
        pub fn new(item: u64) -> Self {
            Self { item }
        }
        pub fn deserialize(input: &[u8]) -> Option<Self> {
            deserialize(input).ok()
        }
    }

    impl ConfigState for MyConfig {
        fn max_space() -> u64 {
            serialized_size(&Self::default()).unwrap()
        }
    }

    fn create_config_account() -> Account {
        Account::new(1, MyConfig::max_space() as usize, &id())
    }

    fn process_transaction(
        tx: &Transaction,
        tx_accounts: &mut Vec<Account>,
    ) -> Result<(), ProgramError> {
        runtime::process_transaction(tx, tx_accounts, process_instruction)
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let from_account_keypair = Keypair::new();
        let from_account = Account::new(1, 0, &system_program::id());

        let config_account_keypair = Keypair::new();
        let config_account = Account::new(0, 0, &system_program::id());

        let transaction = ConfigTransaction::new_account::<MyConfig>(
            &from_account_keypair,
            &config_account_keypair.pubkey(),
            Hash::default(),
            1,
            0,
        );
        let mut accounts = vec![from_account, config_account];
        process_transaction(&transaction, &mut accounts).unwrap();

        assert_eq!(id(), accounts[1].owner);
        assert_eq!(
            MyConfig::default(),
            MyConfig::deserialize(&accounts[1].data).unwrap()
        );
    }

    #[test]
    fn test_process_store_ok() {
        solana_logger::setup();
        let config_account_keypair = Keypair::new();
        let config_account = create_config_account();

        let new_config_state = MyConfig::new(42);

        let transaction = ConfigTransaction::new_store(
            &config_account_keypair,
            &new_config_state,
            Hash::default(),
            0,
        );

        let mut accounts = vec![config_account];
        process_transaction(&transaction, &mut accounts).unwrap();

        assert_eq!(
            new_config_state,
            MyConfig::deserialize(&accounts[0].data).unwrap()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let config_account_keypair = Keypair::new();
        let config_account = create_config_account();

        let new_config_state = MyConfig::new(42);

        let mut transaction = ConfigTransaction::new_store(
            &config_account_keypair,
            &new_config_state,
            Hash::default(),
            0,
        );

        // Replace instruction data with a vector that's too large
        transaction.instructions[0].data = vec![0; 123];

        let mut accounts = vec![config_account];
        process_transaction(&transaction, &mut accounts).unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account0_invalid_owner() {
        solana_logger::setup();
        let config_account_keypair = Keypair::new();
        let mut config_account = create_config_account();
        config_account.owner = Pubkey::default(); // <-- Invalid owner

        let new_config_state = MyConfig::new(42);

        let transaction = ConfigTransaction::new_store(
            &config_account_keypair,
            &new_config_state,
            Hash::default(),
            0,
        );
        let mut accounts = vec![config_account];
        process_transaction(&transaction, &mut accounts).unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account0_not_signer() {
        solana_logger::setup();
        let system_account_keypair = Keypair::new();
        let system_account = Account::new(42, 0, &system_program::id());

        let config_account_keypair = Keypair::new();
        let config_account = create_config_account();

        let mut transaction = Transaction::new(vec![
            SystemInstruction::new_move(&system_account_keypair.pubkey(), &Pubkey::default(), 42),
            ConfigInstruction::new_store(&config_account_keypair.pubkey(), &MyConfig::new(42)),
        ]);

        // Don't sign the transaction with `config_account_keypair`
        transaction.sign_unchecked(&[&system_account_keypair], Hash::default());
        let mut accounts = vec![system_account, config_account];
        process_transaction(&transaction, &mut accounts).unwrap_err();
    }
}
