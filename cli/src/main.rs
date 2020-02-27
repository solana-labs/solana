use clap::{crate_description, crate_name, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use console::style;

use solana_clap_utils::{
    input_parsers::derivation_of,
    input_validators::{is_derivation, is_url},
    keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
};
use solana_cli::{
    cli::{app, parse_command, process_command, CliCommandInfo, CliConfig, CliSigners},
    display::{println_name_value, println_name_value_or},
};
use solana_cli_config::config::{Config, CONFIG_FILE};
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use std::{error, sync::Arc};

fn parse_settings(matches: &ArgMatches<'_>) -> Result<bool, Box<dyn error::Error>> {
    let parse_args = match matches.subcommand() {
        ("config", Some(matches)) => match matches.subcommand() {
            ("get", Some(subcommand_matches)) => {
                if let Some(config_file) = matches.value_of("config_file") {
                    let config = Config::load(config_file).unwrap_or_default();
                    if let Some(field) = subcommand_matches.value_of("specific_setting") {
                        let (field_name, value, default_value) = match field {
                            "url" => ("RPC URL", config.url, CliConfig::default_json_rpc_url()),
                            "keypair" => (
                                "Key Path",
                                config.keypair_path,
                                CliConfig::default_keypair_path(),
                            ),
                            _ => unreachable!(),
                        };
                        println_name_value_or(&format!("{}:", field_name), &value, &default_value);
                    } else {
                        println_name_value("Config File:", config_file);
                        println_name_value_or(
                            "RPC URL:",
                            &config.url,
                            &CliConfig::default_json_rpc_url(),
                        );
                        println_name_value_or(
                            "Keypair Path:",
                            &config.keypair_path,
                            &CliConfig::default_keypair_path(),
                        );
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
                        config.url = url.to_string();
                    }
                    if let Some(keypair) = subcommand_matches.value_of("keypair") {
                        config.keypair_path = keypair.to_string();
                    }
                    config.save(config_file)?;
                    println_name_value("Config File:", config_file);
                    println_name_value("RPC URL:", &config.url);
                    println_name_value("Keypair Path:", &config.keypair_path);
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
    wallet_manager: Option<Arc<RemoteWalletManager>>,
) -> Result<(CliConfig<'a>, CliSigners), Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };
    let json_rpc_url = if let Some(url) = matches.value_of("json_rpc_url") {
        url.to_string()
    } else if config.url != "" {
        config.url
    } else {
        let default = CliConfig::default();
        default.json_rpc_url
    };

    let default_signer_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap().to_string()
    } else if config.keypair_path != "" {
        config.keypair_path
    } else {
        CliConfig::default_keypair_path()
    };

    let CliCommandInfo { command, signers } =
        parse_command(&matches, &default_signer_path, wallet_manager.as_ref())?;

    Ok((
        CliConfig {
            command,
            json_rpc_url,
            signers: vec![],
            keypair_path: default_signer_path,
            derivation_path: derivation_of(matches, "derivation_path"),
            rpc_client: None,
            verbose: matches.is_present("verbose"),
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
            .value_name("PATH")
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
        Arg::with_name("keypair")
            .short("k")
            .long("keypair")
            .value_name("PATH")
            .global(true)
            .takes_value(true)
            .help("/path/to/id.json or usb://remote/wallet/path"),
    )
    .arg(
        Arg::with_name("derivation_path")
            .long("derivation-path")
            .value_name("ACCOUNT or ACCOUNT/CHANGE")
            .global(true)
            .takes_value(true)
            .validator(is_derivation)
            .help("Derivation path to use: m/44'/501'/ACCOUNT'/CHANGE'; default key is device base pubkey: m/44'/501'/0'")
    )
    .arg(
        Arg::with_name("verbose")
            .long("verbose")
            .short("v")
            .global(true)
            .help("Show extra information header"),
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
                            .possible_values(&["url", "keypair"])
                            .help("Return a specific config setting"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("set")
                    .about("Set a config setting")
                    .group(
                        ArgGroup::with_name("config_settings")
                            .args(&["json_rpc_url", "keypair"])
                            .multiple(true)
                            .required(true),
                    ),
            ),
    )
    .get_matches();

    if parse_settings(&matches)? {
        let wallet_manager = maybe_wallet_manager()?;

        let (mut config, signers) = parse_args(&matches, wallet_manager)?;
        config.signers = signers.iter().map(|s| s.as_ref()).collect();
        let result = process_command(&config)?;
        println!("{}", result);
    }
    Ok(())
}
