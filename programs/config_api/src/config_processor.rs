//! Config program

use crate::config_instruction::ConfigKeys;
use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let key_list: ConfigKeys = deserialize(data).map_err(|err| {
        error!("Invalid ConfigKeys data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    let current_data: ConfigKeys = deserialize(&keyed_accounts[0].account.data).map_err(|err| {
        error!("Invalid data in account[0]: {:?} {:?}", data, err);
        InstructionError::InvalidAccountData
    })?;
    let current_signer_keys: Vec<Pubkey> = current_data
        .keys
        .iter()
        .filter(|(_, is_signer)| *is_signer)
        .map(|(pubkey, _)| *pubkey)
        .collect();

    if current_signer_keys.is_empty() {
        // Config account keypair must be a signer on account initilization,
        // or when no signers specified in Config data
        if keyed_accounts[0].signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            Err(InstructionError::MissingRequiredSignature)?;
        }
    }

    let mut counter = 0;
    for (i, (signer, _)) in key_list
        .keys
        .iter()
        .filter(|(_, is_signer)| *is_signer)
        .enumerate()
    {
        counter += 1;
        if signer != keyed_accounts[0].unsigned_key() {
            let account_index = i + 1;
            let signer_account = keyed_accounts.get(account_index);
            if signer_account.is_none() {
                error!("account {:?} is not in account list", signer);
                Err(InstructionError::MissingRequiredSignature)?;
            }
            let signer_key = signer_account.unwrap().signer_key();
            if signer_key.is_none() {
                error!("account {:?} signer_key().is_none()", signer);
                Err(InstructionError::MissingRequiredSignature)?;
            }
            if signer_key.unwrap() != signer {
                error!(
                    "account[{:?}].signer_key() does not match Config data)",
                    account_index
                );
                Err(InstructionError::MissingRequiredSignature)?;
            }
            // If Config account is already initialized, update signatures must match Config data
            if !current_data.keys.is_empty()
                && current_signer_keys
                    .iter()
                    .find(|&pubkey| pubkey == signer)
                    .is_none()
            {
                error!("account {:?} is not in stored signer list", signer);
                Err(InstructionError::MissingRequiredSignature)?;
            }
        } else if keyed_accounts[0].signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            Err(InstructionError::MissingRequiredSignature)?;
        }
    }

    // Check for Config data signers not present in incoming account update
    if current_signer_keys.len() > counter {
        error!(
            "too few signers: {:?}; expected: {:?}",
            counter,
            current_signer_keys.len()
        );
        Err(InstructionError::MissingRequiredSignature)?;
    }

    if keyed_accounts[0].account.data.len() < data.len() {
        error!("instruction data too large");
        Err(InstructionError::InvalidInstructionData)?;
    }

    keyed_accounts[0].account.data[0..data.len()].copy_from_slice(&data);
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

    fn create_config_account(
        bank: Bank,
        mint_keypair: &Keypair,
        keys: Vec<(Pubkey, bool)>,
    ) -> (BankClient, Keypair) {
        let config_keypair = Keypair::new();
        let config_pubkey = config_keypair.pubkey();

        let bank_client = BankClient::new(bank);
        bank_client
            .send_instruction(
                mint_keypair,
                config_instruction::create_account::<MyConfig>(
                    &mint_keypair.pubkey(),
                    &config_pubkey,
                    1,
                    keys,
                ),
            )
            .expect("new_account");

        (bank_client, config_keypair)
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, vec![]);
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
        let keys = vec![];
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));

        bank_client
            .send_message(&[&mint_keypair, &config_keypair], message)
            .unwrap();

        let config_account_data = bank_client
            .get_account_data(&config_pubkey)
            .unwrap()
            .unwrap();
        let meta_length = ConfigKeys::serialized_size(keys);
        let config_account_data = &config_account_data[meta_length..config_account_data.len()];
        assert_eq!(
            my_config,
            MyConfig::deserialize(&config_account_data).unwrap()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, vec![]);
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        let mut instruction = config_instruction::store(&config_pubkey, true, vec![], &my_config);
        instruction.data = vec![0; 123]; // <-- Replace data with a vector that's too large
        let message = Message::new(vec![instruction]);
        bank_client
            .send_message(&[&config_keypair], message)
            .unwrap_err();
    }

    #[test]
    fn test_process_store_fail_account0_not_signer() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let system_keypair = Keypair::new();
        let system_pubkey = system_keypair.pubkey();

        bank.transfer(42, &mint_keypair, &system_pubkey).unwrap();
        let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, vec![]);
        let config_pubkey = config_keypair.pubkey();

        let transfer_instruction =
            system_instruction::transfer(&system_pubkey, &Pubkey::new_rand(), 42);
        let my_config = MyConfig::new(42);
        let mut store_instruction =
            config_instruction::store(&config_pubkey, true, vec![], &my_config);
        store_instruction.accounts[0].is_signer = false; // <----- not a signer

        let message = Message::new(vec![transfer_instruction, store_instruction]);
        bank_client
            .send_message(&[&system_keypair], message)
            .unwrap_err();
    }

    #[test]
    fn test_process_store_with_additional_signers() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let pubkey = Pubkey::new_rand();
        let signer0 = Keypair::new();
        let signer1 = Keypair::new();
        let keys = vec![
            (pubkey, false),
            (signer0.pubkey(), true),
            (signer1.pubkey(), true),
        ];
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));

        bank_client
            .send_message(
                &[&mint_keypair, &config_keypair, &signer0, &signer1],
                message,
            )
            .unwrap();

        let config_account_data = bank_client
            .get_account_data(&config_pubkey)
            .unwrap()
            .unwrap();
        let meta_length = ConfigKeys::serialized_size(keys.clone());
        let meta_data: ConfigKeys = deserialize(&config_account_data[0..meta_length]).unwrap();
        assert_eq!(meta_data.keys, keys);
        let config_account_data = &config_account_data[meta_length..config_account_data.len()];
        assert_eq!(
            my_config,
            MyConfig::deserialize(&config_account_data).unwrap()
        );
    }

    #[test]
    fn test_process_store_without_config_signer() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let pubkey = Pubkey::new_rand();
        let signer0 = Keypair::new();
        let keys = vec![(pubkey, false), (signer0.pubkey(), true)];
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));

        bank_client
            .send_message(&[&mint_keypair, &signer0], message)
            .unwrap_err();
    }

    #[test]
    fn test_process_store_with_bad_additional_signer() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let signer0 = Keypair::new();
        let signer1 = Keypair::new();
        let keys = vec![(signer0.pubkey(), true)];
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        // Config-data pubkey doesn't match signer
        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let mut message =
            Message::new_with_payer(vec![instruction.clone()], Some(&mint_keypair.pubkey()));
        message.account_keys[2] = signer1.pubkey();
        bank_client
            .send_message(&[&mint_keypair, &config_keypair, &signer1], message)
            .unwrap_err();

        // Config-data pubkey not a signer
        let mut message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        message.header.num_required_signatures = 2;
        bank_client
            .send_message(&[&mint_keypair, &config_keypair], message)
            .unwrap_err();
    }

    #[test]
    fn test_config_updates() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let pubkey = Pubkey::new_rand();
        let signer0 = Keypair::new();
        let signer1 = Keypair::new();
        let keys = vec![
            (pubkey, false),
            (signer0.pubkey(), true),
            (signer1.pubkey(), true),
        ];
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();

        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));

        bank_client
            .send_message(
                &[&mint_keypair, &config_keypair, &signer0, &signer1],
                message,
            )
            .unwrap();

        // Update with expected signatures
        let new_config = MyConfig::new(84);
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &new_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        bank_client
            .send_message(&[&mint_keypair, &signer0, &signer1], message)
            .unwrap();

        let config_account_data = bank_client
            .get_account_data(&config_pubkey)
            .unwrap()
            .unwrap();
        let meta_length = ConfigKeys::serialized_size(keys.clone());
        let meta_data: ConfigKeys = deserialize(&config_account_data[0..meta_length]).unwrap();
        assert_eq!(meta_data.keys, keys);
        let config_account_data = &config_account_data[meta_length..config_account_data.len()];
        assert_eq!(
            new_config,
            MyConfig::deserialize(&config_account_data).unwrap()
        );

        // Attempt update with incomplete signatures
        let keys = vec![(pubkey, false), (signer0.pubkey(), true)];
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        bank_client
            .send_message(&[&mint_keypair, &signer0], message)
            .unwrap_err();

        // Attempt update with incorrect signatures
        let signer2 = Keypair::new();
        let keys = vec![
            (pubkey, false),
            (signer0.pubkey(), true),
            (signer2.pubkey(), true),
        ];
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        bank_client
            .send_message(&[&mint_keypair, &signer0, &signer2], message)
            .unwrap_err();
    }

    #[test]
    fn test_config_updates_requiring_config() {
        solana_logger::setup();
        let (bank, mint_keypair) = create_bank(10_000);
        let pubkey = Pubkey::new_rand();
        let signer0 = Keypair::new();
        let keys = vec![
            (pubkey, false),
            (signer0.pubkey(), true),
            (signer0.pubkey(), true),
        ]; // Dummy keys for account sizing
        let (bank_client, config_keypair) =
            create_config_account(bank, &mint_keypair, keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let keys = vec![
            (pubkey, false),
            (signer0.pubkey(), true),
            (config_keypair.pubkey(), true),
        ];

        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));

        bank_client
            .send_message(&[&mint_keypair, &config_keypair, &signer0], message)
            .unwrap();

        // Update with expected signatures
        let new_config = MyConfig::new(84);
        let instruction =
            config_instruction::store(&config_pubkey, true, keys.clone(), &new_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        bank_client
            .send_message(&[&mint_keypair, &config_keypair, &signer0], message)
            .unwrap();

        let config_account_data = bank_client
            .get_account_data(&config_pubkey)
            .unwrap()
            .unwrap();
        let meta_length = ConfigKeys::serialized_size(keys.clone());
        let meta_data: ConfigKeys = deserialize(&config_account_data[0..meta_length]).unwrap();
        assert_eq!(meta_data.keys, keys);
        let config_account_data = &config_account_data[meta_length..config_account_data.len()];
        assert_eq!(
            new_config,
            MyConfig::deserialize(&config_account_data).unwrap()
        );

        // Attempt update with incomplete signatures
        let keys = vec![(pubkey, false), (config_keypair.pubkey(), true)];
        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
        bank_client
            .send_message(&[&mint_keypair, &config_keypair], message)
            .unwrap_err();
    }
}
