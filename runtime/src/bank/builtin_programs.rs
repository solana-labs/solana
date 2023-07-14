#[cfg(test)]
mod tests {
    use {
        crate::bank::*,
        solana_sdk::{
            ed25519_program, feature_set::FeatureSet, genesis_config::create_genesis_config,
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
}
