use {
    clap::{value_t, value_t_or_exit, App, Arg},
    console::style,
    fd_lock::FdLock,
    indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle},
    solana_clap_utils::{
        input_parsers::{pubkey_of, pubkeys_of},
        input_validators::{
            is_pubkey, is_pubkey_or_keypair, is_slot, is_url_or_moniker,
            normalize_to_url_if_moniker,
        },
    },
    solana_client::{client_error, rpc_client::RpcClient, rpc_request},
    solana_core::rpc::JsonRpcConfig,
    solana_faucet::faucet::{run_local_faucet_with_port, FAUCET_PORT},
    solana_sdk::{
        account::Account,
        clock::{Slot, DEFAULT_TICKS_PER_SLOT, MS_PER_TICK},
        commitment_config::CommitmentConfig,
        native_token::{sol_to_lamports, Sol},
        pubkey::Pubkey,
        rpc_port,
        signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
        system_program,
    },
    solana_validator::{start_logger, test_validator::*},
    std::{
        collections::HashSet,
        fs, io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::{Path, PathBuf},
        process::exit,
        sync::mpsc::channel,
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    },
};

#[derive(PartialEq)]
enum Output {
    None,
    Log,
    Dashboard,
}

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_draw_target(ProgressDrawTarget::stdout());
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

/// Pretty print a "name value"
fn println_name_value(name: &str, value: &str) {
    println!("{} {}", style(name).bold(), value);
}

fn main() {
    let default_rpc_port = rpc_port::DEFAULT_RPC_PORT.to_string();

    let matches = App::new("solana-test-validator")
        .about("Test Validator")
        .version(solana_version::version!())
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *solana_cli_config::CONFIG_FILE {
                arg.default_value(&config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .short("u")
                .long("url")
                .value_name("URL_OR_MONIKER")
                .takes_value(true)
                .validator(is_url_or_moniker)
                .help(
                    "URL for Solana's JSON RPC or moniker (or their first letter): \
                   [mainnet-beta, testnet, devnet, localhost]",
                ),
        )
        .arg(
            Arg::with_name("mint_address")
                .long("mint")
                .value_name("PUBKEY")
                .validator(is_pubkey)
                .takes_value(true)
                .help(
                    "Address of the mint account that will receive tokens \
                       created at genesis.  If the ledger already exists then \
                       this parameter is silently ignored [default: client keypair]",
                ),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .default_value("test-ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("reset")
                .short("r")
                .long("reset")
                .takes_value(false)
                .help(
                    "Reset the ledger to genesis if it exists. \
                       By default the validator will resume an existing ledger (if present)",
                ),
        )
        .arg(
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .takes_value(false)
                .conflicts_with("log")
                .help("Quiet mode: suppress normal output"),
        )
        .arg(
            Arg::with_name("log")
                .long("log")
                .takes_value(false)
                .conflicts_with("quiet")
                .help("Log mode: stream the validator log"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .default_value(&default_rpc_port)
                .validator(solana_validator::port_validator)
                .help("Use this port for JSON RPC and the next port for the RPC websocket"),
        )
        .arg(
            Arg::with_name("bpf_program")
                .long("bpf-program")
                .value_name("ADDRESS BPF_PROGRAM.SO")
                .takes_value(true)
                .number_of_values(2)
                .multiple(true)
                .help(
                    "Add a BPF program to the genesis configuration. \
                       If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("clone_account")
                .long("clone")
                .short("c")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .multiple(true)
                .requires("json_rpc_url")
                .help(
                    "Copy an account from the cluster referenced by the --url argument the \
                     genesis configuration. \
                     If the ledger already exists then this parameter is silently ignored",
                ),
        )
        .arg(
            Arg::with_name("warp_slot")
                .required(false)
                .long("warp-slot")
                .short("w")
                .takes_value(true)
                .value_name("WARP_SLOT")
                .validator(is_slot)
                .min_values(0)
                .max_values(1)
                .help(
                    "Warp the ledger to WARP_SLOT after starting the validator. \
                        If no slot is provided then the current slot of the cluster \
                        referenced by the --url argument will be used",
                ),
        )
        .get_matches();

    let cli_config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let cluster_rpc_client = value_t!(matches, "json_rpc_url", String)
        .map(normalize_to_url_if_moniker)
        .map(RpcClient::new);

    let mint_address = pubkey_of(&matches, "mint_address").unwrap_or_else(|| {
        read_keypair_file(&cli_config.keypair_path)
            .unwrap_or_else(|_| Keypair::new())
            .pubkey()
    });

    let ledger_path = value_t_or_exit!(matches, "ledger_path", PathBuf);
    let reset_ledger = matches.is_present("reset");
    let output = if matches.is_present("quiet") {
        Output::None
    } else if matches.is_present("log") {
        Output::Log
    } else {
        Output::Dashboard
    };
    let rpc_port = value_t_or_exit!(matches, "rpc_port", u16);
    let faucet_addr = Some(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        FAUCET_PORT,
    ));

    let mut programs = vec![];
    if let Some(values) = matches.values_of("bpf_program") {
        let values: Vec<&str> = values.collect::<Vec<_>>();
        for address_program in values.chunks(2) {
            match address_program {
                [address, program] => {
                    let address = address.parse::<Pubkey>().unwrap_or_else(|err| {
                        eprintln!("Error: invalid address {}: {}", address, err);
                        exit(1);
                    });

                    let program_path = PathBuf::from(program);
                    if !program_path.exists() {
                        eprintln!(
                            "Error: program file does not exist: {}",
                            program_path.display()
                        );
                        exit(1);
                    }

                    programs.push(ProgramInfo {
                        program_id: address,
                        loader: solana_sdk::bpf_loader::id(),
                        program_path,
                    });
                }
                _ => unreachable!(),
            }
        }
    }

    let clone_accounts: HashSet<_> = pubkeys_of(&matches, "clone_account")
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    let warp_slot = if matches.is_present("warp_slot") {
        Some(match matches.value_of("warp_slot") {
            Some(_) => value_t_or_exit!(matches, "warp_slot", Slot),
            None => {
                cluster_rpc_client.as_ref().unwrap_or_else(|_| {
                        eprintln!("The --url argument must be provided if --warp-slot/-w is used without an explicit slot");
                        exit(1);

                }).get_slot()
                    .unwrap_or_else(|err| {
                        eprintln!("Unable to get current cluster slot: {}", err);
                        exit(1);
                    })
            }
        })
    } else {
        None
    };

    if !ledger_path.exists() {
        fs::create_dir(&ledger_path).unwrap_or_else(|err| {
            eprintln!(
                "Error: Unable to create directory {}: {}",
                ledger_path.display(),
                err
            );
            exit(1);
        });
    }

    let mut ledger_fd_lock = FdLock::new(fs::File::open(&ledger_path).unwrap());
    let _ledger_lock = ledger_fd_lock.try_lock().unwrap_or_else(|_| {
        eprintln!(
            "Error: Unable to lock {} directory. Check if another solana-test-validator is running",
            ledger_path.display()
        );
        exit(1);
    });

    if reset_ledger {
        remove_directory_contents(&ledger_path).unwrap_or_else(|err| {
            eprintln!("Error: Unable to remove {}: {}", ledger_path.display(), err);
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
    let _logger_thread = start_logger(logfile);

    let faucet_lamports = sol_to_lamports(1_000_000.);
    let faucet_keypair_file = ledger_path.join("faucet-keypair.json");
    if !faucet_keypair_file.exists() {
        write_keypair_file(&Keypair::new(), faucet_keypair_file.to_str().unwrap()).unwrap_or_else(
            |err| {
                eprintln!(
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
            eprintln!(
                "Error: Failed to read {}: {}",
                faucet_keypair_file.display(),
                err
            );
            exit(1);
        });

    let validator_start = Instant::now();

    let test_validator = {
        let _progress_bar = if output == Output::Dashboard {
            println_name_value("Mint address:", &mint_address.to_string());
            println_name_value("Ledger location:", &format!("{}", ledger_path.display()));
            println_name_value("Log:", &format!("{}", validator_log_symlink.display()));
            let progress_bar = new_spinner_progress_bar();
            progress_bar.set_message("Initializing...");
            Some(progress_bar)
        } else {
            None
        };

        let mut genesis = TestValidatorGenesis::default();
        genesis
            .ledger_path(&ledger_path)
            .add_account(
                faucet_keypair.pubkey(),
                Account::new(faucet_lamports, 0, &system_program::id()),
            )
            .rpc_config(JsonRpcConfig {
                enable_validator_exit: true,
                enable_rpc_transaction_history: true,
                faucet_addr,
                ..JsonRpcConfig::default()
            })
            .rpc_port(rpc_port)
            .add_programs_with_path(&programs);

        if !clone_accounts.is_empty() {
            genesis.clone_accounts(
                clone_accounts,
                cluster_rpc_client
                    .as_ref()
                    .expect("bug: --url argument missing?"),
            );
        }

        if let Some(warp_slot) = warp_slot {
            genesis.warp_slot(warp_slot);
        }
        genesis.start_with_mint_address(mint_address)
    }
    .unwrap_or_else(|err| {
        eprintln!("Error: failed to start validator: {}", err);
        exit(1);
    });

    if let Some(faucet_addr) = &faucet_addr {
        let (sender, receiver) = channel();
        run_local_faucet_with_port(faucet_keypair, sender, None, faucet_addr.port());
        receiver.recv().expect("run faucet");
    }

    if output == Output::Dashboard {
        let rpc_client = test_validator.rpc_client().0;
        let identity = &rpc_client.get_identity().expect("get_identity");
        println_name_value("Identity:", &identity.to_string());
        println_name_value(
            "Version:",
            &rpc_client.get_version().expect("get_version").solana_core,
        );
        println_name_value("JSON RPC URL:", &test_validator.rpc_url());
        println_name_value(
            "JSON RPC PubSub Websocket:",
            &test_validator.rpc_pubsub_url(),
        );
        println_name_value("Gossip Address:", &test_validator.gossip().to_string());
        println_name_value("TPU Address:", &test_validator.tpu().to_string());
        if let Some(faucet_addr) = &faucet_addr {
            println_name_value(
                "Faucet Address:",
                &format!("{}:{}", &test_validator.gossip().ip(), faucet_addr.port()),
            );
        }

        let progress_bar = new_spinner_progress_bar();

        fn get_validator_stats(
            rpc_client: &RpcClient,
            identity: &Pubkey,
        ) -> client_error::Result<(Slot, Slot, Slot, u64, Sol, String)> {
            let processed_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::processed())?;
            let confirmed_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::confirmed())?;
            let finalized_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::finalized())?;
            let transaction_count =
                rpc_client.get_transaction_count_with_commitment(CommitmentConfig::processed())?;
            let identity_balance = rpc_client
                .get_balance_with_commitment(identity, CommitmentConfig::confirmed())?
                .value;

            let health = match rpc_client.get_health() {
                Ok(()) => "ok".to_string(),
                Err(err) => {
                    if let client_error::ClientErrorKind::RpcError(
                        rpc_request::RpcError::RpcResponseError {
                            code: _,
                            message: _,
                            data:
                                rpc_request::RpcResponseErrorData::NodeUnhealthy {
                                    num_slots_behind: Some(num_slots_behind),
                                },
                        },
                    ) = &err.kind
                    {
                        format!("{} slots behind", num_slots_behind)
                    } else {
                        "unhealthy".to_string()
                    }
                }
            };

            Ok((
                processed_slot,
                confirmed_slot,
                finalized_slot,
                transaction_count,
                Sol(identity_balance),
                health,
            ))
        }

        loop {
            let snapshot_slot = rpc_client.get_snapshot_slot().ok();

            for _i in 0..10 {
                match get_validator_stats(&rpc_client, &identity) {
                    Ok((
                        processed_slot,
                        confirmed_slot,
                        finalized_slot,
                        transaction_count,
                        identity_balance,
                        health,
                    )) => {
                        let uptime = chrono::Duration::from_std(validator_start.elapsed()).unwrap();

                        progress_bar.set_message(&format!(
                            "{:02}:{:02}:{:02} \
                            {}| \
                            Processed Slot: {} | Confirmed Slot: {} | Finalized Slot: {} | \
                            Snapshot Slot: {} | \
                            Transactions: {} | {}",
                            uptime.num_hours(),
                            uptime.num_minutes() % 60,
                            uptime.num_seconds() % 60,
                            if health == "ok" {
                                "".to_string()
                            } else {
                                format!("| {} ", style(health).bold().red())
                            },
                            processed_slot,
                            confirmed_slot,
                            finalized_slot,
                            snapshot_slot
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                            transaction_count,
                            identity_balance
                        ));
                    }
                    Err(err) => {
                        progress_bar.set_message(&format!("{}", err));
                    }
                }
                thread::sleep(Duration::from_millis(
                    MS_PER_TICK * DEFAULT_TICKS_PER_SLOT / 2,
                ));
            }
        }
    }

    std::thread::park();
}

fn remove_directory_contents(ledger_path: &Path) -> Result<(), io::Error> {
    for entry in fs::read_dir(&ledger_path)? {
        let entry = entry?;
        if entry.metadata()?.is_file() {
            fs::remove_file(&entry.path())?
        } else {
            fs::remove_dir_all(&entry.path())?
        }
    }
    Ok(())
}
