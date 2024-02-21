#[cfg(test)]
mod tests {
    use {
        crate::{bank::*, builtins::BUILTINS},
        solana_sdk::{
            ed25519_program,
            feature::{self, Feature},
            feature_set::FeatureSet,
            genesis_config::create_genesis_config,
        },
    };

    #[test]
    fn test_apply_builtin_program_feature_transitions_for_new_epoch() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.finish_init(&genesis_config, None, false);

        // Overwrite precompile accounts to simulate a cluster which already added precompiles.
        for precompile in get_precompiles() {
            bank.store_account(&precompile.program_id, &AccountSharedData::default());
            // Simulate cluster which added ed25519 precompile with a system program owner
            if precompile.program_id == ed25519_program::id() {
                bank.add_precompiled_account_with_owner(
                    &precompile.program_id,
                    solana_sdk::system_program::id(),
                );
            } else {
                bank.add_precompiled_account(&precompile.program_id);
            }
        }

        // Normally feature transitions are applied to a bank that hasn't been
        // frozen yet.  Freeze the bank early to ensure that no account changes
        // are made.
        bank.freeze();

        // Simulate crossing an epoch boundary for a new bank
        let only_apply_transitions_for_new_features = true;
        bank.apply_builtin_program_feature_transitions(
            only_apply_transitions_for_new_features,
            &HashSet::new(),
            None,
        );
    }

    #[test]
    fn test_startup_from_snapshot_after_precompile_transition() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);

        let mut bank = Bank::new_for_tests(&genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.finish_init(&genesis_config, None, false);

        // Overwrite precompile accounts to simulate a cluster which already added precompiles.
        for precompile in get_precompiles() {
            bank.store_account(&precompile.program_id, &AccountSharedData::default());
            bank.add_precompiled_account(&precompile.program_id);
        }

        bank.freeze();

        // Simulate starting up from snapshot finishing the initialization for a frozen bank
        bank.finish_init(&genesis_config, None, false);
    }

    #[test]
    fn test_override_builtins() {
        let check_bank_builtins = |builtins: &[BuiltinPrototype], expected_len| {
            let bank = Bank::new_with_paths_for_tests(
                &GenesisConfig::default(),
                Arc::<RuntimeConfig>::default(),
                Vec::new(),
                AccountSecondaryIndexes::default(),
                AccountShrinkThreshold::default(),
                Some(builtins),
            );

            // Assert the bank's builtins contain all additional builtins.
            assert_eq!(bank.builtin_programs.len(), expected_len);
            BUILTINS
                .iter()
                .filter(|b| b.feature_id.is_none())
                .chain(builtins)
                .for_each(|b| {
                    assert!(bank.builtin_programs.get(&b.program_id).is_some());
                });
        };

        let builtins_len = BUILTINS.iter().filter(|b| b.feature_id.is_none()).count();

        check_bank_builtins(&[], builtins_len);
        check_bank_builtins(
            &[BuiltinPrototype {
                feature_id: None,
                program_id: solana_system_program::id(),
                name: "system_program",
                entrypoint: solana_system_program::system_processor::Entrypoint::vm,
            }],
            // System program should be overriden.
            builtins_len,
        );
        check_bank_builtins(
            &[
                BuiltinPrototype {
                    feature_id: None,
                    program_id: solana_system_program::id(),
                    name: "system_program",
                    entrypoint: solana_system_program::system_processor::Entrypoint::vm,
                },
                BuiltinPrototype {
                    feature_id: None,
                    program_id: Pubkey::new_unique(),
                    name: "random_program",
                    entrypoint: solana_system_program::system_processor::Entrypoint::vm,
                },
            ],
            // System program should be overriden, and random program should be added.
            builtins_len + 1,
        );
        check_bank_builtins(
            &[
                BuiltinPrototype {
                    feature_id: None,
                    program_id: solana_system_program::id(),
                    name: "system_program",
                    entrypoint: solana_system_program::system_processor::Entrypoint::vm,
                },
                BuiltinPrototype {
                    feature_id: None,
                    program_id: solana_stake_program::id(),
                    name: "stake_program",
                    entrypoint: solana_stake_program::stake_instruction::Entrypoint::vm,
                },
                BuiltinPrototype {
                    feature_id: None,
                    program_id: Pubkey::new_unique(),
                    name: "random_program1",
                    entrypoint: solana_system_program::system_processor::Entrypoint::vm,
                },
                BuiltinPrototype {
                    feature_id: None,
                    program_id: Pubkey::new_unique(),
                    name: "stake_program2",
                    entrypoint: solana_stake_program::stake_instruction::Entrypoint::vm,
                },
            ],
            // System & stake should be overriden, and random programs should be added.
            builtins_len + 2,
        );
    }

    #[test]
    fn test_override_builtins_on_feature_activation() {
        let check_bank_builtin_feature_activation = |builtins: &[BuiltinPrototype]| {
            let mut bank = Bank::new_with_paths_for_tests(
                &GenesisConfig::default(),
                Arc::<RuntimeConfig>::default(),
                Vec::new(),
                AccountSecondaryIndexes::default(),
                AccountShrinkThreshold::default(),
                Some(builtins),
            );

            let mut feature_set = FeatureSet::default();
            builtins
                .iter()
                .filter_map(|builtin| builtin.feature_id)
                .for_each(|feature_id| {
                    feature_set.inactive.insert(feature_id);
                    bank.store_account(
                        &feature_id,
                        &feature::create_account(&Feature::default(), 42),
                    );
                });
            bank.feature_set = Arc::new(feature_set.clone());

            // Assert the bank's builtins _do not_ contain the additional
            // builtins, since they have not been enabled.
            builtins.iter().for_each(|b| {
                assert!(bank.builtin_programs.get(&b.program_id).is_none());
            });

            bank.apply_feature_activations(
                ApplyFeatureActivationsCaller::NewFromParent,
                false,
                Some(builtins),
            );

            // Assert the bank's builtins contain the additional builtins,
            // since they have now been enabled.
            builtins.iter().for_each(|builtin| {
                assert!(bank.builtin_programs.get(&builtin.program_id).is_some());
            });
        };

        check_bank_builtin_feature_activation(&[]);
        check_bank_builtin_feature_activation(&[BuiltinPrototype {
            feature_id: Some(Pubkey::new_unique()),
            program_id: solana_system_program::id(),
            name: "system_program",
            entrypoint: solana_system_program::system_processor::Entrypoint::vm,
        }]);
        check_bank_builtin_feature_activation(&[
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: solana_system_program::id(),
                name: "system_program",
                entrypoint: solana_system_program::system_processor::Entrypoint::vm,
            },
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: Pubkey::new_unique(),
                name: "random_program",
                entrypoint: solana_system_program::system_processor::Entrypoint::vm,
            },
        ]);
        check_bank_builtin_feature_activation(&[
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: solana_system_program::id(),
                name: "system_program",
                entrypoint: solana_system_program::system_processor::Entrypoint::vm,
            },
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: solana_stake_program::id(),
                name: "stake_program",
                entrypoint: solana_stake_program::stake_instruction::Entrypoint::vm,
            },
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: Pubkey::new_unique(),
                name: "random_program1",
                entrypoint: solana_system_program::system_processor::Entrypoint::vm,
            },
            BuiltinPrototype {
                feature_id: Some(Pubkey::new_unique()),
                program_id: Pubkey::new_unique(),
                name: "stake_program2",
                entrypoint: solana_stake_program::stake_instruction::Entrypoint::vm,
            },
        ]);
    }
}
