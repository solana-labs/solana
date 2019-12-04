//! Config program

use bincode::{deserialize, serialized_size};
use serde_derive::{Deserialize, Serialize};
use solana_config_program::{
    config_instruction, config_processor::process_instruction, get_config_data, id, ConfigKeys,
    ConfigState,
};
use solana_sdk::{
    account::{create_keyed_is_signer_accounts, Account},
    instruction::InstructionError,
    instruction_processor_utils::limited_deserialize,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction::SystemInstruction,
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

fn create_config_account(keys: Vec<(Pubkey, bool)>) -> (Keypair, Account) {
    let from_pubkey = Pubkey::new_rand();
    let config_keypair = Keypair::new();
    let config_pubkey = config_keypair.pubkey();

    let instructions =
        config_instruction::create_account::<MyConfig>(&from_pubkey, &config_pubkey, 1, keys);

    let system_instruction = limited_deserialize(&instructions[0].data).unwrap();
    let space = match system_instruction {
        SystemInstruction::CreateAccount {
            lamports: _,
            space,
            program_id: _,
        } => space,
        _ => panic!("Not a CreateAccount system instruction"),
    };
    let mut config_account = Account {
        data: vec![0; space as usize],
        ..Account::default()
    };
    let mut accounts = vec![(&config_pubkey, true, &mut config_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instructions[1].data),
        Ok(())
    );

    (config_keypair, config_account)
}

#[test]
fn test_process_create_ok() {
    solana_logger::setup();
    let keys = vec![];
    let (_, config_account) = create_config_account(keys.clone());
    assert_eq!(
        Some(MyConfig::default()),
        deserialize(get_config_data(&config_account.data).unwrap()).ok()
    );
}

#[test]
fn test_process_store_ok() {
    solana_logger::setup();
    let keys = vec![];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let mut accounts = vec![(&config_pubkey, true, &mut config_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );
    assert_eq!(
        Some(my_config),
        deserialize(get_config_data(&config_account.data).unwrap()).ok()
    );
}

#[test]
fn test_process_store_fail_instruction_data_too_large() {
    solana_logger::setup();
    let keys = vec![];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let mut instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    instruction.data = vec![0; 123]; // <-- Replace data with a vector that's too large
    let mut accounts = vec![(&config_pubkey, true, &mut config_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::InvalidInstructionData)
    );
}

#[test]
fn test_process_store_fail_account0_not_signer() {
    solana_logger::setup();
    let keys = vec![];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let mut instruction = config_instruction::store(&config_pubkey, true, vec![], &my_config);
    instruction.accounts[0].is_signer = false; // <----- not a signer
    let mut accounts = vec![(&config_pubkey, false, &mut config_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );
}

#[test]
fn test_process_store_with_additional_signers() {
    solana_logger::setup();
    let pubkey = Pubkey::new_rand();
    let signer0_pubkey = Pubkey::new_rand();
    let signer1_pubkey = Pubkey::new_rand();
    let keys = vec![
        (pubkey, false),
        (signer0_pubkey, true),
        (signer1_pubkey, true),
    ];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let mut signer0_account = Account::default();
    let mut signer1_account = Account::default();
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
        (&signer1_pubkey, true, &mut signer1_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );
    let meta_data: ConfigKeys = deserialize(&config_account.data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        Some(my_config),
        deserialize(get_config_data(&config_account.data).unwrap()).ok()
    );
}

#[test]
fn test_process_store_without_config_signer() {
    solana_logger::setup();
    let pubkey = Pubkey::new_rand();
    let signer0_pubkey = Pubkey::new_rand();
    let keys = vec![(pubkey, false), (signer0_pubkey, true)];
    let (config_keypair, _) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
    let mut signer0_account = Account::default();
    let mut accounts = vec![(&signer0_pubkey, true, &mut signer0_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::InvalidAccountData)
    );
}

#[test]
fn test_process_store_with_bad_additional_signer() {
    solana_logger::setup();
    let signer0_pubkey = Pubkey::new_rand();
    let signer1_pubkey = Pubkey::new_rand();
    let mut signer0_account = Account::default();
    let mut signer1_account = Account::default();
    let keys = vec![(signer0_pubkey, true)];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);

    // Config-data pubkey doesn't match signer
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer1_pubkey, true, &mut signer1_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );

    // Config-data pubkey not a signer
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer0_pubkey, false, &mut signer0_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );
}

// TODO need from_pubkey in all theses?

#[test]
fn test_config_updates() {
    solana_logger::setup();
    let pubkey = Pubkey::new_rand();
    let signer0_pubkey = Pubkey::new_rand();
    let signer1_pubkey = Pubkey::new_rand();
    let signer2_pubkey = Pubkey::new_rand();
    let mut signer0_account = Account::default();
    let mut signer1_account = Account::default();
    let mut signer2_account = Account::default();
    let keys = vec![
        (pubkey, false),
        (signer0_pubkey, true),
        (signer1_pubkey, true),
    ];
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
        (&signer1_pubkey, true, &mut signer1_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );

    // Update with expected signatures
    let new_config = MyConfig::new(84);
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &new_config);
    let mut accounts = vec![
        (&config_pubkey, false, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
        (&signer1_pubkey, true, &mut signer1_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );
    let meta_data: ConfigKeys = deserialize(&config_account.data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        new_config,
        MyConfig::deserialize(get_config_data(&config_account.data).unwrap()).unwrap()
    );

    // Attempt update with incomplete signatures
    let keys = vec![(pubkey, false), (signer0_pubkey, true)];
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
    let mut accounts = vec![
        (&config_pubkey, false, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
        (&signer1_pubkey, false, &mut signer1_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );

    // Attempt update with incorrect signatures
    let keys = vec![
        (pubkey, false),
        (signer0_pubkey, true),
        (signer2_pubkey, true),
    ];
    let instruction = config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
    let mut accounts = vec![
        (&config_pubkey, false, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
        (&signer2_pubkey, true, &mut signer2_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );
}

#[test]
fn test_config_updates_requiring_config() {
    solana_logger::setup();
    let pubkey = Pubkey::new_rand();
    let signer0_pubkey = Pubkey::new_rand();
    let mut signer0_account = Account::default();
    let keys = vec![
        (pubkey, false),
        (signer0_pubkey, true),
        (signer0_pubkey, true),
    ]; // Dummy keys for account sizing
    let (config_keypair, mut config_account) = create_config_account(keys.clone());
    let config_pubkey = config_keypair.pubkey();
    let my_config = MyConfig::new(42);

    let keys = vec![
        (pubkey, false),
        (signer0_pubkey, true),
        (config_keypair.pubkey(), true),
    ];

    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );

    // Update with expected signatures
    let new_config = MyConfig::new(84);
    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &new_config);
    let mut accounts = vec![
        (&config_pubkey, true, &mut config_account),
        (&signer0_pubkey, true, &mut signer0_account),
    ];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Ok(())
    );
    let meta_data: ConfigKeys = deserialize(&config_account.data).unwrap();
    assert_eq!(meta_data.keys, keys);
    assert_eq!(
        new_config,
        MyConfig::deserialize(get_config_data(&config_account.data).unwrap()).unwrap()
    );

    // Attempt update with incomplete signatures
    let keys = vec![(pubkey, false), (config_keypair.pubkey(), true)];
    let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
    let mut accounts = vec![(&config_pubkey, true, &mut config_account)];
    let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &instruction.data),
        Err(InstructionError::MissingRequiredSignature)
    );
}
