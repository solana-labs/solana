use {
    crate::{
        address_lookup_table::AddressLookupTableSubCommands, cli::*, cluster_query::*, feature::*,
        inflation::*, nonce::*, program::*, program_v4::ProgramV4SubCommands, stake::*,
        validator_info::*, vote::*, wallet::*,
    },
    clap::{App, AppSettings, Arg, ArgGroup, SubCommand},
    solana_clap_utils::{self, hidden_unless_forced, input_validators::*, keypair::*},
    solana_cli_config::CONFIG_FILE,
};

pub fn get_clap_app<'ab, 'v>(name: &str, about: &'ab str, version: &'v str) -> App<'ab, 'v> {
    App::new(name)
        .about(about)
        .version(version)
        .setting(AppSettings::SubcommandRequiredElseHelp)
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
            Arg::with_name("use_quic")
                .long("use-quic")
                .global(true)
                .help("Use QUIC when sending transactions."),
        )
        .arg(
            Arg::with_name("use_udp")
                .long("use-udp")
                .global(true)
                .conflicts_with("use_quic")
                .help("Use UDP when sending transactions."),
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
                .hidden(hidden_unless_forced())
                .help("Timeout value for RPC requests"),
        )
        .arg(
            Arg::with_name("confirm_transaction_initial_timeout")
                .long("confirm-timeout")
                .value_name("SECONDS")
                .takes_value(true)
                .default_value(DEFAULT_CONFIRM_TX_TIMEOUT_SECONDS)
                .global(true)
                .hidden(hidden_unless_forced())
                .help("Timeout value for initial transaction status"),
        )
        .cluster_query_subcommands()
        .feature_subcommands()
        .inflation_subcommands()
        .nonce_subcommands()
        .program_subcommands()
        .program_v4_subcommands()
        .address_lookup_table_subcommands()
        .stake_subcommands()
        .validator_info_subcommands()
        .vote_subcommands()
        .wallet_subcommands()
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
        .subcommand(
            SubCommand::with_name("completion")
            .about("Generate completion scripts for various shells")
            .arg(
                Arg::with_name("shell")
                .long("shell")
                .short("s")
                .takes_value(true)
                .possible_values(&["bash", "fish", "zsh", "powershell", "elvish"])
                .default_value("bash")
            )
        )
}
