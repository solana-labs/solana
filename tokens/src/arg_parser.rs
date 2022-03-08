use {
    crate::args::{
        Args, BalancesArgs, Command, DistributeTokensArgs, SenderStakeArgs, SplTokenArgs,
        StakeArgs, TransactionLogArgs,
    },
    clap::{crate_description, crate_name, Arg, ArgMatches},
    solana_clap_utils::{
        input_parsers::{pubkey_of_signer, value_of},
        input_validators::{is_amount, is_valid_pubkey, is_valid_signer},
        keypair::{pubkey_from_path, signer_from_path},
    },
    solana_cli_config::CONFIG_FILE,
    solana_remote_wallet::remote_wallet::maybe_wallet_manager,
    solana_sdk::native_token::sol_to_lamports,
    std::{error::Error, ffi::OsString, process::exit},
};

fn get_matches<I, T>(args: I) -> ArgMatches
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let default_config_file = CONFIG_FILE.as_ref().unwrap();
    clap::Command::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::new("config_file")
                .long("config")
                .takes_value(true)
                .value_name("FILEPATH")
                .default_value(default_config_file)
                .help("Config file"),
        )
        .arg(
            Arg::new("url")
                .long("url")
                .global(true)
                .takes_value(true)
                .value_name("URL")
                .help("RPC entrypoint address. i.e. http://api.devnet.solana.com"),
        )
        .subcommand(
            clap::Command::new("distribute-tokens")
                .about("Distribute SOL")
                .arg(
                    Arg::new("db_path")
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
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Input CSV file"),
                )
                .arg(
                    Arg::new("transfer_amount")
                        .long("transfer-amount")
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(|s| is_amount(s))
                        .help("The amount to send to each recipient, in SOL"),
                )
                .arg(
                    Arg::new("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::new("output_path")
                        .long("output-path")
                        .short('o')
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::new("sender_keypair")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("SENDING_KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Keypair to fund accounts"),
                )
                .arg(
                    Arg::new("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            clap::Command::new("create-stake")
                .about("Create stake accounts")
                .arg(
                    Arg::new("db_path")
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
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::new("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::new("output_path")
                        .long("output-path")
                        .short('o')
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::new("sender_keypair")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("SENDING_KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Keypair to fund accounts"),
                )
                .arg(
                    Arg::new("unlocked_sol")
                        .default_value("1.0")
                        .long("unlocked-sol")
                        .takes_value(true)
                        .value_name("SOL_AMOUNT")
                        .help("Amount of SOL to put in system account to pay for fees"),
                )
                .arg(
                    Arg::new("lockup_authority")
                        .long("lockup-authority")
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(|s| is_valid_pubkey(s))
                        .help("Lockup Authority Address"),
                )
                .arg(
                    Arg::new("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            clap::Command::new("distribute-stake")
                .about("Split to stake accounts")
                .arg(
                    Arg::new("db_path")
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
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::new("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::new("output_path")
                        .long("output-path")
                        .short('o')
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::new("sender_keypair")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("SENDING_KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Keypair to fund accounts"),
                )
                .arg(
                    Arg::new("stake_account_address")
                        .required(true)
                        .long("stake-account-address")
                        .takes_value(true)
                        .value_name("ACCOUNT_ADDRESS")
                        .validator(|s| is_valid_pubkey(s))
                        .help("Stake Account Address"),
                )
                .arg(
                    Arg::new("unlocked_sol")
                        .default_value("1.0")
                        .long("unlocked-sol")
                        .takes_value(true)
                        .value_name("SOL_AMOUNT")
                        .help("Amount of SOL to put in system account to pay for fees"),
                )
                .arg(
                    Arg::new("stake_authority")
                        .long("stake-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Stake Authority Keypair"),
                )
                .arg(
                    Arg::new("withdraw_authority")
                        .long("withdraw-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Withdraw Authority Keypair"),
                )
                .arg(
                    Arg::new("lockup_authority")
                        .long("lockup-authority")
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Lockup Authority Keypair"),
                )
                .arg(
                    Arg::new("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            clap::Command::new("distribute-spl-tokens")
                .about("Distribute SPL tokens")
                .arg(
                    Arg::new("db_path")
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
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::new("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                )
                .arg(
                    Arg::new("transfer_amount")
                        .long("transfer-amount")
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(|s| is_amount(s))
                        .help("The amount of SPL tokens to send to each recipient"),
                )
                .arg(
                    Arg::new("output_path")
                        .long("output-path")
                        .short('o')
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the transaction log to this file"),
                )
                .arg(
                    Arg::new("token_account_address")
                        .long("from")
                        .required(true)
                        .takes_value(true)
                        .value_name("TOKEN_ACCOUNT_ADDRESS")
                        .validator(|s| is_valid_pubkey(s))
                        .help("SPL token account to send from"),
                )
                .arg(
                    Arg::new("token_owner")
                        .long("owner")
                        .required(true)
                        .takes_value(true)
                        .value_name("TOKEN_ACCOUNT_OWNER_KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("SPL token account owner"),
                )
                .arg(
                    Arg::new("fee_payer")
                        .long("fee-payer")
                        .required(true)
                        .takes_value(true)
                        .value_name("KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Fee payer"),
                ),
        )
        .subcommand(
            clap::Command::new("balances")
                .about("Balance of each account")
                .arg(
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                ),
        )
        .subcommand(
            clap::Command::new("spl-token-balances")
                .about("Balance of SPL token associated accounts")
                .arg(
                    Arg::new("input_csv")
                        .long("input-csv")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Allocations CSV file"),
                )
                .arg(
                    Arg::new("mint_address")
                        .long("mint")
                        .required(true)
                        .takes_value(true)
                        .value_name("MINT_ADDRESS")
                        .validator(|s| is_valid_pubkey(s))
                        .help("SPL token mint of distribution"),
                ),
        )
        .subcommand(
            clap::Command::new("transaction-log")
                .about("Print the database to a CSV file")
                .arg(
                    Arg::new("db_path")
                        .long("db-path")
                        .required(true)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Location of database to query"),
                )
                .arg(
                    Arg::new("output_path")
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
    matches: &ArgMatches,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let sender_keypair_str: String = matches.value_of_t_or_exit("sender_keypair");
    let sender_keypair = signer_from_path(
        &signer_matches,
        &sender_keypair_str,
        "sender",
        &mut wallet_manager,
    )?;

    let fee_payer_str: String = matches.value_of_t_or_exit("fee_payer");
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    Ok(DistributeTokensArgs {
        input_csv: matches.value_of_t_or_exit("input_csv"),
        transaction_db: matches.value_of_t_or_exit("db_path"),
        output_path: matches.value_of("output_path").map(|path| path.to_string()),
        dry_run: matches.is_present("dry_run"),
        sender_keypair,
        fee_payer,
        stake_args: None,
        spl_token_args: None,
        transfer_amount: value_of(matches, "transfer_amount").map(sol_to_lamports),
    })
}

fn parse_create_stake_args(matches: &ArgMatches) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let sender_keypair_str: String = matches.value_of_t_or_exit("sender_keypair");
    let sender_keypair = signer_from_path(
        &signer_matches,
        &sender_keypair_str,
        "sender",
        &mut wallet_manager,
    )?;

    let fee_payer_str: String = matches.value_of_t_or_exit("fee_payer");
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    let lockup_authority_str: Option<String> = matches.value_of_t("lockup_authority").ok();
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
        unlocked_sol: sol_to_lamports(matches.value_of_t_or_exit::<f64>("unlocked_sol")),
        lockup_authority,
        sender_stake_args: None,
    };
    Ok(DistributeTokensArgs {
        input_csv: matches.value_of_t_or_exit("input_csv"),
        transaction_db: matches.value_of_t_or_exit("db_path"),
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
    matches: &ArgMatches,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let sender_keypair_str: String = matches.value_of_t_or_exit("sender_keypair");
    let sender_keypair = signer_from_path(
        &signer_matches,
        &sender_keypair_str,
        "sender",
        &mut wallet_manager,
    )?;

    let fee_payer_str: String = matches.value_of_t_or_exit("fee_payer");
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    let stake_account_address_str: String = matches.value_of_t_or_exit("stake_account_address");
    let stake_account_address = pubkey_from_path(
        &signer_matches,
        &stake_account_address_str,
        "stake account address",
        &mut wallet_manager,
    )?;

    let stake_authority_str: String = matches.value_of_t_or_exit("stake_authority");
    let stake_authority = signer_from_path(
        &signer_matches,
        &stake_authority_str,
        "stake authority",
        &mut wallet_manager,
    )?;

    let withdraw_authority_str: String = matches.value_of_t_or_exit("withdraw_authority");
    let withdraw_authority = signer_from_path(
        &signer_matches,
        &withdraw_authority_str,
        "withdraw authority",
        &mut wallet_manager,
    )?;

    let lockup_authority_str: Option<String> = matches.value_of_t("lockup_authority").ok();
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
    };
    let stake_args = StakeArgs {
        unlocked_sol: sol_to_lamports(matches.value_of_t_or_exit::<f64>("unlocked_sol")),
        lockup_authority: lockup_authority_address,
        sender_stake_args: Some(sender_stake_args),
    };
    Ok(DistributeTokensArgs {
        input_csv: matches.value_of_t_or_exit("input_csv"),
        transaction_db: matches.value_of_t_or_exit("db_path"),
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
    matches: &ArgMatches,
) -> Result<DistributeTokensArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let signer_matches = ArgMatches::default(); // No default signer

    let token_owner_str: String = matches.value_of_t_or_exit("token_owner");
    let token_owner = signer_from_path(
        &signer_matches,
        &token_owner_str,
        "owner",
        &mut wallet_manager,
    )?;

    let fee_payer_str: String = matches.value_of_t_or_exit("fee_payer");
    let fee_payer = signer_from_path(
        &signer_matches,
        &fee_payer_str,
        "fee-payer",
        &mut wallet_manager,
    )?;

    let token_account_address_str: String = matches.value_of_t_or_exit("token_account_address");
    let token_account_address = pubkey_from_path(
        &signer_matches,
        &token_account_address_str,
        "token account address",
        &mut wallet_manager,
    )?;

    Ok(DistributeTokensArgs {
        input_csv: matches.value_of_t_or_exit("input_csv"),
        transaction_db: matches.value_of_t_or_exit("db_path"),
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

fn parse_balances_args(matches: &ArgMatches) -> Result<BalancesArgs, Box<dyn Error>> {
    let mut wallet_manager = maybe_wallet_manager()?;
    let spl_token_args =
        pubkey_of_signer(matches, "mint_address", &mut wallet_manager)?.map(|mint| SplTokenArgs {
            mint,
            ..SplTokenArgs::default()
        });
    Ok(BalancesArgs {
        input_csv: matches.value_of_t_or_exit("input_csv"),
        spl_token_args,
    })
}

fn parse_transaction_log_args(matches: &ArgMatches) -> TransactionLogArgs {
    TransactionLogArgs {
        transaction_db: matches.value_of_t_or_exit("db_path"),
        output_path: matches.value_of_t_or_exit("output_path"),
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
        Some(("distribute-tokens", matches)) => {
            Command::DistributeTokens(parse_distribute_tokens_args(matches)?)
        }
        Some(("create-stake", matches)) => {
            Command::DistributeTokens(parse_create_stake_args(matches)?)
        }
        Some(("distribute-stake", matches)) => {
            Command::DistributeTokens(parse_distribute_stake_args(matches)?)
        }
        Some(("distribute-spl-tokens", matches)) => {
            Command::DistributeTokens(parse_distribute_spl_tokens_args(matches)?)
        }
        Some(("balances", matches)) => Command::Balances(parse_balances_args(matches)?),
        Some(("spl-token-balances", matches)) => Command::Balances(parse_balances_args(matches)?),
        Some(("transaction-log", matches)) => {
            Command::TransactionLog(parse_transaction_log_args(matches))
        }
        _ => {
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
