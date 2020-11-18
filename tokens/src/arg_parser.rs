use crate::args::{
    Args, BalancesArgs, Command, DistributeTokensArgs, StakeArgs, TransactionLogArgs,
};
use clap::{
    crate_description, crate_name, value_t, value_t_or_exit, App, Arg, ArgMatches, SubCommand,
};
use solana_clap_utils::{
    input_parsers::value_of,
    input_validators::{is_amount, is_valid_pubkey, is_valid_signer},
    keypair::{pubkey_from_path, signer_from_path},
};
use solana_cli_config::CONFIG_FILE;
use solana_remote_wallet::remote_wallet::maybe_wallet_manager;
use std::error::Error;
use std::ffi::OsString;
use std::process::exit;

fn get_matches<'a, I, T>(args: I) -> ArgMatches<'a>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let default_config_file = CONFIG_FILE.as_ref().unwrap();
    App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("config_file")
                .long("config")
                .takes_value(true)
                .value_name("FILEPATH")
                .default_value(default_config_file)
                .help("Config file"),
        )
        .arg(
            Arg::with_name("url")
                .long("url")
                .global(true)
                .takes_value(true)
                .value_name("URL")
                .help("RPC entrypoint address. i.e. http://devnet.solana.com"),
        )
        .subcommand(
            SubCommand::with_name("distribute-tokens")
                .about("Distribute tokens")
                .arg(
                    Arg::with_name("db_path")
                        .long("db-path")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help(
                            "Location for storing distribution database. \
                            The database is used for tracking transactions as they are finalized \
                            and preventing double spends.",
                        ),
                )
                .arg(
                    Arg::with_name("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Input CSV file"),
                )
                .arg(
                    Arg::with_name("transfer_amount")
                        .long("transfer-amount")
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(is_amount)
                        .help("The amount to send to each recipient, in SOL"),
                )
                .arg(
                    Arg::with_name("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::with_name("output_path")
                        .long("output-path")
                        .short("o")
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::with_name("sender_keypair")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("SENDING_KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Keypair to fund accounts"),
                )
                .arg(
                    Arg::with_name("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            SubCommand::with_name("distribute-stake")
                .about("Distribute stake accounts")
                .arg(
                    Arg::with_name("db_path")
                        .long("db-path")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help(
                            "Location for storing distribution database. \
                            The database is used for tracking transactions as they are finalized \
                            and preventing double spends.",
                        ),
                )
                .arg(
                    Arg::with_name("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::with_name("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::with_name("output_path")
                        .long("output-path")
                        .short("o")
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::with_name("sender_keypair")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("SENDING_KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Keypair to fund accounts"),
                )
                .arg(
                    Arg::with_name("stake_account_address")
                        .required(true)
                        .long("stake-account-address")
                        .takes_value(true)
                        .value_name("ACCOUNT_ADDRESS")
                        .validator(is_valid_pubkey)
                        .help("Stake Account Address"),
                )
                .arg(
                    Arg::with_name("unlocked_sol")
                        .default_value("1.0")
                        .long("unlocked-sol")
                        .takes_value(true)
                        .value_name("SOL_AMOUNT")
                        .help("Amount of SOL to put in system account to pay for fees"),
                )
                .arg(
                    Arg::with_name("stake_authority")
                        .long("stake-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Stake Authority Keypair"),
                )
                .arg(
                    Arg::with_name("withdraw_authority")
                        .long("withdraw-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Withdraw Authority Keypair"),
                )
                .arg(
                    Arg::with_name("lockup_authority")
                        .long("lockup-authority")
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Lockup Authority Keypair"),
                )
                .arg(
                    Arg::with_name("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            SubCommand::with_name("balances")
                .about("Balance of each account")
                .arg(
                    Arg::with_name("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("transaction-log")
                .about("Print the database to a CSV file")
                .arg(
                    Arg::with_name("db_path")
                        .long("db-path")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Location of database to query"),
                )
                .arg(
                    Arg::with_name("output_path")
                        .long("output-path")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Output file"),
                ),
        )
        .get_matches_from(args)
}

fn parse_distribute_tokens_args(
    matches: &ArgMatches<'_>,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let sender_keypair_str = value_t_or_exit!(matches, "sender_keypair", String);
    let sender_keypair = signer_from_path(
        &signer_matches,
        &sender_keypair_str,
        "sender",
        &mut wallet_manager,
    )?;

    let fee_payer_str = value_t_or_exit!(matches, "fee_payer", String);
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    Ok(DistributeTokensArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair,
        fee_payer,
        stake_args: None,
        transfer_amount: value_of(matches, "transfer_amount"),
    })
}

fn parse_distribute_stake_args(
    matches: &ArgMatches<'_>,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let sender_keypair_str = value_t_or_exit!(matches, "sender_keypair", String);
    let sender_keypair = signer_from_path(
        &signer_matches,
        &sender_keypair_str,
        "sender",
        &mut wallet_manager,
    )?;

    let fee_payer_str = value_t_or_exit!(matches, "fee_payer", String);
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    let stake_account_address_str = value_t_or_exit!(matches, "stake_account_address", String);
    let stake_account_address = pubkey_from_path(
        &signer_matches,
        &stake_account_address_str,
        "stake account address",
        &mut wallet_manager,
    )?;

    let stake_authority_str = value_t_or_exit!(matches, "stake_authority", String);
    let stake_authority = signer_from_path(
        &signer_matches,
        &stake_authority_str,
        "stake authority",
        &mut wallet_manager,
    )?;

    let withdraw_authority_str = value_t_or_exit!(matches, "withdraw_authority", String);
    let withdraw_authority = signer_from_path(
        &signer_matches,
        &withdraw_authority_str,
        "withdraw authority",
        &mut wallet_manager,
    )?;

    let lockup_authority_str = value_t!(matches, "lockup_authority", String).ok();
    let lockup_authority = match lockup_authority_str {
        Some(path) => Some(signer_from_path(
            &signer_matches,
            &path,
            "lockup authority",
            &mut wallet_manager,
        )?),
        None => None,
    };

    let stake_args = StakeArgs {
        stake_account_address,
        unlocked_sol: value_t_or_exit!(matches, "unlocked_sol", f64),
        stake_authority,
        withdraw_authority,
        lockup_authority,
    };
    Ok(DistributeTokensArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair,
        fee_payer,
        stake_args: Some(stake_args),
        transfer_amount: None,
    })
}

fn parse_balances_args(matches: &ArgMatches<'_>) -> BalancesArgs {
    BalancesArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
    }
}

fn parse_transaction_log_args(matches: &ArgMatches<'_>) -> TransactionLogArgs {
    TransactionLogArgs {
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: value_t_or_exit!(matches, "output_path", String),
    }
}

pub fn parse_args<I, T>(args: I) -> Result<Args, Box<dyn Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = get_matches(args);
    let config_file = matches.value_of("config_file").unwrap().to_string();
    let url = matches.value_of("url").map(|x| x.to_string());

    let command = match matches.subcommand() {
        ("distribute-tokens", Some(matches)) => {
            Command::DistributeTokens(parse_distribute_tokens_args(matches)?)
        }
        ("distribute-stake", Some(matches)) => {
            Command::DistributeTokens(parse_distribute_stake_args(matches)?)
        }
        ("balances", Some(matches)) => Command::Balances(parse_balances_args(matches)),
        ("transaction-log", Some(matches)) => {
            Command::TransactionLog(parse_transaction_log_args(matches))
        }
        _ => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
    };
    let args = Args {
        config_file,
        url,
        command,
    };
    Ok(args)
}
