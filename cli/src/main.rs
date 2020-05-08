use clap::{crate_description, crate_name, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use console::style;

use solana_clap_utils::{
    input_validators::is_url, keypair::SKIP_SEED_PHRASE_VALIDATION_ARG, DisplayError,
};
use solana_cli::{
    cli::{app, parse_command, process_command, CliCommandInfo, CliConfig, CliSigners},
    cli_output::OutputFormat,
    display::{println_name_value, println_name_value_or},
};
use solana_cli_config::{Config, CONFIG_FILE};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use std::{error, sync::Arc};

fn parse_settings(matches: &ArgMatches<'_>) -> Result<bool, Box<dyn error::Error>> {
    let parse_args = match matches.subcommand() {
        ("config", Some(matches)) => match matches.subcommand() {
            ("get", Some(subcommand_matches)) => {
                if let Some(config_file) = matches.value_of("config_file") {
                    let config = Config::load(config_file).unwrap_or_default();

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

                    if let Some(field) = subcommand_matches.value_of("specific_setting") {
                        let (field_name, value, setting_type) = match field {
                            "json_rpc_url" => ("RPC URL", json_rpc_url, url_setting_type),
                            "websocket_url" => ("WebSocket URL", websocket_url, ws_setting_type),
                            "keypair" => ("Key Path", keypair_path, keypair_setting_type),
                            _ => unreachable!(),
                        };
                        println_name_value_or(&format!("{}:", field_name), &value, setting_type);
                    } else {
                        println_name_value("Config File:", config_file);
                        println_name_value_or("RPC URL:", &json_rpc_url, url_setting_type);
                        println_name_value_or("WebSocket URL:", &websocket_url, ws_setting_type);
                        println_name_value_or("Keypair Path:", &keypair_path, keypair_setting_type);
                    }
                } else {
                    println!(
                        "{} Either provide the `--config` arg or ensure home directory exists to use the default config location",
                        style("No config file found.").bold()
                    );
                }
                false
            }
            ("set", Some(subcommand_matches)) => {
                if let Some(config_file) = matches.value_of("config_file") {
                    let mut config = Config::load(config_file).unwrap_or_default();
                    if let Some(url) = subcommand_matches.value_of("json_rpc_url") {
                        config.json_rpc_url = url.to_string();
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

                    println_name_value("Config File:", config_file);
                    println_name_value_or("RPC URL:", &json_rpc_url, url_setting_type);
                    println_name_value_or("WebSocket URL:", &websocket_url, ws_setting_type);
                    println_name_value_or("Keypair Path:", &keypair_path, keypair_setting_type);
                } else {
                    println!(
                        "{} Either provide the `--config` arg or ensure home directory exists to use the default config location",
                        style("No config file found.").bold()
                    );
                }
                false
            }
            _ => unreachable!(),
        },
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
    let (_, websocket_url) = CliConfig::compute_websocket_url_setting(
        matches.value_of("websocket_url").unwrap_or(""),
        &config.websocket_url,
        matches.value_of("json_rpc_url").unwrap_or(""),
        &config.json_rpc_url,
    );
    let (_, default_signer_path) = CliConfig::compute_keypair_path_setting(
        matches.value_of("keypair").unwrap_or(""),
        &config.keypair_path,
    );

    let CliCommandInfo { command, signers } =
        parse_command(&matches, &default_signer_path, &mut wallet_manager)?;

    let output_format = matches
        .value_of("output_format")
        .map(|value| match value {
            "json" => OutputFormat::Json,
            "json-compact" => OutputFormat::JsonCompact,
            _ => unreachable!(),
        })
        .unwrap_or(OutputFormat::Display);

    Ok((
        CliConfig {
            command,
            json_rpc_url,
            websocket_url,
            signers: vec![],
            keypair_path: default_signer_path,
            rpc_client: None,
            verbose: matches.is_present("verbose"),
            output_format,
        },
        signers,
    ))
}

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup();
    let matches = app(
        crate_name!(),
        crate_description!(),
        solana_clap_utils::version!(),
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
            arg.default_value(&config_file)
        } else {
            arg
        }
    })
    .arg(
        Arg::with_name("json_rpc_url")
            .short("u")
            .long("url")
            .value_name("URL")
            .takes_value(true)
            .global(true)
            .validator(is_url)
            .help("JSON RPC URL for the solana cluster"),
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
        Arg::with_name("verbose")
            .long("verbose")
            .short("v")
            .global(true)
            .help("Show additional information"),
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
                            .possible_values(&["json_rpc_url", "websocket_url", "keypair"])
                            .help("Return a specific config setting"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("set")
                    .about("Set a config setting")
                    .group(
                        ArgGroup::with_name("config_settings")
                            .args(&["json_rpc_url", "websocket_url", "keypair"])
                            .multiple(true)
                            .required(true),
                    ),
            ),
    )
    .get_matches();

    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(matches: &ArgMatches<'_>) -> Result<(), Box<dyn error::Error>> {
    if parse_settings(&matches)? {
        let mut wallet_manager = None;

        let (mut config, signers) = parse_args(&matches, &mut wallet_manager)?;
        config.signers = signers.iter().map(|s| s.as_ref()).collect();
        let result = process_command(&config)?;
        println!("{}", result);
    };
    Ok(())
}
