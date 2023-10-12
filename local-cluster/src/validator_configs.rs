use {
    solana_core::validator::ValidatorConfig,
    solana_sdk::exit::Exit,
    std::sync::{Arc, RwLock},
};

pub fn safe_clone_config(config: &ValidatorConfig) -> ValidatorConfig {
    ValidatorConfig {
        halt_at_slot: config.halt_at_slot,
        expected_genesis_hash: config.expected_genesis_hash,
        expected_bank_hash: config.expected_bank_hash,
        expected_shred_version: config.expected_shred_version,
        voting_disabled: config.voting_disabled,
        account_paths: config.account_paths.clone(),
        account_snapshot_paths: config.account_snapshot_paths.clone(),
        account_shrink_paths: config.account_shrink_paths.clone(),
        rpc_config: config.rpc_config.clone(),
        on_start_geyser_plugin_config_files: config.on_start_geyser_plugin_config_files.clone(),
        rpc_addrs: config.rpc_addrs,
        pubsub_config: config.pubsub_config.clone(),
        snapshot_config: config.snapshot_config.clone(),
        max_ledger_shreds: config.max_ledger_shreds,
        broadcast_stage_type: config.broadcast_stage_type.clone(),
        turbine_disabled: config.turbine_disabled.clone(),
        enforce_ulimit_nofile: config.enforce_ulimit_nofile,
        fixed_leader_schedule: config.fixed_leader_schedule.clone(),
        wait_for_supermajority: config.wait_for_supermajority,
        new_hard_forks: config.new_hard_forks.clone(),
        known_validators: config.known_validators.clone(),
        repair_validators: config.repair_validators.clone(),
        repair_whitelist: config.repair_whitelist.clone(),
        gossip_validators: config.gossip_validators.clone(),
        accounts_hash_interval_slots: config.accounts_hash_interval_slots,
        accounts_hash_fault_injector: config.accounts_hash_fault_injector,
        max_genesis_archive_unpacked_size: config.max_genesis_archive_unpacked_size,
        wal_recovery_mode: config.wal_recovery_mode.clone(),
        run_verification: config.run_verification,
        require_tower: config.require_tower,
        tower_storage: config.tower_storage.clone(),
        debug_keys: config.debug_keys.clone(),
        contact_debug_interval: config.contact_debug_interval,
        contact_save_interval: config.contact_save_interval,
        send_transaction_service_config: config.send_transaction_service_config.clone(),
        no_poh_speed_test: config.no_poh_speed_test,
        no_os_memory_stats_reporting: config.no_os_memory_stats_reporting,
        no_os_network_stats_reporting: config.no_os_network_stats_reporting,
        no_os_cpu_stats_reporting: config.no_os_cpu_stats_reporting,
        no_os_disk_stats_reporting: config.no_os_disk_stats_reporting,
        poh_pinned_cpu_core: config.poh_pinned_cpu_core,
        account_indexes: config.account_indexes.clone(),
        warp_slot: config.warp_slot,
        accounts_db_test_hash_calculation: config.accounts_db_test_hash_calculation,
        accounts_db_skip_shrink: config.accounts_db_skip_shrink,
        accounts_db_force_initial_clean: config.accounts_db_force_initial_clean,
        tpu_coalesce: config.tpu_coalesce,
        staked_nodes_overrides: config.staked_nodes_overrides.clone(),
        validator_exit: Arc::new(RwLock::new(Exit::default())),
        poh_hashes_per_batch: config.poh_hashes_per_batch,
        process_ledger_before_services: config.process_ledger_before_services,
        no_wait_for_vote_to_start_leader: config.no_wait_for_vote_to_start_leader,
        accounts_shrink_ratio: config.accounts_shrink_ratio,
        accounts_db_config: config.accounts_db_config.clone(),
        wait_to_vote_slot: config.wait_to_vote_slot,
        ledger_column_options: config.ledger_column_options.clone(),
        runtime_config: config.runtime_config.clone(),
        replay_slots_concurrently: config.replay_slots_concurrently,
        banking_trace_dir_byte_limit: config.banking_trace_dir_byte_limit,
        block_verification_method: config.block_verification_method.clone(),
        block_production_method: config.block_production_method.clone(),
        generator_config: config.generator_config.clone(),
        use_snapshot_archives_at_startup: config.use_snapshot_archives_at_startup,
    }
}

pub fn make_identical_validator_configs(
    config: &ValidatorConfig,
    num: usize,
) -> Vec<ValidatorConfig> {
    let mut configs = vec![];
    for _ in 0..num {
        configs.push(safe_clone_config(config));
    }
    configs
}
