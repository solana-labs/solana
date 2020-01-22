//! Config program

use crate::ConfigKeys;
use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::{limited_deserialize, next_keyed_account};
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let key_list: ConfigKeys = limited_deserialize(data)?;
    let keyed_accounts_iter = &mut keyed_accounts.iter();
    let config_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
    let current_data: ConfigKeys = {
        let config_account = config_keyed_account.try_account_ref_mut()?;
        deserialize(&config_account.data).map_err(|err| {
            error!(
                "Unable to deserialize account[0]: {:?} (len={}): {:?}",
                config_account.data,
                config_account.data.len(),
                err
            );
            InstructionError::InvalidAccountData
        })?
    };
    let current_signer_keys: Vec<Pubkey> = current_data
        .keys
        .iter()
        .filter(|(_, is_signer)| *is_signer)
        .map(|(pubkey, _)| *pubkey)
        .collect();

    if current_signer_keys.is_empty() {
        // Config account keypair must be a signer on account initialization,
        // or when no signers specified in Config data
        if config_keyed_account.signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    let mut counter = 0;
    for (signer, _) in key_list.keys.iter().filter(|(_, is_signer)| *is_signer) {
        counter += 1;
        if signer != config_keyed_account.unsigned_key() {
            let signer_account = keyed_accounts_iter.next();
            if signer_account.is_none() {
                error!("account {:?} is not in account list", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
            let signer_key = signer_account.unwrap().signer_key();
            if signer_key.is_none() {
                error!("account {:?} signer_key().is_none()", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
            if signer_key.unwrap() != signer {
                error!(
                    "account[{:?}].signer_key() does not match Config data)",
                    counter + 1
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            // If Config account is already initialized, update signatures must match Config data
            if !current_data.keys.is_empty()
                && current_signer_keys
                    .iter()
                    .find(|&pubkey| pubkey == signer)
                    .is_none()
            {
                error!("account {:?} is not in stored signer list", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
        } else if config_keyed_account.signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    // Check for Config data signers not present in incoming account update
    if current_signer_keys.len() > counter {
        error!(
            "too few signers: {:?}; expected: {:?}",
            counter,
            current_signer_keys.len()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    if config_keyed_account.data_len()? < data.len() {
        error!("instruction data too large");
        return Err(InstructionError::InvalidInstructionData);
    }

    config_keyed_account.try_account_ref_mut()?.data[0..data.len()].copy_from_slice(&data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config_instruction, get_config_data, id, ConfigKeys, ConfigState};
    use bincode::serialized_size;
    use serde_derive::{Deserialize, Serialize};
    use solana_sdk::{
        account::{create_keyed_is_signer_accounts, Account},
        signature::{Keypair, KeypairUtil},
        system_instruction::SystemInstruction,
    };
    use std::cell::RefCell;

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

    fn create_config_account(keys: Vec<(Pubkey, bool)>) -> (Keypair, RefCell<Account>) {
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
        let mut config_account = RefCell::new(Account {
            data: vec![0; space as usize],
            ..Account::default()
        });
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
            deserialize(get_config_data(&config_account.borrow().data).unwrap()).ok()
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
            deserialize(get_config_data(&config_account.borrow().data).unwrap()).ok()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let keys = vec![];
        let (config_keypair, mut config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let mut instruction =
            config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
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
        let mut signer0_account = RefCell::new(Account::default());
        let mut signer1_account = RefCell::new(Account::default());
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
        let meta_data: ConfigKeys = deserialize(&config_account.borrow().data).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            Some(my_config),
            deserialize(get_config_data(&config_account.borrow().data).unwrap()).ok()
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

        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
        let mut signer0_account = RefCell::new(Account::default());
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
        let mut signer0_account = RefCell::new(Account::default());
        let mut signer1_account = RefCell::new(Account::default());
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

    #[test]
    fn test_config_updates() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let signer0_pubkey = Pubkey::new_rand();
        let signer1_pubkey = Pubkey::new_rand();
        let signer2_pubkey = Pubkey::new_rand();
        let mut signer0_account = RefCell::new(Account::default());
        let mut signer1_account = RefCell::new(Account::default());
        let mut signer2_account = RefCell::new(Account::default());
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
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &new_config);
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
        let meta_data: ConfigKeys = deserialize(&config_account.borrow().data).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            new_config,
            MyConfig::deserialize(get_config_data(&config_account.borrow().data).unwrap()).unwrap()
        );

        // Attempt update with incomplete signatures
        let keys = vec![(pubkey, false), (signer0_pubkey, true)];
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
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
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &my_config);
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
        let mut signer0_account = RefCell::new(Account::default());
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
        let instruction =
            config_instruction::store(&config_pubkey, true, keys.clone(), &new_config);
        let mut accounts = vec![
            (&config_pubkey, true, &mut config_account),
            (&signer0_pubkey, true, &mut signer0_account),
        ];
        let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &instruction.data),
            Ok(())
        );
        let meta_data: ConfigKeys = deserialize(&config_account.borrow().data).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            new_config,
            MyConfig::deserialize(get_config_data(&config_account.borrow().data).unwrap()).unwrap()
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

    #[test]
    fn test_config_initialize_no_panic() {
        let from_pubkey = Pubkey::new_rand();
        let config_pubkey = Pubkey::new_rand();
        let instructions =
            config_instruction::create_account::<MyConfig>(&from_pubkey, &config_pubkey, 1, vec![]);
        let mut accounts = vec![];
        let mut keyed_accounts = create_keyed_is_signer_accounts(&mut accounts);
        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &instructions[1].data),
            Err(InstructionError::NotEnoughAccountKeys)
        );
    }
}
