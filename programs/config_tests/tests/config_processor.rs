//! Config program

use bincode::{deserialize, serialized_size};
use serde_derive::{Deserialize, Serialize};
use solana_config_api::{
    config_instruction, config_processor::process_instruction, get_config_data, id, ConfigKeys,
    ConfigState,
};
use solana_runtime::{bank::Bank, bank_client::BankClient};
use solana_sdk::{
    client::SyncClient,
    genesis_block::create_genesis_block,
    instruction::InstructionError,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction,
    transaction::TransactionError,
};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct MyConfig {
    pub item: u64,
}
impl Default for MyConfig {
    fn default() -> Self {
        Self { item: 123456789 }
    }
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
        .send_message(
            &[mint_keypair, &config_keypair],
            Message::new(config_instruction::create_account::<MyConfig>(
                &mint_keypair.pubkey(),
                &config_pubkey,
                1,
                keys,
            )),
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
        Some(MyConfig::default()),
        deserialize(get_config_data(&config_account_data).unwrap()).ok()
    );
}

#[test]
fn test_process_store_ok() {
    solana_logger::setup();
    let (bank, mint_keypair) = create_bank(10_000);
    let keys = vec![];
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
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
    assert_eq!(
        Some(my_config),
        deserialize(get_config_data(&config_account_data).unwrap()).ok()
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
    let mut store_instruction = config_instruction::store(&config_pubkey, true, vec![], &my_config);
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
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
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
    let meta_data: ConfigKeys = deserialize(&config_account_data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        Some(my_config),
        deserialize(get_config_data(&config_account_data).unwrap()).ok()
    );
}

#[test]
fn test_process_store_without_config_signer() {
    solana_logger::setup();
    let (bank, mint_keypair) = create_bank(10_000);
    let pubkey = Pubkey::new_rand();
    let signer0 = Keypair::new();
    let keys = vec![(pubkey, false), (signer0.pubkey(), true)];
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
    let config_pubkey = config_keypair.pubkey();

    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
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
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
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
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
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
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &new_config);
    let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
    bank_client
        .send_message(&[&mint_keypair, &signer0, &signer1], message)
        .unwrap();

    let config_account_data = bank_client
        .get_account_data(&config_pubkey)
        .unwrap()
        .unwrap();
    let meta_data: ConfigKeys = deserialize(&config_account_data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        new_config,
        MyConfig::deserialize(get_config_data(&config_account_data).unwrap()).unwrap()
    );

    // Attempt update with incomplete signatures
    let keys = vec![(pubkey, false), (signer0.pubkey(), true)];
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
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
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
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
    let (bank_client, config_keypair) = create_config_account(bank, &mint_keypair, keys.clone());
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
    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &new_config);
    let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
    bank_client
        .send_message(&[&mint_keypair, &config_keypair, &signer0], message)
        .unwrap();

    let config_account_data = bank_client
        .get_account_data(&config_pubkey)
        .unwrap()
        .unwrap();
    let meta_data: ConfigKeys = deserialize(&config_account_data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        new_config,
        MyConfig::deserialize(get_config_data(&config_account_data).unwrap()).unwrap()
    );

    // Attempt update with incomplete signatures
    let keys = vec![(pubkey, false), (config_keypair.pubkey(), true)];
    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let message = Message::new_with_payer(vec![instruction], Some(&mint_keypair.pubkey()));
    bank_client
        .send_message(&[&mint_keypair, &config_keypair], message)
        .unwrap_err();
}

#[test]
fn test_config_initialize_no_panic() {
    let (bank, alice_keypair) = create_bank(3);
    let bank_client = BankClient::new(bank);
    let config_account = Keypair::new();

    let mut instructions = config_instruction::create_account::<MyConfig>(
        &alice_keypair.pubkey(),
        &config_account.pubkey(),
        1,
        vec![],
    );
    instructions[1].accounts = vec![]; // <!-- Attack! Prevent accounts from being passed into processor.

    let message = Message::new(instructions);
    assert_eq!(
        bank_client
            .send_message(&[&alice_keypair, &config_account], message)
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(1, InstructionError::NotEnoughAccountKeys)
    );
}
