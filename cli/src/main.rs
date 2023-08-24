use {
    clap::{crate_description, crate_name, value_t_or_exit, ArgMatches},
    console::style,
    solana_clap_utils::{
        input_validators::normalize_to_url_if_moniker,
        keypair::{CliSigners, DefaultSigner},
        DisplayError,
    },
    solana_cli::{
        clap_app::get_clap_app,
        cli::{parse_command, process_command, CliCommandInfo, CliConfig},
    },
    solana_cli_config::{Config, ConfigInput},
    solana_cli_output::{
        display::{println_name_value, println_name_value_or},
        OutputFormat,
    },
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client_api::config::RpcSendTransactionConfig,
    solana_tpu_client::tpu_client::DEFAULT_TPU_ENABLE_UDP,
    std::{collections::HashMap, error, path::PathBuf, rc::Rc, time::Duration},
};

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
                        ConfigInput::compute_json_rpc_url_setting("", &config.json_rpc_url);
                    let (ws_setting_type, websocket_url) =
                        ConfigInput::compute_websocket_url_setting(
                            "",
                            &config.websocket_url,
                            "",
                            &config.json_rpc_url,
                        );
                    let (keypair_setting_type, keypair_path) =
                        ConfigInput::compute_keypair_path_setting("", &config.keypair_path);
                    let (commitment_setting_type, commitment) =
                        ConfigInput::compute_commitment_config("", &config.commitment);

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
                        println_name_value_or(&format!("{field_name}:"), &value, setting_type);
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
                        ConfigInput::compute_json_rpc_url_setting("", &config.json_rpc_url);
                    let (ws_setting_type, websocket_url) =
                        ConfigInput::compute_websocket_url_setting(
                            "",
                            &config.websocket_url,
                            "",
                            &config.json_rpc_url,
                        );
                    let (keypair_setting_type, keypair_path) =
                        ConfigInput::compute_keypair_path_setting("", &config.keypair_path);
                    let (commitment_setting_type, commitment) =
                        ConfigInput::compute_commitment_config("", &config.commitment);

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
                    println!("Address labels imported from {filename:?}");
                }
                ("export-address-labels", Some(subcommand_matches)) => {
                    let filename = value_t_or_exit!(subcommand_matches, "filename", PathBuf);
                    config.export_address_labels(&filename)?;
                    println!("Address labels exported to {filename:?}");
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
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<(CliConfig<'a>, CliSigners), Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };
    let (_, json_rpc_url) = ConfigInput::compute_json_rpc_url_setting(
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );

    let rpc_timeout = value_t_or_exit!(matches, "rpc_timeout", u64);
    let rpc_timeout = Duration::from_secs(rpc_timeout);

    let confirm_transaction_initial_timeout =
        value_t_or_exit!(matches, "confirm_transaction_initial_timeout", u64);
    let confirm_transaction_initial_timeout =
        Duration::from_secs(confirm_transaction_initial_timeout);

    let (_, websocket_url) = ConfigInput::compute_websocket_url_setting(
        matches.value_of("websocket_url").unwrap_or(""),
        &config.websocket_url,
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );
    let default_signer_arg_name = "keypair".to_string();
    let (_, default_signer_path) = ConfigInput::compute_keypair_path_setting(
        matches.value_of(&default_signer_arg_name).unwrap_or(""),
        &config.keypair_path,
    );

    let default_signer = DefaultSigner::new(default_signer_arg_name, &default_signer_path);

    let CliCommandInfo {
        command,
        mut signers,
    } = parse_command(matches, &default_signer, wallet_manager)?;

    if signers.is_empty() {
        if let Ok(signer_info) =
            default_signer.generate_unique_signers(vec![None], matches, wallet_manager)
        {
            signers.extend(signer_info.signers);
        }
    }

    let verbose = matches.is_present("verbose");
    let output_format = OutputFormat::from_matches(matches, "output_format", verbose);

    let (_, commitment) = ConfigInput::compute_commitment_config(
        matches.value_of("commitment").unwrap_or(""),
        &config.commitment,
    );

    let address_labels = if matches.is_present("no_address_labels") {
        HashMap::new()
    } else {
        config.address_labels
    };

    let use_quic = if matches.is_present("use_quic") {
        true
    } else if matches.is_present("use_udp") {
        false
    } else {
        !DEFAULT_TPU_ENABLE_UDP
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
            use_quic,
        },
        signers,
    ))
}

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup_with_default("off");
    let matches = get_clap_app(
        crate_name!(),
        crate_description!(),
        solana_version::version!(),
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
        println!("{result}");
    };
    Ok(())
}
