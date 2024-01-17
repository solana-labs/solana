use {
    clap::{crate_name, value_t, value_t_or_exit, values_t_or_exit},
    crossbeam_channel::unbounded,
    itertools::Itertools,
    log::*,
    solana_accounts_db::accounts_index::{AccountIndex, AccountSecondaryIndexes},
    solana_clap_utils::{
        input_parsers::{pubkey_of, pubkeys_of, value_of},
        input_validators::normalize_to_url_if_moniker,
    },
    solana_core::consensus::tower_storage::FileTowerStorage,
    solana_faucet::faucet::run_local_faucet_with_port,
    solana_rpc::{
        rpc::{JsonRpcConfig, RpcBigtableConfig},
        rpc_pubsub_service::PubSubConfig,
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        account::AccountSharedData,
        clock::Slot,
        epoch_schedule::EpochSchedule,
        feature_set,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
        system_program,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::*,
    solana_validator::{
        admin_rpc_service, cli, dashboard::Dashboard, ledger_lockfile, lock_ledger,
        println_name_value, redirect_stderr_to_file,
    },
    std::{
        collections::HashSet,
        fs, io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::{Path, PathBuf},
        process::exit,
        sync::{Arc, RwLock},
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

#[derive(PartialEq, Eq)]
enum Output {
    None,
    Log,
    Dashboard,
}

fn main() {
    let default_args = cli::DefaultTestArgs::new();
    let version = solana_version::version!();
    let matches = cli::test_app(version, &default_args).get_matches();

    let output = if matches.is_present("quiet") {
        Output::None
    } else if matches.is_present("log") {
        Output::Log
    } else {
        Output::Dashboard
    };

    let ledger_path = value_t_or_exit!(matches, "ledger_path", PathBuf);
    let reset_ledger = matches.is_present("reset");

    let indexes: HashSet<AccountIndex> = matches
        .values_of("account_indexes")
        .unwrap_or_default()
        .map(|value| match value {
            "program-id" => AccountIndex::ProgramId,
            "spl-token-mint" => AccountIndex::SplTokenMint,
            "spl-token-owner" => AccountIndex::SplTokenOwner,
            _ => unreachable!(),
        })
        .collect();

    let account_indexes = AccountSecondaryIndexes {
        keys: None,
        indexes,
    };

    if !ledger_path.exists() {
        fs::create_dir(&ledger_path).unwrap_or_else(|err| {
            println!(
                "Error: Unable to create directory {}: {}",
                ledger_path.display(),
                err
            );
            exit(1);
        });
    }

    let mut ledger_lock = ledger_lockfile(&ledger_path);
    let _ledger_write_guard = lock_ledger(&ledger_path, &mut ledger_lock);
    if reset_ledger {
        remove_directory_contents(&ledger_path).unwrap_or_else(|err| {
            println!("Error: Unable to remove {}: {}", ledger_path.display(), err);
            exit(1);
        })
    }
    solana_runtime::snapshot_utils::remove_tmp_snapshot_archives(&ledger_path);

    let validator_log_symlink = ledger_path.join("validator.log");

    let logfile = if output != Output::Log {
        let validator_log_with_timestamp = format!(
            "validator-{}.log",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        let _ = fs::remove_file(&validator_log_symlink);
        symlink::symlink_file(&validator_log_with_timestamp, &validator_log_symlink).unwrap();

        Some(
            ledger_path
                .join(validator_log_with_timestamp)
                .into_os_string()
                .into_string()
                .unwrap(),
        )
    } else {
        None
    };
    let _logger_thread = redirect_stderr_to_file(logfile);

    info!("{} {}", crate_name!(), solana_version::version!());
    info!("Starting validator with: {:#?}", std::env::args_os());
    solana_core::validator::report_target_features();

    // TODO: Ideally test-validator should *only* allow private addresses.
    let socket_addr_space = SocketAddrSpace::new(/*allow_private_addr=*/ true);
    let cli_config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let cluster_rpc_client = value_t!(matches, "json_rpc_url", String)
        .map(normalize_to_url_if_moniker)
        .map(RpcClient::new);

    let (mint_address, random_mint) = pubkey_of(&matches, "mint_address")
        .map(|pk| (pk, false))
        .unwrap_or_else(|| {
            read_keypair_file(&cli_config.keypair_path)
                .map(|kp| (kp.pubkey(), false))
                .unwrap_or_else(|_| (Keypair::new().pubkey(), true))
        });

    let rpc_port = value_t_or_exit!(matches, "rpc_port", u16);
    let enable_vote_subscription = matches.is_present("rpc_pubsub_enable_vote_subscription");
    let enable_block_subscription = matches.is_present("rpc_pubsub_enable_block_subscription");
    let faucet_port = value_t_or_exit!(matches, "faucet_port", u16);
    let ticks_per_slot = value_t!(matches, "ticks_per_slot", u64).ok();
    let slots_per_epoch = value_t!(matches, "slots_per_epoch", Slot).ok();
    let gossip_host = matches.value_of("gossip_host").map(|gossip_host| {
        solana_net_utils::parse_host(gossip_host).unwrap_or_else(|err| {
            eprintln!("Failed to parse --gossip-host: {err}");
            exit(1);
        })
    });
    let gossip_port = value_t!(matches, "gossip_port", u16).ok();
    let dynamic_port_range = matches.value_of("dynamic_port_range").map(|port_range| {
        solana_net_utils::parse_port_range(port_range).unwrap_or_else(|| {
            eprintln!("Failed to parse --dynamic-port-range");
            exit(1);
        })
    });
    let bind_address = matches.value_of("bind_address").map(|bind_address| {
        solana_net_utils::parse_host(bind_address).unwrap_or_else(|err| {
            eprintln!("Failed to parse --bind-address: {err}");
            exit(1);
        })
    });
    let compute_unit_limit = value_t!(matches, "compute_unit_limit", u64).ok();

    let faucet_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), faucet_port);

    let parse_address = |address: &str, input_type: &str| {
        address
            .parse::<Pubkey>()
            .or_else(|_| read_keypair_file(address).map(|keypair| keypair.pubkey()))
            .unwrap_or_else(|err| {
                println!("Error: invalid {input_type} {address}: {err}");
                exit(1);
            })
    };

    let parse_program_path = |program: &str| {
        let program_path = PathBuf::from(program);
        if !program_path.exists() {
            println!(
                "Error: program file does not exist: {}",
                program_path.display()
            );
            exit(1);
        }
        program_path
    };

    let mut upgradeable_programs_to_load = vec![];
    if let Some(values) = matches.values_of("bpf_program") {
        for (address, program) in values.into_iter().tuples() {
            let address = parse_address(address, "address");
            let program_path = parse_program_path(program);

            upgradeable_programs_to_load.push(UpgradeableProgramInfo {
                program_id: address,
                loader: solana_sdk::bpf_loader_upgradeable::id(),
                upgrade_authority: Pubkey::default(),
                program_path,
            });
        }
    }

    if let Some(values) = matches.values_of("upgradeable_program") {
        for (address, program, upgrade_authority) in
            values.into_iter().tuples::<(&str, &str, &str)>()
        {
            let address = parse_address(address, "address");
            let program_path = parse_program_path(program);
            let upgrade_authority_address = if upgrade_authority == "none" {
                Pubkey::default()
            } else {
                upgrade_authority
                    .parse::<Pubkey>()
                    .or_else(|_| {
                        read_keypair_file(upgrade_authority).map(|keypair| keypair.pubkey())
                    })
                    .unwrap_or_else(|err| {
                        println!("Error: invalid upgrade_authority {upgrade_authority}: {err}");
                        exit(1);
                    })
            };

            upgradeable_programs_to_load.push(UpgradeableProgramInfo {
                program_id: address,
                loader: solana_sdk::bpf_loader_upgradeable::id(),
                upgrade_authority: upgrade_authority_address,
                program_path,
            });
        }
    }

    let mut accounts_to_load = vec![];
    if let Some(values) = matches.values_of("account") {
        for (address, filename) in values.into_iter().tuples() {
            let address = if address == "-" {
                None
            } else {
                Some(address.parse::<Pubkey>().unwrap_or_else(|err| {
                    println!("Error: invalid address {address}: {err}");
                    exit(1);
                }))
            };

            accounts_to_load.push(AccountInfo { address, filename });
        }
    }

    let accounts_from_dirs: HashSet<_> = matches
        .values_of("account_dir")
        .unwrap_or_default()
        .collect();

    let accounts_to_clone: HashSet<_> = pubkeys_of(&matches, "clone_account")
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    let accounts_to_maybe_clone: HashSet<_> = pubkeys_of(&matches, "maybe_clone_account")
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    let upgradeable_programs_to_clone: HashSet<_> =
        pubkeys_of(&matches, "clone_upgradeable_program")
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();

    let warp_slot = if matches.is_present("warp_slot") {
        Some(match matches.value_of("warp_slot") {
            Some(_) => value_t_or_exit!(matches, "warp_slot", Slot),
            None => {
                cluster_rpc_client.as_ref().unwrap_or_else(|_| {
                        println!("The --url argument must be provided if --warp-slot/-w is used without an explicit slot");
                        exit(1);

                }).get_slot()
                    .unwrap_or_else(|err| {
                        println!("Unable to get current cluster slot: {err}");
                        exit(1);
                    })
            }
        })
    } else {
        None
    };

    let faucet_lamports = sol_to_lamports(value_of(&matches, "faucet_sol").unwrap());
    let faucet_keypair_file = ledger_path.join("faucet-keypair.json");
    if !faucet_keypair_file.exists() {
        write_keypair_file(&Keypair::new(), faucet_keypair_file.to_str().unwrap()).unwrap_or_else(
            |err| {
                println!(
                    "Error: Failed to write {}: {}",
                    faucet_keypair_file.display(),
                    err
                );
                exit(1);
            },
        );
    }

    let faucet_keypair =
        read_keypair_file(faucet_keypair_file.to_str().unwrap()).unwrap_or_else(|err| {
            println!(
                "Error: Failed to read {}: {}",
                faucet_keypair_file.display(),
                err
            );
            exit(1);
        });
    let faucet_pubkey = faucet_keypair.pubkey();

    let faucet_time_slice_secs = value_t_or_exit!(matches, "faucet_time_slice_secs", u64);
    let faucet_per_time_cap = value_t!(matches, "faucet_per_time_sol_cap", f64)
        .ok()
        .map(sol_to_lamports);
    let faucet_per_request_cap = value_t!(matches, "faucet_per_request_sol_cap", f64)
        .ok()
        .map(sol_to_lamports);

    let (sender, receiver) = unbounded();
    run_local_faucet_with_port(
        faucet_keypair,
        sender,
        Some(faucet_time_slice_secs),
        faucet_per_time_cap,
        faucet_per_request_cap,
        faucet_addr.port(),
    );
    let _ = receiver.recv().expect("run faucet").unwrap_or_else(|err| {
        println!("Error: failed to start faucet: {err}");
        exit(1);
    });

    let mut features_to_deactivate = pubkeys_of(&matches, "deactivate_feature").unwrap_or_default();
    // Remove this when client support is ready for the enable_partitioned_epoch_reward feature
    features_to_deactivate.push(feature_set::enable_partitioned_epoch_reward::id());

    if TestValidatorGenesis::ledger_exists(&ledger_path) {
        for (name, long) in &[
            ("bpf_program", "--bpf-program"),
            ("clone_account", "--clone"),
            ("account", "--account"),
            ("mint_address", "--mint"),
            ("ticks_per_slot", "--ticks-per-slot"),
            ("slots_per_epoch", "--slots-per-epoch"),
            ("faucet_sol", "--faucet-sol"),
            ("deactivate_feature", "--deactivate-feature"),
        ] {
            if matches.is_present(name) {
                println!("{long} argument ignored, ledger already exists");
            }
        }
    } else if random_mint {
        println_name_value(
            "\nNotice!",
            "No wallet available. `solana airdrop` localnet SOL after creating one\n",
        );
    }

    let mut genesis = TestValidatorGenesis::default();
    genesis.max_ledger_shreds = value_of(&matches, "limit_ledger_size");
    genesis.max_genesis_archive_unpacked_size = Some(u64::MAX);
    genesis.log_messages_bytes_limit = value_t!(matches, "log_messages_bytes_limit", usize).ok();
    genesis.transaction_account_lock_limit =
        value_t!(matches, "transaction_account_lock_limit", usize).ok();

    let tower_storage = Arc::new(FileTowerStorage::new(ledger_path.clone()));

    let admin_service_post_init = Arc::new(RwLock::new(None));
    // If geyser_plugin_config value is invalid, the validator will exit when the values are extracted below
    let (rpc_to_plugin_manager_sender, rpc_to_plugin_manager_receiver) =
        if matches.is_present("geyser_plugin_config") {
            let (sender, receiver) = unbounded();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
    admin_rpc_service::run(
        &ledger_path,
        admin_rpc_service::AdminRpcRequestMetadata {
            rpc_addr: Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), rpc_port)),
            start_progress: genesis.start_progress.clone(),
            start_time: std::time::SystemTime::now(),
            validator_exit: genesis.validator_exit.clone(),
            authorized_voter_keypairs: genesis.authorized_voter_keypairs.clone(),
            staked_nodes_overrides: genesis.staked_nodes_overrides.clone(),
            post_init: admin_service_post_init,
            tower_storage: tower_storage.clone(),
            rpc_to_plugin_manager_sender,
        },
    );
    let dashboard = if output == Output::Dashboard {
        Some(
            Dashboard::new(
                &ledger_path,
                Some(&validator_log_symlink),
                Some(&mut genesis.validator_exit.write().unwrap()),
            )
            .unwrap(),
        )
    } else {
        None
    };

    let rpc_bigtable_config = if matches.is_present("enable_rpc_bigtable_ledger_storage") {
        Some(RpcBigtableConfig {
            enable_bigtable_ledger_upload: false,
            bigtable_instance_name: value_t_or_exit!(matches, "rpc_bigtable_instance", String),
            bigtable_app_profile_id: value_t_or_exit!(
                matches,
                "rpc_bigtable_app_profile_id",
                String
            ),
            timeout: None,
            ..RpcBigtableConfig::default()
        })
    } else {
        None
    };

    genesis
        .ledger_path(&ledger_path)
        .tower_storage(tower_storage)
        .add_account(
            faucet_pubkey,
            AccountSharedData::new(faucet_lamports, 0, &system_program::id()),
        )
        .pubsub_config(PubSubConfig {
            enable_vote_subscription,
            enable_block_subscription,
            ..PubSubConfig::default()
        })
        .rpc_port(rpc_port)
        .add_upgradeable_programs_with_path(&upgradeable_programs_to_load)
        .add_accounts_from_json_files(&accounts_to_load)
        .unwrap_or_else(|e| {
            println!("Error: add_accounts_from_json_files failed: {e}");
            exit(1);
        })
        .add_accounts_from_directories(&accounts_from_dirs)
        .unwrap_or_else(|e| {
            println!("Error: add_accounts_from_directories failed: {e}");
            exit(1);
        })
        .deactivate_features(&features_to_deactivate);

    genesis.rpc_config(JsonRpcConfig {
        enable_rpc_transaction_history: true,
        enable_extended_tx_metadata_storage: true,
        rpc_bigtable_config,
        faucet_addr: Some(faucet_addr),
        account_indexes,
        ..JsonRpcConfig::default_for_test()
    });

    if !accounts_to_clone.is_empty() {
        if let Err(e) = genesis.clone_accounts(
            accounts_to_clone,
            cluster_rpc_client
                .as_ref()
                .expect("bug: --url argument missing?"),
            false,
        ) {
            println!("Error: clone_accounts failed: {e}");
            exit(1);
        }
    }

    if !accounts_to_maybe_clone.is_empty() {
        if let Err(e) = genesis.clone_accounts(
            accounts_to_maybe_clone,
            cluster_rpc_client
                .as_ref()
                .expect("bug: --url argument missing?"),
            true,
        ) {
            println!("Error: clone_accounts failed: {e}");
            exit(1);
        }
    }

    if !upgradeable_programs_to_clone.is_empty() {
        if let Err(e) = genesis.clone_upgradeable_programs(
            upgradeable_programs_to_clone,
            cluster_rpc_client
                .as_ref()
                .expect("bug: --url argument missing?"),
        ) {
            println!("Error: clone_upgradeable_programs failed: {e}");
            exit(1);
        }
    }

    if let Some(warp_slot) = warp_slot {
        genesis.warp_slot(warp_slot);
    }

    if let Some(ticks_per_slot) = ticks_per_slot {
        genesis.ticks_per_slot(ticks_per_slot);
    }

    if let Some(slots_per_epoch) = slots_per_epoch {
        genesis.epoch_schedule(EpochSchedule::custom(
            slots_per_epoch,
            slots_per_epoch,
            /* enable_warmup_epochs = */ false,
        ));

        genesis.rent = Rent::with_slots_per_epoch(slots_per_epoch);
    }

    if let Some(gossip_host) = gossip_host {
        genesis.gossip_host(gossip_host);
    }

    if let Some(gossip_port) = gossip_port {
        genesis.gossip_port(gossip_port);
    }

    if let Some(dynamic_port_range) = dynamic_port_range {
        genesis.port_range(dynamic_port_range);
    }

    if let Some(bind_address) = bind_address {
        genesis.bind_ip_addr(bind_address);
    }

    if matches.is_present("geyser_plugin_config") {
        genesis.geyser_plugin_config_files = Some(
            values_t_or_exit!(matches, "geyser_plugin_config", String)
                .into_iter()
                .map(PathBuf::from)
                .collect(),
        );
    }

    if let Some(compute_unit_limit) = compute_unit_limit {
        genesis.compute_unit_limit(compute_unit_limit);
    }

    match genesis.start_with_mint_address_and_geyser_plugin_rpc(
        mint_address,
        socket_addr_space,
        rpc_to_plugin_manager_receiver,
    ) {
        Ok(test_validator) => {
            if let Some(dashboard) = dashboard {
                dashboard.run(Duration::from_millis(250));
            }
            test_validator.join();
        }
        Err(err) => {
            drop(dashboard);
            println!("Error: failed to start validator: {err}");
            exit(1);
        }
    }
}

fn remove_directory_contents(ledger_path: &Path) -> Result<(), io::Error> {
    for entry in fs::read_dir(ledger_path)? {
        let entry = entry?;
        if entry.metadata()?.is_dir() {
            fs::remove_dir_all(entry.path())?
        } else {
            fs::remove_file(entry.path())?
        }
    }
    Ok(())
}
