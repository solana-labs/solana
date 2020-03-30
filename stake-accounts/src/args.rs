use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::input_validators::{is_amount, is_valid_pubkey, is_valid_signer};
use solana_cli_config::CONFIG_FILE;
use solana_sdk::native_token::sol_to_lamports;
use std::ffi::OsString;
use std::process::exit;

pub(crate) struct NewCommandConfig {
    pub fee_payer: String,
    pub funding_keypair: String,
    pub base_keypair: String,
    pub lamports: u64,
    pub stake_authority: String,
    pub withdraw_authority: String,
    pub index: usize,
}

pub(crate) struct CountCommandConfig {
    pub base_pubkey: String,
}

pub(crate) struct QueryCommandConfig {
    pub base_pubkey: String,
    pub num_accounts: usize,
}

pub(crate) struct AuthorizeCommandConfig {
    pub fee_payer: String,
    pub base_pubkey: String,
    pub stake_authority: String,
    pub withdraw_authority: String,
    pub new_stake_authority: String,
    pub new_withdraw_authority: String,
    pub num_accounts: usize,
}

pub(crate) struct RebaseCommandConfig {
    pub fee_payer: String,
    pub base_pubkey: String,
    pub new_base_keypair: String,
    pub stake_authority: String,
    pub num_accounts: usize,
}

pub(crate) struct MoveCommandConfig {
    pub rebase_config: RebaseCommandConfig,
    pub authorize_config: AuthorizeCommandConfig,
}

pub(crate) enum Command {
    New(NewCommandConfig),
    Count(CountCommandConfig),
    Addresses(QueryCommandConfig),
    Balance(QueryCommandConfig),
    Authorize(AuthorizeCommandConfig),
    Rebase(RebaseCommandConfig),
    Move(Box<MoveCommandConfig>),
}

pub(crate) struct CommandConfig {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command,
}

fn fee_payer_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("fee_payer")
        .long("fee-payer")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(is_valid_signer)
        .help("Fee payer")
}

fn funding_keypair_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("funding_keypair")
        .required(true)
        .takes_value(true)
        .value_name("FUNDING_KEYPAIR")
        .validator(is_valid_signer)
        .help("Keypair to fund accounts")
}

fn base_pubkey_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("base_pubkey")
        .required(true)
        .takes_value(true)
        .value_name("BASE_PUBKEY")
        .validator(is_valid_pubkey)
        .help("Public key which stake account addresses are derived from")
}

fn new_base_keypair_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("new_base_keypair")
        .required(true)
        .takes_value(true)
        .value_name("NEW_BASE_KEYPAIR")
        .validator(is_valid_signer)
        .help("New keypair which stake account addresses are derived from")
}

fn stake_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("stake_authority")
        .long("stake-authority")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(is_valid_signer)
        .help("Stake authority")
}

fn withdraw_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("withdraw_authority")
        .long("withdraw-authority")
        .required(true)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(is_valid_signer)
        .help("Withdraw authority")
}

fn new_stake_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("new_stake_authority")
        .long("new-stake-authority")
        .required(true)
        .takes_value(true)
        .value_name("PUBKEY")
        .validator(is_valid_pubkey)
        .help("New stake authority")
}

fn new_withdraw_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("new_withdraw_authority")
        .long("new-withdraw-authority")
        .required(true)
        .takes_value(true)
        .value_name("PUBKEY")
        .validator(is_valid_pubkey)
        .help("New withdraw authority")
}

fn num_accounts_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("num_accounts")
        .long("num-accounts")
        .required(true)
        .takes_value(true)
        .value_name("NUMBER")
        .help("Number of derived stake accounts")
}

pub(crate) fn get_matches<'a, I, T>(args: I) -> ArgMatches<'a>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let default_config_file = CONFIG_FILE.as_ref().unwrap();
    App::new("solana-stake-accounts")
        .about("about")
        .version("version")
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
            SubCommand::with_name("new")
                .about("Create derived stake accounts")
                .arg(fee_payer_arg())
                .arg(funding_keypair_arg().index(1))
                .arg(
                    Arg::with_name("base_keypair")
                        .required(true)
                        .index(2)
                        .takes_value(true)
                        .value_name("BASE_KEYPAIR")
                        .validator(is_valid_signer)
                        .help("Keypair which stake account addresses are derived from"),
                )
                .arg(
                    Arg::with_name("amount")
                        .required(true)
                        .index(3)
                        .takes_value(true)
                        .value_name("AMOUNT")
                        .validator(is_amount)
                        .help("Amount to move into the new stake accounts, in SOL"),
                )
                .arg(
                    Arg::with_name("stake_authority")
                        .long("stake-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_valid_pubkey)
                        .help("Stake authority"),
                )
                .arg(
                    Arg::with_name("withdraw_authority")
                        .long("withdraw-authority")
                        .required(true)
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_valid_pubkey)
                        .help("Withdraw authority"),
                )
                .arg(
                    Arg::with_name("index")
                        .long("index")
                        .takes_value(true)
                        .default_value("0")
                        .value_name("NUMBER")
                        .help("Index of the derived account to create"),
                ),
        )
        .subcommand(
            SubCommand::with_name("count")
                .about("Count derived stake accounts")
                .arg(base_pubkey_arg().index(1)),
        )
        .subcommand(
            SubCommand::with_name("addresses")
                .about("Show public keys of all derived stake accounts")
                .arg(base_pubkey_arg().index(1))
                .arg(num_accounts_arg()),
        )
        .subcommand(
            SubCommand::with_name("balance")
                .about("Sum balances of all derived stake accounts")
                .arg(base_pubkey_arg().index(1))
                .arg(num_accounts_arg()),
        )
        .subcommand(
            SubCommand::with_name("authorize")
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
            SubCommand::with_name("rebase")
                .about("Relocate derived stake accounts")
                .arg(fee_payer_arg())
                .arg(base_pubkey_arg().index(1))
                .arg(new_base_keypair_arg().index(2))
                .arg(stake_authority_arg())
                .arg(num_accounts_arg()),
        )
        .subcommand(
            SubCommand::with_name("move")
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

fn parse_new_args(matches: &ArgMatches<'_>) -> NewCommandConfig {
    let fee_payer = value_t_or_exit!(matches, "fee_payer", String);
    let funding_keypair = value_t_or_exit!(matches, "funding_keypair", String);
    let lamports = sol_to_lamports(value_t_or_exit!(matches, "amount", f64));
    let base_keypair = value_t_or_exit!(matches, "base_keypair", String);
    let stake_authority = value_t_or_exit!(matches, "stake_authority", String);
    let withdraw_authority = value_t_or_exit!(matches, "withdraw_authority", String);
    let index = value_t_or_exit!(matches, "index", usize);
    NewCommandConfig {
        fee_payer,
        funding_keypair,
        lamports,
        base_keypair,
        stake_authority,
        withdraw_authority,
        index,
    }
}

fn parse_count_args(matches: &ArgMatches<'_>) -> CountCommandConfig {
    let base_pubkey = value_t_or_exit!(matches, "base_pubkey", String);
    CountCommandConfig { base_pubkey }
}

fn parse_query_args(matches: &ArgMatches<'_>) -> QueryCommandConfig {
    let base_pubkey = value_t_or_exit!(matches, "base_pubkey", String);
    let num_accounts = value_t_or_exit!(matches, "num_accounts", usize);
    QueryCommandConfig {
        base_pubkey,
        num_accounts,
    }
}

fn parse_authorize_args(matches: &ArgMatches<'_>) -> AuthorizeCommandConfig {
    let fee_payer = value_t_or_exit!(matches, "fee_payer", String);
    let base_pubkey = value_t_or_exit!(matches, "base_pubkey", String);
    let stake_authority = value_t_or_exit!(matches, "stake_authority", String);
    let withdraw_authority = value_t_or_exit!(matches, "withdraw_authority", String);
    let new_stake_authority = value_t_or_exit!(matches, "new_stake_authority", String);
    let new_withdraw_authority = value_t_or_exit!(matches, "new_withdraw_authority", String);
    let num_accounts = value_t_or_exit!(matches, "num_accounts", usize);
    AuthorizeCommandConfig {
        fee_payer,
        base_pubkey,
        stake_authority,
        withdraw_authority,
        new_stake_authority,
        new_withdraw_authority,
        num_accounts,
    }
}

fn parse_rebase_args(matches: &ArgMatches<'_>) -> RebaseCommandConfig {
    let fee_payer = value_t_or_exit!(matches, "fee_payer", String);
    let base_pubkey = value_t_or_exit!(matches, "base_pubkey", String);
    let new_base_keypair = value_t_or_exit!(matches, "new_base_keypair", String);
    let stake_authority = value_t_or_exit!(matches, "stake_authority", String);
    let num_accounts = value_t_or_exit!(matches, "num_accounts", usize);
    RebaseCommandConfig {
        fee_payer,
        base_pubkey,
        new_base_keypair,
        stake_authority,
        num_accounts,
    }
}

fn parse_move_args(matches: &ArgMatches<'_>) -> MoveCommandConfig {
    let rebase_config = parse_rebase_args(matches);
    let authorize_config = parse_authorize_args(matches);
    MoveCommandConfig {
        rebase_config,
        authorize_config,
    }
}

pub(crate) fn parse_args<I, T>(args: I) -> CommandConfig
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = get_matches(args);
    let config_file = matches.value_of("config_file").unwrap().to_string();
    let url = matches.value_of("url").map(|x| x.to_string());

    let command = match matches.subcommand() {
        ("new", Some(matches)) => Command::New(parse_new_args(matches)),
        ("count", Some(matches)) => Command::Count(parse_count_args(matches)),
        ("addresses", Some(matches)) => Command::Addresses(parse_query_args(matches)),
        ("balance", Some(matches)) => Command::Balance(parse_query_args(matches)),
        ("authorize", Some(matches)) => Command::Authorize(parse_authorize_args(matches)),
        ("rebase", Some(matches)) => Command::Rebase(parse_rebase_args(matches)),
        ("move", Some(matches)) => Command::Move(Box::new(parse_move_args(matches))),
        _ => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
    };
    CommandConfig {
        config_file,
        url,
        command,
    }
}
