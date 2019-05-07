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
    use crate::{config_instruction, id, ConfigState};
    use bincode::{deserialize, serialized_size};
    use serde_derive::{Deserialize, Serialize};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;

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
        let (genesis_block, mint_keypair) = create_genesis_block(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_config_client(bank: Bank, mint_keypair: Keypair) -> (BankClient, Keypair, Keypair) {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let config_keypair = Keypair::new();
        let config_pubkey = config_keypair.pubkey();

        let bank_client = BankClient::new(bank);
        bank_client
            .transfer(42, &mint_keypair, &from_pubkey)
            .unwrap();

        bank_client
            .send_instruction(
                &mint_keypair,
                config_instruction::create_account::<MyConfig>(
                    &mint_keypair.pubkey(),
                    &config_pubkey,
                    1,
                ),
            )
            .expect("new_account");

        (bank_client, from_keypair, config_keypair)
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let (bank, from_keypair) = create_bank(10_000);
        let (bank_client, _, config_keypair) = create_config_client(bank, from_keypair);
        let config_account_data = bank_client
            .get_account_data(&config_keypair.pubkey())
            .unwrap()
            .unwrap();
        assert_eq!(
            MyConfig::default(),
            MyConfig::deserialize(&config_account_data).unwrap()
        );
    }

    #[test]
    fn test_process_store_ok() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (bank_client, from_keypair, config_keypair) = create_config_client(bank, mint_keypair);
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);
        let instruction =
            config_instruction::store(&from_keypair.pubkey(), &config_pubkey, &my_config);
        let message = Message::new(vec![instruction]);
        bank_client
            .send_message(&[&from_keypair, &config_keypair], message)
            .unwrap();

        let config_account_data = bank_client
            .get_account_data(&config_pubkey)
            .unwrap()
            .unwrap();
        assert_eq!(
            my_config,
            MyConfig::deserialize(&config_account_data).unwrap()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (bank_client, from_keypair, config_keypair) = create_config_client(bank, mint_keypair);

        let my_config = MyConfig::new(42);

        // Replace instruction data with a vector that's too large
        let mut instruction =
            config_instruction::store(&from_keypair.pubkey(), &config_keypair.pubkey(), &my_config);
        instruction.data = vec![0; 123];

        let message = Message::new(vec![instruction]);
        bank_client
            .send_message(&[&from_keypair, &config_keypair], message)
            .unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account1_not_signer() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let system_keypair = Keypair::new();
        let system_pubkey = system_keypair.pubkey();
        bank.transfer(42, &mint_keypair, &system_pubkey).unwrap();
        let (bank_client, from_keypair, config_keypair) = create_config_client(bank, mint_keypair);

        let transfer_instruction =
            system_instruction::transfer(&system_pubkey, &Pubkey::default(), 42);
        let my_config = MyConfig::new(42);
        let mut store_instruction =
            config_instruction::store(&from_keypair.pubkey(), &config_keypair.pubkey(), &my_config);
        store_instruction.accounts[0].is_signer = false;
        store_instruction.accounts[1].is_signer = false;

        // Don't sign the transaction with `config_client`
        let message = Message::new(vec![transfer_instruction, store_instruction]);
        bank_client
            .send_message(&[&system_keypair], message)
            .unwrap_err();
    }
}
