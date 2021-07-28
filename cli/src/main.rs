use clap::{
    crate_description, crate_name, value_t_or_exit, AppSettings, Arg, ArgGroup, ArgMatches,
    SubCommand,
};
use console::style;
use solana_clap_utils::{
    input_validators::{is_url, is_url_or_moniker, normalize_to_url_if_moniker},
    keypair::{CliSigners, DefaultSigner, SKIP_SEED_PHRASE_VALIDATION_ARG},
    DisplayError,
};
use solana_cli::cli::{
    app, parse_command, process_command, CliCommandInfo, CliConfig, SettingType,
    DEFAULT_CONFIRM_TX_TIMEOUT_SECONDS, DEFAULT_RPC_TIMEOUT_SECONDS,
};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_cli_output::{display::println_name_value, OutputFormat};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use std::{collections::HashMap, error, path::PathBuf, sync::Arc, time::Duration};

pub fn println_name_value_or(name: &str, value: &str, setting_type: SettingType) {
    let description = match setting_type {
        SettingType::Explicit => "",
        SettingType::Computed => "(computed)",
        SettingType::SystemDefault => "(default)",
    };

    println!(
        "{} {} {}",
        style(name).bold(),
        style(value),
        style(description).italic(),
    );
}

fn parse_settings(matches: &ArgMatches<'_>) -> Result<bool, Box<dyn error::Error>> {
    let parse_args = match matches.subcommand() {
        ("config", Some(matches)) => {
            let config_file = match matches.value_of("config_file") {
                None => {
                    println!(
                        "{} Either provide the `--config` arg or ensure home directory exists to use the default config location",
                        style("No config file found.").bold()
                    );
                    return Ok(false);
                }
                Some(config_file) => config_file,
            };
            let mut config = Config::load(config_file).unwrap_or_default();

            match matches.subcommand() {
                ("get", Some(subcommand_matches)) => {
                    let (url_setting_type, json_rpc_url) =
                        CliConfig::compute_json_rpc_url_setting("", &config.json_rpc_url);
                    let (ws_setting_type, websocket_url) = CliConfig::compute_websocket_url_setting(
                        "",
                        &config.websocket_url,
                        "",
                        &config.json_rpc_url,
                    );
                    let (keypair_setting_type, keypair_path) =
                        CliConfig::compute_keypair_path_setting("", &config.keypair_path);
                    let (commitment_setting_type, commitment) =
                        CliConfig::compute_commitment_config("", &config.commitment);

                    if let Some(field) = subcommand_matches.value_of("specific_setting") {
                        let (field_name, value, setting_type) = match field {
                            "json_rpc_url" => ("RPC URL", json_rpc_url, url_setting_type),
                            "websocket_url" => ("WebSocket URL", websocket_url, ws_setting_type),
                            "keypair" => ("Key Path", keypair_path, keypair_setting_type),
                            "commitment" => (
                                "Commitment",
                                commitment.commitment.to_string(),
                                commitment_setting_type,
                            ),
                            _ => unreachable!(),
                        };
                        println_name_value_or(&format!("{}:", field_name), &value, setting_type);
                    } else {
                        println_name_value("Config File:", config_file);
                        println_name_value_or("RPC URL:", &json_rpc_url, url_setting_type);
                        println_name_value_or("WebSocket URL:", &websocket_url, ws_setting_type);
                        println_name_value_or("Keypair Path:", &keypair_path, keypair_setting_type);
                        println_name_value_or(
                            "Commitment:",
                            &commitment.commitment.to_string(),
                            commitment_setting_type,
                        );
                    }
                }
                ("set", Some(subcommand_matches)) => {
                    if let Some(url) = subcommand_matches.value_of("json_rpc_url") {
                        config.json_rpc_url = normalize_to_url_if_moniker(url);
                        // Revert to a computed `websocket_url` value when `json_rpc_url` is
                        // changed
                        config.websocket_url = "".to_string();
                    }
                    if let Some(url) = subcommand_matches.value_of("websocket_url") {
                        config.websocket_url = url.to_string();
                    }
                    if let Some(keypair) = subcommand_matches.value_of("keypair") {
                        config.keypair_path = keypair.to_string();
                    }
                    if let Some(commitment) = subcommand_matches.value_of("commitment") {
                        config.commitment = commitment.to_string();
                    }

                    config.save(config_file)?;

                    let (url_setting_type, json_rpc_url) =
                        CliConfig::compute_json_rpc_url_setting("", &config.json_rpc_url);
                    let (ws_setting_type, websocket_url) = CliConfig::compute_websocket_url_setting(
                        "",
                        &config.websocket_url,
                        "",
                        &config.json_rpc_url,
                    );
                    let (keypair_setting_type, keypair_path) =
                        CliConfig::compute_keypair_path_setting("", &config.keypair_path);
                    let (commitment_setting_type, commitment) =
                        CliConfig::compute_commitment_config("", &config.commitment);

                    println_name_value("Config File:", config_file);
                    println_name_value_or("RPC URL:", &json_rpc_url, url_setting_type);
                    println_name_value_or("WebSocket URL:", &websocket_url, ws_setting_type);
                    println_name_value_or("Keypair Path:", &keypair_path, keypair_setting_type);
                    println_name_value_or(
                        "Commitment:",
                        &commitment.commitment.to_string(),
                        commitment_setting_type,
                    );
                }
                ("import-address-labels", Some(subcommand_matches)) => {
                    let filename = value_t_or_exit!(subcommand_matches, "filename", PathBuf);
                    config.import_address_labels(&filename)?;
                    config.save(config_file)?;
                    println!("Address labels imported from {:?}", filename);
                }
                ("export-address-labels", Some(subcommand_matches)) => {
                    let filename = value_t_or_exit!(subcommand_matches, "filename", PathBuf);
                    config.export_address_labels(&filename)?;
                    println!("Address labels exported to {:?}", filename);
                }
                _ => unreachable!(),
            }
            false
        }
        _ => true,
    };
    Ok(parse_args)
}

pub fn parse_args<'a>(
    matches: &ArgMatches<'_>,
    mut wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<(CliConfig<'a>, CliSigners), Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };
    let (_, json_rpc_url) = CliConfig::compute_json_rpc_url_setting(
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );

    let rpc_timeout = value_t_or_exit!(matches, "rpc_timeout", u64);
    let rpc_timeout = Duration::from_secs(rpc_timeout);

    let confirm_transaction_initial_timeout =
        value_t_or_exit!(matches, "confirm_transaction_initial_timeout", u64);
    let confirm_transaction_initial_timeout =
        Duration::from_secs(confirm_transaction_initial_timeout);

    let (_, websocket_url) = CliConfig::compute_websocket_url_setting(
        matches.value_of("websocket_url").unwrap_or(""),
        &config.websocket_url,
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );
    let default_signer_arg_name = "keypair".to_string();
    let (_, default_signer_path) = CliConfig::compute_keypair_path_setting(
        matches.value_of(&default_signer_arg_name).unwrap_or(""),
        &config.keypair_path,
    );

    let default_signer = DefaultSigner::new(default_signer_arg_name, &default_signer_path);

    let CliCommandInfo {
        command,
        mut signers,
    } = parse_command(matches, &default_signer, &mut wallet_manager)?;

    if signers.is_empty() {
        if let Ok(signer_info) =
            default_signer.generate_unique_signers(vec![None], matches, &mut wallet_manager)
        {
            signers.extend(signer_info.signers);
        }
    }

    let verbose = matches.is_present("verbose");
    let output_format = OutputFormat::from_matches(matches, "output_format", verbose);

    let (_, commitment) = CliConfig::compute_commitment_config(
        matches.value_of("commitment").unwrap_or(""),
        &config.commitment,
    );

    let address_labels = if matches.is_present("no_address_labels") {
        HashMap::new()
    } else {
        config.address_labels
    };

    Ok((
        CliConfig {
            command,
            json_rpc_url,
            websocket_url,
            signers: vec![],
            keypair_path: default_signer_path,
            rpc_client: None,
            rpc_timeout,
            verbose,
            output_format,
            commitment,
            send_transaction_config: RpcSendTransactionConfig {
                preflight_commitment: Some(commitment.commitment),
                ..RpcSendTransactionConfig::default()
            },
            confirm_transaction_initial_timeout,
            address_labels,
        },
        signers,
    ))
}

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup_with_default("off");
    let matches = app(
        crate_name!(),
        crate_description!(),
        solana_version::version!(),
    )
    .arg({
        let arg = Arg::with_name("config_file")
            .short("C")
            .long("config")
            .value_name("FILEPATH")
            .takes_value(true)
            .global(true)
            .help("Configuration file to use");
        if let Some(ref config_file) = *CONFIG_FILE {
            arg.default_value(config_file)
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
            .global(true)
            .validator(is_url_or_moniker)
            .help(
                "URL for Solana's JSON RPC or moniker (or their first letter): \
                   [mainnet-beta, testnet, devnet, localhost]",
            ),
    )
    .arg(
        Arg::with_name("websocket_url")
            .long("ws")
            .value_name("URL")
            .takes_value(true)
            .global(true)
            .validator(is_url)
            .help("WebSocket URL for the solana cluster"),
    )
    .arg(
        Arg::with_name("keypair")
            .short("k")
            .long("keypair")
            .value_name("KEYPAIR")
            .global(true)
            .takes_value(true)
            .help("Filepath or URL to a keypair"),
    )
    .arg(
        Arg::with_name("commitment")
            .long("commitment")
            .takes_value(true)
            .possible_values(&[
                "processed",
                "confirmed",
                "finalized",
                "recent", // Deprecated as of v1.5.5
                "single", // Deprecated as of v1.5.5
                "singleGossip", // Deprecated as of v1.5.5
                "root", // Deprecated as of v1.5.5
                "max", // Deprecated as of v1.5.5
            ])
            .value_name("COMMITMENT_LEVEL")
            .hide_possible_values(true)
            .global(true)
            .help("Return information at the selected commitment level [possible values: processed, confirmed, finalized]"),
    )
    .arg(
        Arg::with_name("verbose")
            .long("verbose")
            .short("v")
            .global(true)
            .help("Show additional information"),
    )
    .arg(
        Arg::with_name("no_address_labels")
            .long("no-address-labels")
            .global(true)
            .help("Do not use address labels in the output"),
    )
    .arg(
        Arg::with_name("output_format")
            .long("output")
            .value_name("FORMAT")
            .global(true)
            .takes_value(true)
            .possible_values(&["json", "json-compact"])
            .help("Return information in specified output format"),
    )
    .arg(
        Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
            .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
            .global(true)
            .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
    )
    .arg(
        Arg::with_name("rpc_timeout")
            .long("rpc-timeout")
            .value_name("SECONDS")
            .takes_value(true)
            .default_value(DEFAULT_RPC_TIMEOUT_SECONDS)
            .global(true)
            .hidden(true)
            .help("Timeout value for RPC requests"),
    )
    .arg(
        Arg::with_name("confirm_transaction_initial_timeout")
            .long("confirm-timeout")
            .value_name("SECONDS")
            .takes_value(true)
            .default_value(DEFAULT_CONFIRM_TX_TIMEOUT_SECONDS)
            .global(true)
            .hidden(true)
            .help("Timeout value for initial transaction status"),
    )
    .subcommand(
        SubCommand::with_name("config")
            .about("Solana command-line tool configuration settings")
            .aliases(&["get", "set"])
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .subcommand(
                SubCommand::with_name("get")
                    .about("Get current config settings")
                    .arg(
                        Arg::with_name("specific_setting")
                            .index(1)
                            .value_name("CONFIG_FIELD")
                            .takes_value(true)
                            .possible_values(&[
                                "json_rpc_url",
                                "websocket_url",
                                "keypair",
                                "commitment",
                            ])
                            .help("Return a specific config setting"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("set")
                    .about("Set a config setting")
                    .group(
                        ArgGroup::with_name("config_settings")
                            .args(&["json_rpc_url", "websocket_url", "keypair", "commitment"])
                            .multiple(true)
                            .required(true),
                    ),
            )
            .subcommand(
                SubCommand::with_name("import-address-labels")
                    .about("Import a list of address labels")
                    .arg(
                        Arg::with_name("filename")
                            .index(1)
                            .value_name("FILENAME")
                            .takes_value(true)
                            .help("YAML file of address labels"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("export-address-labels")
                    .about("Export the current address labels")
                    .arg(
                        Arg::with_name("filename")
                            .index(1)
                            .value_name("FILENAME")
                            .takes_value(true)
                            .help("YAML file to receive the current address labels"),
                    ),
            ),
    )
    .get_matches();

    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(matches: &ArgMatches<'_>) -> Result<(), Box<dyn error::Error>> {
    if parse_settings(matches)? {
        let mut wallet_manager = None;

        let (mut config, signers) = parse_args(matches, &mut wallet_manager)?;
        config.signers = signers.iter().map(|s| s.as_ref()).collect();
        let result = process_command(&config)?;
        println!("{}", result);
    };
    Ok(())
}
