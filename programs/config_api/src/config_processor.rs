//! Config program

use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    if keyed_accounts[1].signer_key().is_none() {
        error!("account[1] should sign the transaction");
        Err(InstructionError::MissingRequiredSignature)?;
    }

    if keyed_accounts[1].account.data.len() < data.len() {
        error!("instruction data too large");
        Err(InstructionError::InvalidInstructionData)?;
    }

    keyed_accounts[1].account.data[0..data.len()].copy_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{id, ConfigInstruction, ConfigState};
    use bincode::{deserialize, serialized_size};
    use serde_derive::{Deserialize, Serialize};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::script::Script;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;

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

    fn create_config_client(bank: &Bank, mint_keypair: Keypair) -> (BankClient, Pubkey, Pubkey) {
        let config_client =
            BankClient::new_with_keypairs(&bank, vec![Keypair::new(), Keypair::new()]);

        let from_pubkey = config_client.pubkeys()[0];
        let config_pubkey = config_client.pubkeys()[1];

        let mint_client = BankClient::new(&bank, mint_keypair);
        mint_client
            .process_instruction(SystemInstruction::new_move(
                &mint_client.pubkey(),
                &from_pubkey,
                42,
            ))
            .expect("new_move");

        mint_client
            .process_instruction(ConfigInstruction::new_account::<MyConfig>(
                &mint_client.pubkey(),
                &config_pubkey,
                1,
            ))
            .expect("new_account");

        (config_client, from_pubkey, config_pubkey)
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let (config_client, _, _) = create_config_client(&bank, from_keypair);
        let config_account = bank.get_account(&config_client.pubkeys()[1]).unwrap();
        assert_eq!(id(), config_account.owner);
        assert_eq!(
            MyConfig::default(),
            MyConfig::deserialize(&config_account.data).unwrap()
        );
    }

    #[test]
    fn test_process_store_ok() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (config_client, from_pubkey, config_pubkey) = create_config_client(&bank, mint_keypair);

        let my_config = MyConfig::new(42);
        let instruction = ConfigInstruction::new_store(&from_pubkey, &config_pubkey, &my_config);
        config_client.process_instruction(instruction).unwrap();

        let config_account = bank.get_account(&config_pubkey).unwrap();
        assert_eq!(
            my_config,
            MyConfig::deserialize(&config_account.data).unwrap()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (config_client, from_pubkey, config_pubkey) = create_config_client(&bank, mint_keypair);

        let my_config = MyConfig::new(42);
        let instruction = ConfigInstruction::new_store(&from_pubkey, &config_pubkey, &my_config);

        // Replace instruction data with a vector that's too large
        let script = Script::new(vec![instruction]);
        let mut transaction = script.compile();
        transaction.instructions[0].data = vec![0; 123];
        config_client.process_transaction(transaction).unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account1_not_signer() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let system_keypair = Keypair::new();
        let system_pubkey = system_keypair.pubkey();
        bank.transfer(42, &mint_keypair, &system_pubkey, bank.last_blockhash())
            .unwrap();
        let (_config_client, from_pubkey, config_pubkey) =
            create_config_client(&bank, mint_keypair);

        let move_instruction = SystemInstruction::new_move(&system_pubkey, &Pubkey::default(), 42);
        let my_config = MyConfig::new(42);
        let store_instruction =
            ConfigInstruction::new_store(&from_pubkey, &config_pubkey, &my_config);

        // Don't sign the transaction with `config_client`
        let script = Script::new(vec![move_instruction, store_instruction]);
        let mut transaction = script.compile();
        transaction.sign_unchecked(&[&system_keypair], bank.last_blockhash());
        let system_client = BankClient::new(&bank, system_keypair);
        system_client.process_transaction(transaction).unwrap_err();
    }
}
