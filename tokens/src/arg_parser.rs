use {
    crate::args::{
        Args, BalancesArgs, Command, DistributeTokensArgs, SenderStakeArgs, SplTokenArgs,
        StakeArgs, TransactionLogArgs,
    },
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, App, Arg, ArgMatches, SubCommand,
    },
    solana_clap_utils::{
        input_parsers::{pubkey_of_signer, value_of},
        input_validators::{is_amount, is_url_or_moniker, is_valid_pubkey, is_valid_signer},
        keypair::{pubkey_from_path, signer_from_path},
    },
    solana_cli_config::CONFIG_FILE,
    solana_remote_wallet::remote_wallet::maybe_wallet_manager,
    solana_sdk::native_token::sol_to_lamports,
    std::{error::Error, ffi::OsString, process::exit},
};

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
                .short("C")
                .long("config")
                .takes_value(true)
                .value_name("FILEPATH")
                .default_value(default_config_file)
                .help("Config file"),
        )
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
        .subcommand(
            SubCommand::with_name("distribute-tokens")
                .about("Distribute SOL")
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
            SubCommand::with_name("create-stake")
                .about("Create stake accounts")
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
                    Arg::with_name("unlocked_sol")
                        .default_value("1.0")
                        .long("unlocked-sol")
                        .takes_value(true)
                        .value_name("SOL_AMOUNT")
                        .help("Amount of SOL to put in system account to pay for fees"),
                )
                .arg(
                    Arg::with_name("lockup_authority")
                        .long("lockup-authority")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_valid_pubkey)
                        .help("Lockup Authority Address"),
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
                .about("Split to stake accounts")
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
            SubCommand::with_name("distribute-spl-tokens")
                .about("Distribute SPL tokens")
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
                    Arg::with_name("transfer_amount")
                        .long("transfer-amount")
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(is_amount)
                        .help("The amount of SPL tokens to send to each recipient"),
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
                    Arg::with_name("token_account_address")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("TOKEN_ACCOUNT_ADDRESS")
                        .validator(is_valid_pubkey)
                        .help("SPL token account to send from"),
                )
                .arg(
                    Arg::with_name("token_owner")
                        .long("owner")
                        .required(true)
                        .takes_value(true)
                        .value_name("TOKEN_ACCOUNT_OWNER_KEYPAIR")
                        .validator(is_valid_signer)
                        .help("SPL token account owner"),
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
            SubCommand::with_name("spl-token-balances")
                .about("Balance of SPL token associated accounts")
                .arg(
                    Arg::with_name("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::with_name("mint_address")
                        .long("mint")
                        .required(true)
                        .takes_value(true)
                        .value_name("MINT_ADDRESS")
                        .validator(is_valid_pubkey)
                        .help("SPL token mint of distribution"),
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
        spl_token_args: None,
        transfer_amount: value_of(matches, "transfer_amount").map(sol_to_lamports),
    })
}

fn parse_create_stake_args(
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

    let lockup_authority_str = value_t!(matches, "lockup_authority", String).ok();
    let lockup_authority = lockup_authority_str
        .map(|path| {
            pubkey_from_path(
                &signer_matches,
                &path,
                "lockup authority",
                &mut wallet_manager,
            )
        })
        .transpose()?;

    let stake_args = StakeArgs {
        unlocked_sol: sol_to_lamports(value_t_or_exit!(matches, "unlocked_sol", f64)),
        lockup_authority,
        sender_stake_args: None,
    };
    Ok(DistributeTokensArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair,
        fee_payer,
        stake_args: Some(stake_args),
        spl_token_args: None,
        transfer_amount: None,
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
    let lockup_authority = lockup_authority_str
        .map(|path| {
            signer_from_path(
                &signer_matches,
                &path,
                "lockup authority",
                &mut wallet_manager,
            )
        })
        .transpose()?;

    let lockup_authority_address = lockup_authority.as_ref().map(|keypair| keypair.pubkey());
    let sender_stake_args = SenderStakeArgs {
        stake_account_address,
        stake_authority,
        withdraw_authority,
        lockup_authority,
        rent_exempt_reserve: None,
    };
    let stake_args = StakeArgs {
        unlocked_sol: sol_to_lamports(value_t_or_exit!(matches, "unlocked_sol", f64)),
        lockup_authority: lockup_authority_address,
        sender_stake_args: Some(sender_stake_args),
    };
    Ok(DistributeTokensArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair,
        fee_payer,
        stake_args: Some(stake_args),
        spl_token_args: None,
        transfer_amount: None,
    })
}

fn parse_distribute_spl_tokens_args(
    matches: &ArgMatches<'_>,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let token_owner_str = value_t_or_exit!(matches, "token_owner", String);
    let token_owner = signer_from_path(
        &signer_matches,
        &token_owner_str,
        "owner",
        &mut wallet_manager,
    )?;

    let fee_payer_str = value_t_or_exit!(matches, "fee_payer", String);
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    let token_account_address_str = value_t_or_exit!(matches, "token_account_address", String);
    let token_account_address = pubkey_from_path(
        &signer_matches,
        &token_account_address_str,
        "token account address",
        &mut wallet_manager,
    )?;

    Ok(DistributeTokensArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        transaction_db: value_t_or_exit!(matches, "db_path", String),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair: token_owner,
        fee_payer,
        stake_args: None,
        spl_token_args: Some(SplTokenArgs {
            token_account_address,
            ..SplTokenArgs::default()
        }),
        transfer_amount: value_of(matches, "transfer_amount"),
    })
}

fn parse_balances_args(matches: &ArgMatches<'_>) -> Result<BalancesArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let spl_token_args =
        pubkey_of_signer(matches, "mint_address", &mut wallet_manager)?.map(|mint| SplTokenArgs {
            mint,
            ..SplTokenArgs::default()
        });
    Ok(BalancesArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        spl_token_args,
    })
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
    let url = matches.value_of("json_rpc_url").map(|x| x.to_string());

    let command = match matches.subcommand() {
        ("distribute-tokens", Some(matches)) => {
            Command::DistributeTokens(parse_distribute_tokens_args(matches)?)
        }
        ("create-stake", Some(matches)) => {
            Command::DistributeTokens(parse_create_stake_args(matches)?)
        }
        ("distribute-stake", Some(matches)) => {
            Command::DistributeTokens(parse_distribute_stake_args(matches)?)
        }
        ("distribute-spl-tokens", Some(matches)) => {
            Command::DistributeTokens(parse_distribute_spl_tokens_args(matches)?)
        }
        ("balances", Some(matches)) => Command::Balances(parse_balances_args(matches)?),
        ("spl-token-balances", Some(matches)) => Command::Balances(parse_balances_args(matches)?),
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
