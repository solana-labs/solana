//! Config program

use {
    crate::ConfigKeys,
    bincode::deserialize,
    solana_program_runtime::{declare_process_instruction, ic_msg},
    solana_sdk::{
        feature_set, instruction::InstructionError, program_utils::limited_deserialize,
        pubkey::Pubkey, transaction_context::IndexOfAccount,
    },
    std::collections::BTreeSet,
};

pub const DEFAULT_COMPUTE_UNITS: u64 = 450;

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let data = instruction_context.get_instruction_data();

    let key_list: ConfigKeys = limited_deserialize(data)?;
    let config_account_key = transaction_context.get_key_of_account_at_index(
        instruction_context.get_index_of_instruction_account_in_transaction(0)?,
    )?;
    let config_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let is_config_account_signer = config_account.is_signer();
    let current_data: ConfigKeys = {
        if config_account.get_owner() != &crate::id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        deserialize(config_account.get_data()).map_err(|err| {
            ic_msg!(
                invoke_context,
                "Unable to deserialize config account: {}",
                err
            );
            InstructionError::InvalidAccountData
        })?
    };
    drop(config_account);

    let current_signer_keys: Vec<Pubkey> = current_data
        .keys
        .iter()
        .filter(|(_, is_signer)| *is_signer)
        .map(|(pubkey, _)| *pubkey)
        .collect();
    if current_signer_keys.is_empty() {
        // Config account keypair must be a signer on account initialization,
        // or when no signers specified in Config data
        if !is_config_account_signer {
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    let mut counter = 0;
    for (signer, _) in key_list.keys.iter().filter(|(_, is_signer)| *is_signer) {
        counter += 1;
        if signer != config_account_key {
            let signer_account = instruction_context
                .try_borrow_instruction_account(transaction_context, counter as IndexOfAccount)
                .map_err(|_| {
                    ic_msg!(
                        invoke_context,
                        "account {:?} is not in account list",
                        signer,
                    );
                    InstructionError::MissingRequiredSignature
                })?;
            if !signer_account.is_signer() {
                ic_msg!(
                    invoke_context,
                    "account {:?} signer_key().is_none()",
                    signer
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            if signer_account.get_key() != signer {
                ic_msg!(
                    invoke_context,
                    "account[{:?}].signer_key() does not match Config data)",
                    counter + 1
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            // If Config account is already initialized, update signatures must match Config data
            if !current_data.keys.is_empty()
                && !current_signer_keys.iter().any(|pubkey| pubkey == signer)
            {
                ic_msg!(
                    invoke_context,
                    "account {:?} is not in stored signer list",
                    signer
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
        } else if !is_config_account_signer {
            ic_msg!(invoke_context, "account[0].signer_key().is_none()");
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    if invoke_context
        .feature_set
        .is_active(&feature_set::dedupe_config_program_signers::id())
    {
        let total_new_keys = key_list.keys.len();
        let unique_new_keys = key_list.keys.into_iter().collect::<BTreeSet<_>>();
        if unique_new_keys.len() != total_new_keys {
            ic_msg!(invoke_context, "new config contains duplicate keys");
            return Err(InstructionError::InvalidArgument);
        }
    }

    // Check for Config data signers not present in incoming account update
    if current_signer_keys.len() > counter {
        ic_msg!(
            invoke_context,
            "too few signers: {:?}; expected: {:?}",
            counter,
            current_signer_keys.len()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    let mut config_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    if config_account.get_data().len() < data.len() {
        ic_msg!(invoke_context, "instruction data too large");
        return Err(InstructionError::InvalidInstructionData);
    }
    config_account.get_data_mut()?[..data.len()].copy_from_slice(data);
    Ok(())
});

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{config_instruction, get_config_data, id, ConfigKeys, ConfigState},
        bincode::serialized_size,
        serde_derive::{Deserialize, Serialize},
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            instruction::AccountMeta,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_instruction::SystemInstruction,
        },
    };

    fn process_instruction(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            expected_result,
            Entrypoint::vm,
            |_invoke_context| {},
            |_invoke_context| {},
        )
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct MyConfig {
        pub item: u64,
    }
    impl Default for MyConfig {
        fn default() -> Self {
            Self { item: 123_456_789 }
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

    fn create_config_account(keys: Vec<(Pubkey, bool)>) -> (Keypair, AccountSharedData) {
        let from_pubkey = Pubkey::new_unique();
        let config_keypair = Keypair::new();
        let config_pubkey = config_keypair.pubkey();
        let instructions =
            config_instruction::create_account::<MyConfig>(&from_pubkey, &config_pubkey, 1, keys);
        let system_instruction = limited_deserialize(&instructions[0].data).unwrap();
        let SystemInstruction::CreateAccount {
            lamports: _,
            space,
            owner: _,
        } = system_instruction
        else {
            panic!("Not a CreateAccount system instruction")
        };
        let config_account = AccountSharedData::new(0, space as usize, &id());
        let accounts = process_instruction(
            &instructions[1].data,
            vec![(config_pubkey, config_account)],
            vec![AccountMeta {
                pubkey: config_pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
        );
        (config_keypair, accounts[0].clone())
    }

    #[test]
    fn test_process_create_ok() {
        solana_logger::setup();
        let (_, config_account) = create_config_account(vec![]);
        assert_eq!(
            Some(MyConfig::default()),
            deserialize(get_config_data(config_account.data()).unwrap()).ok()
        );
    }

    #[test]
    fn test_process_store_ok() {
        solana_logger::setup();
        let keys = vec![];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![(config_pubkey, config_account)],
            vec![AccountMeta {
                pubkey: config_pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Ok(()),
        );
        assert_eq!(
            Some(my_config),
            deserialize(get_config_data(accounts[0].data()).unwrap()).ok()
        );
    }

    #[test]
    fn test_process_store_fail_instruction_data_too_large() {
        solana_logger::setup();
        let keys = vec![];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let mut instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        instruction.data = vec![0; 123]; // <-- Replace data with a vector that's too large
        process_instruction(
            &instruction.data,
            vec![(config_pubkey, config_account)],
            vec![AccountMeta {
                pubkey: config_pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::InvalidInstructionData),
        );
    }

    #[test]
    fn test_process_store_fail_account0_not_signer() {
        solana_logger::setup();
        let keys = vec![];
        let (config_keypair, config_account) = create_config_account(keys);
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let mut instruction = config_instruction::store(&config_pubkey, true, vec![], &my_config);
        instruction.accounts[0].is_signer = false; // <----- not a signer
        process_instruction(
            &instruction.data,
            vec![(config_pubkey, config_account)],
            vec![AccountMeta {
                pubkey: config_pubkey,
                is_signer: false,
                is_writable: true,
            }],
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_process_store_with_additional_signers() {
        solana_logger::setup();
        let pubkey = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let signer1_pubkey = Pubkey::new_unique();
        let keys = vec![
            (pubkey, false),
            (signer0_pubkey, true),
            (signer1_pubkey, true),
        ];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let signer1_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account),
                (signer1_pubkey, signer1_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        let meta_data: ConfigKeys = deserialize(accounts[0].data()).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            Some(my_config),
            deserialize(get_config_data(accounts[0].data()).unwrap()).ok()
        );
    }

    #[test]
    fn test_process_store_without_config_signer() {
        solana_logger::setup();
        let pubkey = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let keys = vec![(pubkey, false), (signer0_pubkey, true)];
        let (config_keypair, _) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);
        let signer0_account = AccountSharedData::new(0, 0, &id());

        let instruction = config_instruction::store(&config_pubkey, false, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![(signer0_pubkey, signer0_account)],
            vec![AccountMeta {
                pubkey: signer0_pubkey,
                is_signer: true,
                is_writable: false,
            }],
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_store_with_bad_additional_signer() {
        solana_logger::setup();
        let signer0_pubkey = Pubkey::new_unique();
        let signer1_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let signer1_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let keys = vec![(signer0_pubkey, true)];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        // Config-data pubkey doesn't match signer
        let instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account.clone()),
                (signer1_pubkey, signer1_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Config-data pubkey not a signer
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_config_updates() {
        solana_logger::setup();
        let pubkey = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let signer1_pubkey = Pubkey::new_unique();
        let signer2_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let signer1_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let signer2_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let keys = vec![
            (pubkey, false),
            (signer0_pubkey, true),
            (signer1_pubkey, true),
        ];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account.clone()),
                (signer1_pubkey, signer1_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Update with expected signatures
        let new_config = MyConfig::new(84);
        let instruction =
            config_instruction::store(&config_pubkey, false, keys.clone(), &new_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, accounts[0].clone()),
                (signer0_pubkey, signer0_account.clone()),
                (signer1_pubkey, signer1_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        let meta_data: ConfigKeys = deserialize(accounts[0].data()).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            new_config,
            MyConfig::deserialize(get_config_data(accounts[0].data()).unwrap()).unwrap()
        );

        // Attempt update with incomplete signatures
        let keys = vec![(pubkey, false), (signer0_pubkey, true)];
        let instruction = config_instruction::store(&config_pubkey, false, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, accounts[0].clone()),
                (signer0_pubkey, signer0_account.clone()),
                (signer1_pubkey, signer1_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );

        // Attempt update with incorrect signatures
        let keys = vec![
            (pubkey, false),
            (signer0_pubkey, true),
            (signer2_pubkey, true),
        ];
        let instruction = config_instruction::store(&config_pubkey, false, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, accounts[0].clone()),
                (signer0_pubkey, signer0_account),
                (signer2_pubkey, signer2_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer2_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_config_initialize_contains_duplicates_fails() {
        solana_logger::setup();
        let config_address = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let keys = vec![
            (config_address, false),
            (signer0_pubkey, true),
            (signer0_pubkey, true),
        ];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        // Attempt initialization with duplicate signer inputs
        let instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_config_update_contains_duplicates_fails() {
        solana_logger::setup();
        let config_address = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let signer1_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let signer1_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let keys = vec![
            (config_address, false),
            (signer0_pubkey, true),
            (signer1_pubkey, true),
        ];
        let (config_keypair, config_account) = create_config_account(keys.clone());
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);

        let instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account.clone()),
                (signer1_pubkey, signer1_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer1_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Attempt update with duplicate signer inputs
        let new_config = MyConfig::new(84);
        let dupe_keys = vec![
            (config_address, false),
            (signer0_pubkey, true),
            (signer0_pubkey, true),
        ];
        let instruction = config_instruction::store(&config_pubkey, false, dupe_keys, &new_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, accounts[0].clone()),
                (signer0_pubkey, signer0_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_config_updates_requiring_config() {
        solana_logger::setup();
        let pubkey = Pubkey::new_unique();
        let signer0_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let keys = vec![
            (pubkey, false),
            (signer0_pubkey, true),
            (signer0_pubkey, true),
        ]; // Dummy keys for account sizing
        let (config_keypair, config_account) = create_config_account(keys);
        let config_pubkey = config_keypair.pubkey();
        let my_config = MyConfig::new(42);
        let keys = vec![
            (pubkey, false),
            (signer0_pubkey, true),
            (config_keypair.pubkey(), true),
        ];

        let instruction = config_instruction::store(&config_pubkey, true, keys.clone(), &my_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Update with expected signatures
        let new_config = MyConfig::new(84);
        let instruction =
            config_instruction::store(&config_pubkey, true, keys.clone(), &new_config);
        let accounts = process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, accounts[0].clone()),
                (signer0_pubkey, signer0_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        let meta_data: ConfigKeys = deserialize(accounts[0].data()).unwrap();
        assert_eq!(meta_data.keys, keys);
        assert_eq!(
            new_config,
            MyConfig::deserialize(get_config_data(accounts[0].data()).unwrap()).unwrap()
        );

        // Attempt update with incomplete signatures
        let keys = vec![(pubkey, false), (config_keypair.pubkey(), true)];
        let instruction = config_instruction::store(&config_pubkey, true, keys, &my_config);
        process_instruction(
            &instruction.data,
            vec![(config_pubkey, accounts[0].clone())],
            vec![AccountMeta {
                pubkey: config_pubkey,
                is_signer: true,
                is_writable: true,
            }],
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_config_initialize_no_panic() {
        let from_pubkey = Pubkey::new_unique();
        let config_pubkey = Pubkey::new_unique();
        let (_, _config_account) = create_config_account(vec![]);
        let instructions =
            config_instruction::create_account::<MyConfig>(&from_pubkey, &config_pubkey, 1, vec![]);
        process_instruction(
            &instructions[1].data,
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_config_bad_owner() {
        let from_pubkey = Pubkey::new_unique();
        let config_pubkey = Pubkey::new_unique();
        let new_config = MyConfig::new(84);
        let signer0_pubkey = Pubkey::new_unique();
        let signer0_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let config_account = AccountSharedData::new(0, 0, &Pubkey::new_unique());
        let (_, _config_account) = create_config_account(vec![]);
        let keys = vec![
            (from_pubkey, false),
            (signer0_pubkey, true),
            (config_pubkey, true),
        ];

        let instruction = config_instruction::store(&config_pubkey, true, keys, &new_config);
        process_instruction(
            &instruction.data,
            vec![
                (config_pubkey, config_account),
                (signer0_pubkey, signer0_account),
            ],
            vec![
                AccountMeta {
                    pubkey: config_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: signer0_pubkey,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidAccountOwner),
        );
    }
}
