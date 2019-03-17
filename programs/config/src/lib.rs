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
    _tick_height: u64,
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
    tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(program_id, keyed_accounts, data, tick_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialized_size};
    use serde_derive::{Deserialize, Serialize};
    use solana_config_api::{id, ConfigInstruction, ConfigState};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::transaction::{Instruction, Transaction};

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

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_account_instruction(from_pubkey: &Pubkey, config_pubkey: &Pubkey) -> Instruction {
        ConfigInstruction::new_account::<MyConfig>(&from_pubkey, &config_pubkey, 1)
    }

    fn create_config_client(bank: &Bank, from_keypair: Keypair) -> BankClient {
        let from_client = BankClient::new(&bank, from_keypair);
        let from_pubkey = from_client.pubkey();
        let config_client = BankClient::new(&bank, Keypair::new());
        let config_pubkey = config_client.pubkey();

        let instruction = create_account_instruction(&from_pubkey, &config_pubkey);
        from_client.process_script(vec![instruction]).unwrap();

        config_client
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let config_client = create_config_client(&bank, from_keypair);
        let config_account = bank.get_account(&config_client.pubkey()).unwrap();
        assert_eq!(id(), config_account.owner);
        assert_eq!(
            MyConfig::default(),
            MyConfig::deserialize(&config_account.data).unwrap()
        );
    }

    #[test]
    fn test_process_store_ok() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let config_client = create_config_client(&bank, from_keypair);
        let config_pubkey = config_client.pubkey();

        let my_config = MyConfig::new(42);
        let instruction = ConfigInstruction::new_store(&config_pubkey, &my_config);
        config_client.process_script(vec![instruction]).unwrap();

        let config_account = bank.get_account(&config_pubkey).unwrap();
        assert_eq!(
            my_config,
            MyConfig::deserialize(&config_account.data).unwrap()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let config_client = create_config_client(&bank, from_keypair);
        let config_pubkey = config_client.pubkey();

        let my_config = MyConfig::new(42);
        let instruction = ConfigInstruction::new_store(&config_pubkey, &my_config);

        // Replace instruction data with a vector that's too large
        let mut transaction = Transaction::new(vec![instruction]);
        transaction.instructions[0].data = vec![0; 123];
        config_client.process_transaction(transaction).unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account0_not_signer() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let system_keypair = Keypair::new();
        let system_pubkey = system_keypair.pubkey();
        bank.transfer(42, &from_keypair, &system_pubkey, bank.last_blockhash())
            .unwrap();
        let config_client = create_config_client(&bank, from_keypair);
        let config_pubkey = config_client.pubkey();

        let move_instruction = SystemInstruction::new_move(&system_pubkey, &Pubkey::default(), 42);
        let my_config = MyConfig::new(42);
        let store_instruction = ConfigInstruction::new_store(&config_pubkey, &my_config);

        // Don't sign the transaction with `config_client`
        let mut transaction = Transaction::new(vec![move_instruction, store_instruction]);
        transaction.sign_unchecked(&[&system_keypair], bank.last_blockhash());
        let system_client = BankClient::new(&bank, system_keypair);
        system_client.process_transaction(transaction).unwrap_err();
    }
}
