use {
    crate::args::{
        Args, AuthorizeArgs, Command, CountArgs, MoveArgs, NewArgs, QueryArgs, RebaseArgs,
        SetLockupArgs,
    },
    clap::{Arg, ArgMatches},
    solana_clap_utils::{
        input_parsers::unix_timestamp_from_rfc3339_datetime,
        input_validators::{is_amount, is_rfc3339_datetime, is_valid_pubkey, is_valid_signer},
    },
    solana_cli_config::CONFIG_FILE,
    solana_sdk::native_token::sol_to_lamports,
    std::{ffi::OsString, process::exit},
};

fn fee_payer_arg<'a>() -> Arg<'a> {
    solana_clap_utils::fee_payer::fee_payer_arg().required(true)
}

fn funding_keypair_arg<'a>() -> Arg<'a> {
    Arg::new("funding_keypair")
        .required(true)
        .takes_value(true)
        .value_name("FUNDING_KEYPAIR")
        .validator(|s| is_valid_signer(s))
        .help("Keypair to fund accounts")
}

fn base_pubkey_arg<'a>() -> Arg<'a> {
    Arg::new("base_pubkey")
        .required(true)
        .takes_value(true)
        .value_name("BASE_PUBKEY")
        .validator(|s| is_valid_pubkey(s))
        .help("Public key which stake account addresses are derived from")
}

fn custodian_arg<'a>() -> Arg<'a> {
    Arg::new("custodian")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(|s| is_valid_signer(s))
        .help("Authority to modify lockups")
}

fn new_custodian_arg<'a>() -> Arg<'a> {
    Arg::new("new_custodian")
        .takes_value(true)
        .value_name("PUBKEY")
        .validator(|s| is_valid_pubkey(s))
        .help("New authority to modify lockups")
}

fn new_base_keypair_arg<'a>() -> Arg<'a> {
    Arg::new("new_base_keypair")
        .required(true)
        .takes_value(true)
        .value_name("NEW_BASE_KEYPAIR")
        .validator(|s| is_valid_signer(s))
        .help("New keypair which stake account addresses are derived from")
}

fn stake_authority_arg<'a>() -> Arg<'a> {
    Arg::new("stake_authority")
        .long("stake-authority")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(|s| is_valid_signer(s))
        .help("Stake authority")
}

fn withdraw_authority_arg<'a>() -> Arg<'a> {
    Arg::new("withdraw_authority")
        .long("withdraw-authority")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(|s| is_valid_signer(s))
        .help("Withdraw authority")
}

fn new_stake_authority_arg<'a>() -> Arg<'a> {
    Arg::new("new_stake_authority")
        .long("new-stake-authority")
        .required(true)
        .takes_value(true)
        .value_name("PUBKEY")
        .validator(|s| is_valid_pubkey(s))
        .help("New stake authority")
}

fn new_withdraw_authority_arg<'a>() -> Arg<'a> {
    Arg::new("new_withdraw_authority")
        .long("new-withdraw-authority")
        .required(true)
        .takes_value(true)
        .value_name("PUBKEY")
        .validator(|s| is_valid_pubkey(s))
        .help("New withdraw authority")
}

fn lockup_epoch_arg<'a>() -> Arg<'a> {
    Arg::new("lockup_epoch")
        .long("lockup-epoch")
        .takes_value(true)
        .value_name("NUMBER")
        .help("The epoch height at which each account will be available for withdrawl")
}

fn lockup_date_arg<'a>() -> Arg<'a> {
    Arg::new("lockup_date")
        .long("lockup-date")
        .value_name("RFC3339 DATETIME")
        .validator(|s| is_rfc3339_datetime(s))
        .takes_value(true)
        .help("The date and time at which each account will be available for withdrawl")
}

fn num_accounts_arg<'a>() -> Arg<'a> {
    Arg::new("num_accounts")
        .long("num-accounts")
        .required(true)
        .takes_value(true)
        .value_name("NUMBER")
        .help("Number of derived stake accounts")
}

pub(crate) fn get_matches<I, T>(args: I) -> ArgMatches
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let default_config_file = CONFIG_FILE.as_ref().unwrap();
    clap::Command::new("solana-stake-accounts")
        .about("about")
        .version("version")
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
            clap::Command::new("new")
                .about("Create derived stake accounts")
                .arg(fee_payer_arg())
                .arg(funding_keypair_arg().index(1))
                .arg(
                    Arg::new("base_keypair")
                        .required(true)
                        .index(2)
                        .takes_value(true)
                        .value_name("BASE_KEYPAIR")
                        .validator(|s| is_valid_signer(s))
                        .help("Keypair which stake account addresses are derived from"),
                )
                .arg(
                    Arg::new("amount")
                        .required(true)
                        .index(3)
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(|s| is_amount(s))
                        .help("Amount to move into the new stake accounts, in SOL"),
                )
                .arg(
                    Arg::new("stake_authority")
                        .long("stake-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(|s| is_valid_pubkey(s))
                        .help("Stake authority"),
                )
                .arg(
                    Arg::new("withdraw_authority")
                        .long("withdraw-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(|s| is_valid_pubkey(s))
                        .help("Withdraw authority"),
                )
                .arg(
                    Arg::new("index")
                        .long("index")
                        .takes_value(true)
                        .default_value("0")
                        .value_name("NUMBER")
                        .help("Index of the derived account to create"),
                ),
        )
        .subcommand(
            clap::Command::new("count")
                .about("Count derived stake accounts")
                .arg(base_pubkey_arg().index(1)),
        )
        .subcommand(
            clap::Command::new("addresses")
                .about("Show public keys of all derived stake accounts")
                .arg(base_pubkey_arg().index(1))
                .arg(num_accounts_arg()),
        )
        .subcommand(
            clap::Command::new("balance")
                .about("Sum balances of all derived stake accounts")
                .arg(base_pubkey_arg().index(1))
                .arg(num_accounts_arg()),
        )
        .subcommand(
            clap::Command::new("authorize")
                .about("Set new authorities in all derived stake accounts")
                .arg(fee_payer_arg())
                .arg(base_pubkey_arg().index(1))
                .arg(stake_authority_arg())
                .arg(withdraw_authority_arg())
                .arg(new_stake_authority_arg())
                .arg(new_withdraw_authority_arg())
                .arg(num_accounts_arg()),
        )
        .subcommand(
            clap::Command::new("set-lockup")
                .about("Set new lockups in all derived stake accounts")
                .arg(fee_payer_arg())
                .arg(base_pubkey_arg().index(1))
                .arg(custodian_arg())
                .arg(lockup_epoch_arg())
                .arg(lockup_date_arg())
                .arg(new_custodian_arg())
                .arg(num_accounts_arg())
                .arg(
                    Arg::new("no_wait")
                        .long("no-wait")
                        .help("Send transactions without waiting for confirmation"),
                )
                .arg(
                    Arg::new("unlock_years")
                        .long("unlock-years")
                        .takes_value(true)
                        .value_name("NUMBER")
                        .help("Years to unlock after the cliff"),
                ),
        )
        .subcommand(
            clap::Command::new("rebase")
                .about("Relocate derived stake accounts")
                .arg(fee_payer_arg())
                .arg(base_pubkey_arg().index(1))
                .arg(new_base_keypair_arg().index(2))
                .arg(stake_authority_arg())
                .arg(num_accounts_arg()),
        )
        .subcommand(
            clap::Command::new("move")
                .about("Rebase and set new authorities in all derived stake accounts")
                .arg(fee_payer_arg())
                .arg(base_pubkey_arg().index(1))
                .arg(new_base_keypair_arg().index(2))
                .arg(stake_authority_arg())
                .arg(withdraw_authority_arg())
                .arg(new_stake_authority_arg())
                .arg(new_withdraw_authority_arg())
                .arg(num_accounts_arg()),
        )
        .get_matches_from(args)
}

fn parse_new_args(matches: &ArgMatches) -> NewArgs<String, String> {
    NewArgs {
        fee_payer: matches.value_of_t_or_exit("fee_payer"),
        funding_keypair: matches.value_of_t_or_exit("funding_keypair"),
        lamports: sol_to_lamports(matches.value_of_t_or_exit::<f64>("amount")),
        base_keypair: matches.value_of_t_or_exit("base_keypair"),
        stake_authority: matches.value_of_t_or_exit("stake_authority"),
        withdraw_authority: matches.value_of_t_or_exit("withdraw_authority"),
        index: matches.value_of_t_or_exit("index"),
    }
}

fn parse_count_args(matches: &ArgMatches) -> CountArgs<String> {
    CountArgs {
        base_pubkey: matches.value_of_t_or_exit("base_pubkey"),
    }
}

fn parse_query_args(matches: &ArgMatches) -> QueryArgs<String> {
    QueryArgs {
        base_pubkey: matches.value_of_t_or_exit("base_pubkey"),
        num_accounts: matches.value_of_t_or_exit("num_accounts"),
    }
}

fn parse_authorize_args(matches: &ArgMatches) -> AuthorizeArgs<String, String> {
    AuthorizeArgs {
        fee_payer: matches.value_of_t_or_exit("fee_payer"),
        base_pubkey: matches.value_of_t_or_exit("base_pubkey"),
        stake_authority: matches.value_of_t_or_exit("stake_authority"),
        withdraw_authority: matches.value_of_t_or_exit("withdraw_authority"),
        new_stake_authority: matches.value_of_t_or_exit("new_stake_authority"),
        new_withdraw_authority: matches.value_of_t_or_exit("new_withdraw_authority"),
        num_accounts: matches.value_of_t_or_exit("num_accounts"),
    }
}

fn parse_set_lockup_args(matches: &ArgMatches) -> SetLockupArgs<String, String> {
    SetLockupArgs {
        fee_payer: matches.value_of_t_or_exit("fee_payer"),
        base_pubkey: matches.value_of_t_or_exit("base_pubkey"),
        custodian: matches.value_of_t_or_exit("custodian"),
        lockup_epoch: matches.value_of_t("lockup_epoch").ok(),
        lockup_date: unix_timestamp_from_rfc3339_datetime(matches, "lockup_date"),
        new_custodian: matches.value_of_t("new_custodian").ok(),
        num_accounts: matches.value_of_t_or_exit("num_accounts"),
        no_wait: matches.is_present("no_wait"),
        unlock_years: matches.value_of_t("unlock_years").ok(),
    }
}

fn parse_rebase_args(matches: &ArgMatches) -> RebaseArgs<String, String> {
    RebaseArgs {
        fee_payer: matches.value_of_t_or_exit("fee_payer"),
        base_pubkey: matches.value_of_t_or_exit("base_pubkey"),
        new_base_keypair: matches.value_of_t_or_exit("new_base_keypair"),
        stake_authority: matches.value_of_t_or_exit("stake_authority"),
        num_accounts: matches.value_of_t_or_exit("num_accounts"),
    }
}

fn parse_move_args(matches: &ArgMatches) -> MoveArgs<String, String> {
    MoveArgs {
        rebase_args: parse_rebase_args(matches),
        authorize_args: parse_authorize_args(matches),
    }
}

pub(crate) fn parse_args<I, T>(args: I) -> Args<String, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = get_matches(args);
    let config_file = matches.value_of("config_file").unwrap().to_string();
    let url = matches.value_of("url").map(|x| x.to_string());

    let command = match matches.subcommand() {
        Some(("new", matches)) => Command::New(parse_new_args(matches)),
        Some(("count", matches)) => Command::Count(parse_count_args(matches)),
        Some(("addresses", matches)) => Command::Addresses(parse_query_args(matches)),
        Some(("balance", matches)) => Command::Balance(parse_query_args(matches)),
        Some(("authorize", matches)) => Command::Authorize(parse_authorize_args(matches)),
        Some(("set-lockup", matches)) => Command::SetLockup(parse_set_lockup_args(matches)),
        Some(("rebase", matches)) => Command::Rebase(parse_rebase_args(matches)),
        Some(("move", matches)) => Command::Move(Box::new(parse_move_args(matches))),
        _ => {
            exit(1);
        }
    };
    Args {
        config_file,
        url,
        command,
    }
}
