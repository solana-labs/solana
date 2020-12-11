use {
    clap::{value_t_or_exit, App, Arg},
    console::style,
    indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle},
    solana_clap_utils::{input_parsers::pubkey_of, input_validators::is_pubkey},
    solana_client::{client_error, rpc_client::RpcClient},
    solana_core::rpc::JsonRpcConfig,
    solana_sdk::{
        clock::{Slot, DEFAULT_TICKS_PER_SLOT, MS_PER_TICK},
        commitment_config::CommitmentConfig,
        fee_calculator::FeeRateGovernor,
        rent::Rent,
        rpc_port,
        signature::{read_keypair_file, Signer},
    },
    solana_validator::{start_logger, test_validator::*},
    std::{
        path::PathBuf,
        process::exit,
        thread::sleep,
        time::{Duration, SystemTime, UNIX_EPOCH},
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

    let matches = App::new("solana-test-validator").about("Test Validator")
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
            Arg::with_name("mint_address")
                .long("mint")
                .value_name("PUBKEY")
                .validator(is_pubkey)
                .takes_value(true)
                .help("Address of the mint account that will receive all the initial tokens [default: client keypair]"),
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
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .takes_value(false)
                .conflicts_with("log")
                .help("Quiet mode: suppress normal output")
        )
        .arg(
            Arg::with_name("log")
                .long("log")
                .takes_value(false)
                .conflicts_with("quiet")
                .help("Log mode: stream the validator log")
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
        .get_matches();

    let cli_config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let mint_address = pubkey_of(&matches, "mint_address").unwrap_or_else(|| {
        read_keypair_file(&cli_config.keypair_path)
            .unwrap_or_else(|err| {
                eprintln!(
                    "Error: Unable to read keypair file {}: {}",
                    cli_config.keypair_path, err
                );
                exit(1);
            })
            .pubkey()
    });

    let ledger_path = value_t_or_exit!(matches, "ledger_path", PathBuf);
    let output = if matches.is_present("quiet") {
        Output::None
    } else if matches.is_present("log") {
        Output::Log
    } else {
        Output::Dashboard
    };

    let rpc_ports = {
        let rpc_port = value_t_or_exit!(matches, "rpc_port", u16);
        (rpc_port, rpc_port + 1)
    };

    if !ledger_path.exists() {
        let _progress_bar = if output == Output::Dashboard {
            println_name_value("Mint address:", &mint_address.to_string());
            let progress_bar = new_spinner_progress_bar();
            progress_bar.set_message("Creating ledger...");
            Some(progress_bar)
        } else {
            None
        };

        TestValidator::initialize_ledger(
            Some(&ledger_path),
            TestValidatorGenesisConfig {
                mint_address,
                fee_rate_governor: FeeRateGovernor::default(),
                rent: Rent::default(),
            },
        )
        .unwrap_or_else(|err| {
            eprintln!(
                "Error: failed to initialize ledger at {}: {}",
                ledger_path.display(),
                err
            );
            exit(1);
        });
    }

    let validator_log_symlink = ledger_path.join("validator.log");
    let logfile = if output != Output::Log {
        let validator_log_with_timestamp = format!(
            "validator-{}.log",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        let _ = std::fs::remove_file(&validator_log_symlink);
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

    let test_validator = {
        let _progress_bar = if output == Output::Dashboard {
            println_name_value("Ledger location:", &format!("{}", ledger_path.display()));
            println_name_value("Log:", &format!("{}", validator_log_symlink.display()));
            let progress_bar = new_spinner_progress_bar();
            progress_bar.set_message("Initializing...");
            Some(progress_bar)
        } else {
            None
        };

        TestValidator::start(
            &ledger_path,
            TestValidatorStartConfig {
                preserve_ledger: true,
                rpc_config: JsonRpcConfig {
                    enable_validator_exit: true,
                    enable_rpc_transaction_history: true,
                    ..JsonRpcConfig::default()
                },
                rpc_ports: Some(rpc_ports),
            },
        )
    }
    .unwrap_or_else(|err| {
        eprintln!("Error: failed to start validator: {}", err);
        exit(1);
    });

    if output == Output::Dashboard {
        println_name_value("JSON RPC URL:", &test_validator.rpc_url());
        println_name_value(
            "JSON RPC PubSub Websocket:",
            &test_validator.rpc_pubsub_url(),
        );
        println_name_value("Gossip Address:", &test_validator.gossip().to_string());
        println_name_value("TPU Address:", &test_validator.tpu().to_string());

        let progress_bar = new_spinner_progress_bar();
        let rpc_client = RpcClient::new(test_validator.rpc_url());

        fn get_validator_stats(rpc_client: &RpcClient) -> client_error::Result<(Slot, Slot, u64)> {
            let max_slot = rpc_client.get_slot_with_commitment(CommitmentConfig::max())?;
            let recent_slot = rpc_client.get_slot_with_commitment(CommitmentConfig::recent())?;
            let transaction_count =
                rpc_client.get_transaction_count_with_commitment(CommitmentConfig::recent())?;
            Ok((recent_slot, max_slot, transaction_count))
        }

        loop {
            match get_validator_stats(&rpc_client) {
                Ok((recent_slot, max_slot, transaction_count)) => {
                    progress_bar.set_message(&format!(
                        "Recent slot: {} | Max confirmed slot: {} | Transaction count: {}",
                        recent_slot, max_slot, transaction_count,
                    ));
                }
                Err(err) => {
                    progress_bar.set_message(&format!("{}", err));
                }
            }
            sleep(Duration::from_millis(
                MS_PER_TICK * DEFAULT_TICKS_PER_SLOT / 2,
            ));
        }
    }

    std::thread::park();
}
