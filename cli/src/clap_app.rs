use crate::{
    cli::*, cluster_query::*, feature::*, inflation::*, nonce::*, program::*, stake::*,
    validator_info::*, vote::*,
};
use clap::{App, AppSettings, Arg, ArgGroup, SubCommand};
use solana_clap_utils::{
    self, fee_payer::fee_payer_arg, input_validators::*, keypair::*, memo::memo_arg, nonce::*,
    offline::*,
};
use solana_cli_config::CONFIG_FILE;

pub fn get_clap_app<'ab, 'v>(name: &str, about: &'ab str, version: &'v str) -> App<'ab, 'v> {
    App::new(name)
        .about(about)
        .version(version)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("address")
                .about("Get your public key")
                .arg(
                    Arg::with_name("confirm_key")
                        .long("confirm-key")
                        .takes_value(false)
                        .help("Confirm key on device; only relevant if using remote wallet"),
                ),
        )
        .cluster_query_subcommands()
        .feature_subcommands()
        .inflation_subcommands()
        .nonce_subcommands()
        .program_subcommands()
        .stake_subcommands()
        .subcommand(
            SubCommand::with_name("airdrop")
                .about("Request SOL from a faucet")
                .arg(
                    Arg::with_name("amount")
                        .index(1)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount)
                        .required(true)
                        .help("The airdrop amount to request, in SOL"),
                )
                .arg(
                    pubkey!(Arg::with_name("to")
                        .index(2)
                        .value_name("RECIPIENT_ADDRESS"),
                        "The account address of airdrop recipient. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("balance")
                .about("Get your balance")
                .arg(
                    pubkey!(Arg::with_name("pubkey")
                        .index(1)
                        .value_name("ACCOUNT_ADDRESS"),
                        "The account address of the balance to check. ")
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("confirm")
                .about("Confirm transaction by signature")
                .arg(
                    Arg::with_name("signature")
                        .index(1)
                        .value_name("TRANSACTION_SIGNATURE")
                        .takes_value(true)
                        .required(true)
                        .help("The transaction signature to confirm"),
                )
                .after_help(// Formatted specifically for the manually-indented heredoc string
                   "Note: This will show more detailed information for finalized transactions with verbose mode (-v/--verbose).\
                  \n\
                  \nAccount modes:\
                  \n  |srwx|\
                  \n    s: signed\
                  \n    r: readable (always true)\
                  \n    w: writable\
                  \n    x: program account (inner instructions excluded)\
                   "
                ),
        )
        .subcommand(
            SubCommand::with_name("decode-transaction")
                .about("Decode a serialized transaction")
                .arg(
                    Arg::with_name("transaction")
                        .index(1)
                        .value_name("TRANSACTION")
                        .takes_value(true)
                        .required(true)
                        .help("transaction to decode"),
                )
                .arg(
                    Arg::with_name("encoding")
                        .index(2)
                        .value_name("ENCODING")
                        .possible_values(&["base58", "base64"]) // Subset of `UiTransactionEncoding` enum
                        .default_value("base58")
                        .takes_value(true)
                        .required(true)
                        .help("transaction encoding"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-address-with-seed")
                .about("Generate a derived account address with a seed")
                .arg(
                    Arg::with_name("seed")
                        .index(1)
                        .value_name("SEED_STRING")
                        .takes_value(true)
                        .required(true)
                        .validator(is_derived_address_seed)
                        .help("The seed.  Must not take more than 32 bytes to encode as utf-8"),
                )
                .arg(
                    Arg::with_name("program_id")
                        .index(2)
                        .value_name("PROGRAM_ID")
                        .takes_value(true)
                        .required(true)
                        .help(
                            "The program_id that the address will ultimately be used for, \n\
                             or one of NONCE, STAKE, and VOTE keywords",
                        ),
                )
                .arg(
                    pubkey!(Arg::with_name("from")
                        .long("from")
                        .value_name("FROM_PUBKEY")
                        .required(false),
                        "From (base) key, [default: cli config keypair]. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("deploy")
                .about("Deploy a program")
                .arg(
                    Arg::with_name("program_location")
                        .index(1)
                        .value_name("PROGRAM_FILEPATH")
                        .takes_value(true)
                        .required(true)
                        .help("/path/to/program.o"),
                )
                .arg(
                    Arg::with_name("address_signer")
                        .index(2)
                        .value_name("PROGRAM_ADDRESS_SIGNER")
                        .takes_value(true)
                        .validator(is_valid_signer)
                        .help("The signer for the desired address of the program [default: new random address]")
                )
                .arg(
                    Arg::with_name("use_deprecated_loader")
                        .long("use-deprecated-loader")
                        .takes_value(false)
                        .hidden(true) // Don't document this argument to discourage its use
                        .help("Use the deprecated BPF loader")
                )
                .arg(
                    Arg::with_name("allow_excessive_balance")
                        .long("allow-excessive-deploy-account-balance")
                        .takes_value(false)
                        .help("Use the designated program id, even if the account already holds a large balance of SOL")
                ),
        )
        .subcommand(
            SubCommand::with_name("resolve-signer")
                .about("Checks that a signer is valid, and returns its specific path; useful for signers that may be specified generally, eg. usb://ledger")
                .arg(
                    Arg::with_name("signer")
                        .index(1)
                        .value_name("SIGNER_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("The signer path to resolve")
                )
        )
        .subcommand(
            SubCommand::with_name("transfer")
                .about("Transfer funds between system accounts")
                .alias("pay")
                .arg(
                    pubkey!(Arg::with_name("to")
                        .index(1)
                        .value_name("RECIPIENT_ADDRESS")
                        .required(true),
                        "The account address of recipient. "),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount_or_all)
                        .required(true)
                        .help("The amount to send, in SOL; accepts keyword ALL"),
                )
                .arg(
                    pubkey!(Arg::with_name("from")
                        .long("from")
                        .value_name("FROM_ADDRESS"),
                        "Source account of funds (if different from client local account). "),
                )
                .arg(
                    Arg::with_name("no_wait")
                        .long("no-wait")
                        .takes_value(false)
                        .help("Return signature immediately after submitting the transaction, instead of waiting for confirmations"),
                )
                .arg(
                    Arg::with_name("derived_address_seed")
                        .long("derived-address-seed")
                        .takes_value(true)
                        .value_name("SEED_STRING")
                        .requires("derived_address_program_id")
                        .validator(is_derived_address_seed)
                        .hidden(true)
                )
                .arg(
                    Arg::with_name("derived_address_program_id")
                        .long("derived-address-program-id")
                        .takes_value(true)
                        .value_name("PROGRAM_ID")
                        .requires("derived_address_seed")
                        .hidden(true)
                )
                .arg(
                    Arg::with_name("allow_unfunded_recipient")
                        .long("allow-unfunded-recipient")
                        .takes_value(false)
                        .help("Complete the transfer even if the recipient address is not funded")
                )
                .offline_args()
                .nonce_args(false)
                .arg(memo_arg())
                .arg(fee_payer_arg()),
        )
        .subcommand(
            SubCommand::with_name("account")
                .about("Show the contents of an account")
                .alias("account")
                .arg(
                    pubkey!(Arg::with_name("account_pubkey")
                        .index(1)
                        .value_name("ACCOUNT_ADDRESS")
                        .required(true),
                        "Account key URI. ")
                )
                .arg(
                    Arg::with_name("output_file")
                        .long("output-file")
                        .short("o")
                        .value_name("FILEPATH")
                        .takes_value(true)
                        .help("Write the account data to this file"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .validator_info_subcommands()
        .vote_subcommands()
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
