use crate::{
    cli::{
        build_balance_message, check_account_for_fee, check_unique_pubkeys, fee_payer_arg,
        log_instruction_custom_error, nonce_authority_arg, replace_signatures,
        required_lamports_from, return_signers, CliCommand, CliCommandInfo, CliConfig, CliError,
        ProcessResult, SigningAuthority, FEE_PAYER_ARG,
    },
    nonce::{check_nonce_account, nonce_arg, NONCE_ARG, NONCE_AUTHORITY_ARG},
    offline::*,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use console::style;
use solana_clap_utils::{input_parsers::*, input_validators::*, ArgConstant};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::{
    account_utils::StateMut,
    pubkey::Pubkey,
    signature::KeypairUtil,
    system_instruction::{create_address_with_seed, SystemError},
    sysvar::{
        stake_history::{self, StakeHistory},
        Sysvar,
    },
    transaction::Transaction,
};
use solana_stake_program::{
    stake_instruction::{self, StakeError},
    stake_state::{Authorized, Lockup, Meta, StakeAuthorize, StakeState},
};
use solana_vote_program::vote_state::VoteState;
use std::ops::Deref;

pub const STAKE_AUTHORITY_ARG: ArgConstant<'static> = ArgConstant {
    name: "stake_authority",
    long: "stake-authority",
    help: "Public key of authorized staker (defaults to cli config pubkey)",
};

pub const WITHDRAW_AUTHORITY_ARG: ArgConstant<'static> = ArgConstant {
    name: "withdraw_authority",
    long: "withdraw-authority",
    help: "Public key of authorized withdrawer (defaults to cli config pubkey)",
};

fn stake_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(STAKE_AUTHORITY_ARG.name)
        .long(STAKE_AUTHORITY_ARG.long)
        .takes_value(true)
        .value_name("KEYPAIR or PUBKEY")
        .validator(is_pubkey_or_keypair_or_ask_keyword)
        .help(STAKE_AUTHORITY_ARG.help)
}

fn withdraw_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(WITHDRAW_AUTHORITY_ARG.name)
        .long(WITHDRAW_AUTHORITY_ARG.long)
        .takes_value(true)
        .value_name("KEYPAIR or PUBKEY")
        .validator(is_pubkey_or_keypair_or_ask_keyword)
        .help(WITHDRAW_AUTHORITY_ARG.help)
}

pub trait StakeSubCommands {
    fn stake_subcommands(self) -> Self;
}

impl StakeSubCommands for App<'_, '_> {
    fn stake_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("create-stake-account")
                .about("Create a stake account")
                .arg(
                    Arg::with_name("stake_account")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair_or_ask_keyword)
                        .help("Signing authority of the stake address to fund")
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount)
                        .required(true)
                        .help("The amount of send to the vote account (default unit SOL)")
                )
                .arg(
                    Arg::with_name("unit")
                        .index(3)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request")
                )
                .arg(
                    Arg::with_name("custodian")
                        .long("custodian")
                        .value_name("KEYPAIR or PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Identity of the custodian (can withdraw before lockup expires)")
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("SEED STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the STAKE ACCOUNT pubkey")
                )
                .arg(
                    Arg::with_name("lockup_epoch")
                        .long("lockup-epoch")
                        .value_name("EPOCH")
                        .takes_value(true)
                        .help("The epoch height at which this account will be available for withdrawal")
                )
                .arg(
                    Arg::with_name("lockup_date")
                        .long("lockup-date")
                        .value_name("RFC3339 DATE TIME")
                        .validator(is_rfc3339_datetime)
                        .takes_value(true)
                        .help("The date and time at which this account will be available for withdrawal")
                )
                .arg(
                    Arg::with_name(STAKE_AUTHORITY_ARG.name)
                        .long(STAKE_AUTHORITY_ARG.long)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help(STAKE_AUTHORITY_ARG.help)
                )
                .arg(
                    Arg::with_name(WITHDRAW_AUTHORITY_ARG.name)
                        .long(WITHDRAW_AUTHORITY_ARG.long)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help(WITHDRAW_AUTHORITY_ARG.help)
                )
                .arg(
                    Arg::with_name("from")
                        .long("from")
                        .takes_value(true)
                        .value_name("KEYPAIR or PUBKEY")
                        .validator(is_pubkey_or_keypair_or_ask_keyword)
                        .help("Source account of funds (if different from client local account)"),
                )
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("delegate-stake")
                .about("Delegate stake to a vote account")
                .arg(
                    Arg::with_name("force")
                        .long("force")
                        .takes_value(false)
                        .hidden(true) // Don't document this argument to discourage its use
                        .help("Override vote account sanity checks (use carefully!)")
                )
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account to delegate")
                )
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(2)
                        .value_name("VOTE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The vote account to which the stake will be delegated")
                )
                .arg(stake_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("stake-authorize-staker")
                .about("Authorize a new stake signing keypair for the given stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account in which to set the authorized staker")
                )
                .arg(
                    Arg::with_name("authorized_pubkey")
                        .index(2)
                        .value_name("AUTHORIZE PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New authorized staker")
                )
                .arg(stake_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("stake-authorize-withdrawer")
                .about("Authorize a new withdraw signing keypair for the given stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account in which to set the authorized withdrawer")
                )
                .arg(
                    Arg::with_name("authorized_pubkey")
                        .index(2)
                        .value_name("AUTHORIZE PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New authorized withdrawer")
                )
                .arg(withdraw_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("deactivate-stake")
                .about("Deactivate the delegated stake from the stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .help("Stake account to be deactivated.")
                )
                .arg(stake_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("split-stake")
                .about("Split a stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .help("Stake account to be split")
                )
                .arg(
                    Arg::with_name("split_stake_account")
                        .index(2)
                        .value_name("SPLIT STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword)
                        .help("Keypair of the new stake account to split funds into")
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount)
                        .required(true)
                        .help("The amount to move into the new stake account (default unit SOL)")
                )
                .arg(
                    Arg::with_name("unit")
                        .index(4)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request")
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("SEED STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the SPLIT STAKE ACCOUNT pubkey")
                )
                .arg(stake_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("withdraw-stake")
                .about("Withdraw the unstaked lamports from the stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account from which to withdraw")
                )
                .arg(
                    Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("DESTINATION ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The account to which the lamports should be transferred")
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount)
                        .required(true)
                        .help("The amount to withdraw from the stake account (default unit SOL)")
                )
                .arg(
                    Arg::with_name("unit")
                        .index(4)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request")
                )
                .arg(withdraw_authority_arg())
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("stake-set-lockup")
                .about("Set Lockup for the stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account for which to set Lockup")
                )
                .arg(
                    Arg::with_name("lockup_epoch")
                        .long("lockup-epoch")
                        .value_name("EPOCH")
                        .takes_value(true)
                        .help("The epoch height at which this account will be available for withdrawal")
                )
                .arg(
                    Arg::with_name("lockup_date")
                        .long("lockup-date")
                        .value_name("RFC3339 DATE TIME")
                        .validator(is_rfc3339_datetime)
                        .takes_value(true)
                        .help("The date and time at which this account will be available for withdrawal")
                )
                .arg(
                    Arg::with_name("new_custodian")
                        .long("new-custodian")
                        .value_name("KEYPAIR or PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Identity of the new lockup custodian (can withdraw before lockup expires)")
                )
                .arg(
                    Arg::with_name("custodian")
                        .long("custodian")
                        .takes_value(true)
                        .value_name("KEYPAIR or PUBKEY")
                        .validator(is_pubkey_or_keypair_or_ask_keyword)
                        .help("Public key of signing custodian (defaults to cli config pubkey)")
                )
                .offline_args()
                .arg(nonce_arg())
                .arg(nonce_authority_arg())
                .arg(fee_payer_arg())
        )
        .subcommand(
            SubCommand::with_name("stake-account")
                .about("Show the contents of a stake account")
                .alias("show-stake-account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the stake account to display")
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL")
                )
        )
        .subcommand(
            SubCommand::with_name("stake-history")
                .about("Show the stake history")
                .alias("show-stake-history")
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL")
                )
        )
    }
}

pub fn parse_stake_create_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let seed = matches.value_of("seed").map(|s| s.to_string());
    let epoch = value_of(&matches, "lockup_epoch").unwrap_or(0);
    let unix_timestamp = unix_timestamp_from_rfc3339_datetime(&matches, "lockup_date").unwrap_or(0);
    let custodian = pubkey_of(matches, "custodian").unwrap_or_default();
    let staker = pubkey_of(matches, STAKE_AUTHORITY_ARG.name);
    let withdrawer = pubkey_of(matches, WITHDRAW_AUTHORITY_ARG.name);
    let lamports = required_lamports_from(matches, "amount", "unit")?;
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;
    let from = SigningAuthority::new_from_matches(&matches, "from", signers.as_deref())?;
    let stake_account =
        SigningAuthority::new_from_matches(&matches, "stake_account", signers.as_deref())
            .unwrap()
            .unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::CreateStakeAccount {
            stake_account,
            seed,
            staker,
            withdrawer,
            lockup: Lockup {
                custodian,
                epoch,
                unix_timestamp,
            },
            lamports,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
            from,
        },
        require_keypair,
    })
}

pub fn parse_stake_delegate_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let force = matches.is_present("force");
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let stake_authority =
        SigningAuthority::new_from_matches(&matches, STAKE_AUTHORITY_ARG.name, signers.as_deref())?;
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::DelegateStake {
            stake_account_pubkey,
            vote_account_pubkey,
            stake_authority,
            force,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
        },
        require_keypair,
    })
}

pub fn parse_stake_authorize(
    matches: &ArgMatches<'_>,
    stake_authorize: StakeAuthorize,
) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let new_authorized_pubkey = pubkey_of(matches, "authorized_pubkey").unwrap();
    let authority_flag = match stake_authorize {
        StakeAuthorize::Staker => STAKE_AUTHORITY_ARG.name,
        StakeAuthorize::Withdrawer => WITHDRAW_AUTHORITY_ARG.name,
    };
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let authority =
        SigningAuthority::new_from_matches(&matches, authority_flag, signers.as_deref())?;
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::StakeAuthorize {
            stake_account_pubkey,
            new_authorized_pubkey,
            stake_authorize,
            authority,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
        },
        require_keypair: true,
    })
}

pub fn parse_split_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let split_stake_account = keypair_of(matches, "split_stake_account").unwrap();
    let lamports = required_lamports_from(matches, "amount", "unit")?;
    let seed = matches.value_of("seed").map(|s| s.to_string());

    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let stake_authority =
        SigningAuthority::new_from_matches(&matches, STAKE_AUTHORITY_ARG.name, signers.as_deref())?;
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::SplitStake {
            stake_account_pubkey,
            stake_authority,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            split_stake_account: split_stake_account.into(),
            seed,
            lamports,
            fee_payer,
        },
        require_keypair,
    })
}

pub fn parse_stake_deactivate_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let stake_authority =
        SigningAuthority::new_from_matches(&matches, STAKE_AUTHORITY_ARG.name, signers.as_deref())?;
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::DeactivateStake {
            stake_account_pubkey,
            stake_authority,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
        },
        require_keypair,
    })
}

pub fn parse_stake_withdraw_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let destination_account_pubkey = pubkey_of(matches, "destination_account_pubkey").unwrap();
    let lamports = required_lamports_from(matches, "amount", "unit")?;
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let withdraw_authority = SigningAuthority::new_from_matches(
        &matches,
        WITHDRAW_AUTHORITY_ARG.name,
        signers.as_deref(),
    )?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawStake {
            stake_account_pubkey,
            destination_account_pubkey,
            lamports,
            withdraw_authority,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
        },
        require_keypair,
    })
}

pub fn parse_stake_set_lockup(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let epoch = value_of(&matches, "lockup_epoch").unwrap_or(0);
    let unix_timestamp = unix_timestamp_from_rfc3339_datetime(&matches, "lockup_date").unwrap_or(0);
    let new_custodian = pubkey_of(matches, "new_custodian").unwrap_or_default();

    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let signers = pubkeys_sigs_of(&matches, SIGNER_ARG.name);
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let require_keypair = signers.is_none();
    let nonce_account = pubkey_of(&matches, NONCE_ARG.name);

    let custodian = SigningAuthority::new_from_matches(&matches, "custodian", signers.as_deref())?;
    let nonce_authority =
        SigningAuthority::new_from_matches(&matches, NONCE_AUTHORITY_ARG.name, signers.as_deref())?;
    let fee_payer =
        SigningAuthority::new_from_matches(&matches, FEE_PAYER_ARG.name, signers.as_deref())?;

    Ok(CliCommandInfo {
        command: CliCommand::StakeSetLockup {
            stake_account_pubkey,
            lockup: Lockup {
                custodian: new_custodian,
                epoch,
                unix_timestamp,
            },
            custodian,
            sign_only,
            signers,
            blockhash_query,
            nonce_account,
            nonce_authority,
            fee_payer,
        },
        require_keypair,
    })
}

pub fn parse_show_stake_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let use_lamports_unit = matches.is_present("lamports");
    Ok(CliCommandInfo {
        command: CliCommand::ShowStakeAccount {
            pubkey: stake_account_pubkey,
            use_lamports_unit,
        },
        require_keypair: false,
    })
}

pub fn parse_show_stake_history(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let use_lamports_unit = matches.is_present("lamports");
    Ok(CliCommandInfo {
        command: CliCommand::ShowStakeHistory { use_lamports_unit },
        require_keypair: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn process_create_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account: &SigningAuthority,
    seed: &Option<String>,
    staker: &Option<Pubkey>,
    withdrawer: &Option<Pubkey>,
    lockup: &Lockup,
    lamports: u64,
    sign_only: bool,
    signers: Option<&Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<&Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
    from: Option<&SigningAuthority>,
) -> ProcessResult {
    // Offline derived address creation currently is not possible
    // https://github.com/solana-labs/solana/pull/8252
    if seed.is_some() && (sign_only || signers.is_some()) {
        return Err(CliError::BadParameter(
            "Offline stake account creation with derived addresses are not yet supported"
                .to_string(),
        )
        .into());
    }

    let (stake_account_pubkey, stake_account) = (stake_account.pubkey(), stake_account.keypair());
    let stake_account_address = if let Some(seed) = seed {
        create_address_with_seed(&stake_account_pubkey, &seed, &solana_stake_program::id())?
    } else {
        stake_account_pubkey
    };
    let (from_pubkey, from) = from
        .map(|f| (f.pubkey(), f.keypair()))
        .unwrap_or((config.keypair.pubkey(), &config.keypair));
    check_unique_pubkeys(
        (&from_pubkey, "from keypair".to_string()),
        (&stake_account_address, "stake_account".to_string()),
    )?;

    if !sign_only {
        if let Ok(stake_account) = rpc_client.get_account(&stake_account_address) {
            let err_msg = if stake_account.owner == solana_stake_program::id() {
                format!("Stake account {} already exists", stake_account_address)
            } else {
                format!(
                    "Account {} already exists and is not a stake account",
                    stake_account_address
                )
            };
            return Err(CliError::BadParameter(err_msg).into());
        }

        let minimum_balance =
            rpc_client.get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())?;

        if lamports < minimum_balance {
            return Err(CliError::BadParameter(format!(
                "need at least {} lamports for stake account to be rent exempt, provided lamports: {}",
                minimum_balance, lamports
            ))
            .into());
        }
    }

    let authorized = Authorized {
        staker: staker.unwrap_or(from_pubkey),
        withdrawer: withdrawer.unwrap_or(from_pubkey),
    };

    let ixs = if let Some(seed) = seed {
        stake_instruction::create_account_with_seed(
            &from.pubkey(),          // from
            &stake_account_address,  // to
            &stake_account.pubkey(), // base
            seed,                    // seed
            &authorized,
            lockup,
            lamports,
        )
    } else {
        stake_instruction::create_account(
            &from.pubkey(),
            &stake_account.pubkey(),
            &authorized,
            lockup,
            lamports,
        )
    };
    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;

    let fee_payer = fee_payer.map(|fp| fp.keypair()).unwrap_or(&config.keypair);
    let mut tx_signers = if stake_account_pubkey != from_pubkey {
        vec![fee_payer, from, stake_account] // both must sign if `from` and `to` differ
    } else {
        vec![fee_payer, from] // when stake_account == config.keypair and there's a seed, we only need one signature
    };
    let (nonce_authority_pubkey, nonce_authority) = nonce_authority
        .map(|na| (na.pubkey(), na.keypair()))
        .unwrap_or((config.keypair.pubkey(), &config.keypair));

    let mut tx = if let Some(nonce_account) = &nonce_account {
        tx_signers.push(nonce_authority);
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &tx_signers,
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &tx_signers,
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &tx_signers);
        log_instruction_custom_error::<SystemError>(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process_stake_authorize(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    stake_authorize: StakeAuthorize,
    authority: Option<&SigningAuthority>,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    check_unique_pubkeys(
        (stake_account_pubkey, "stake_account_pubkey".to_string()),
        (authorized_pubkey, "new_authorized_pubkey".to_string()),
    )?;
    let authority = authority.map(|a| a.keypair()).unwrap_or(&config.keypair);
    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;
    let ixs = vec![stake_instruction::authorize(
        stake_account_pubkey, // stake account to update
        &authority.pubkey(),  // currently authorized
        authorized_pubkey,    // new stake signer
        stake_authorize,      // stake or withdraw
    )];

    let (nonce_authority, nonce_authority_pubkey) = nonce_authority
        .map(|a| (a.keypair(), a.pubkey()))
        .unwrap_or((&config.keypair, config.keypair.pubkey()));
    let fee_payer = fee_payer.map(|f| f.keypair()).unwrap_or(&config.keypair);
    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, nonce_authority, authority],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, authority],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<StakeError>(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process_deactivate_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    stake_authority: Option<&SigningAuthority>,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;
    let stake_authority = stake_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);
    let ixs = vec![stake_instruction::deactivate_stake(
        stake_account_pubkey,
        &stake_authority.pubkey(),
    )];
    let (nonce_authority, nonce_authority_pubkey) = nonce_authority
        .map(|a| (a.keypair(), a.pubkey()))
        .unwrap_or((&config.keypair, config.keypair.pubkey()));
    let fee_payer = fee_payer.map(|f| f.keypair()).unwrap_or(&config.keypair);
    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, nonce_authority, stake_authority],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, stake_authority],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<StakeError>(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process_withdraw_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
    withdraw_authority: Option<&SigningAuthority>,
    sign_only: bool,
    signers: Option<&Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<&Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;
    let withdraw_authority = withdraw_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);

    let ixs = vec![stake_instruction::withdraw(
        stake_account_pubkey,
        &withdraw_authority.pubkey(),
        destination_account_pubkey,
        lamports,
    )];

    let fee_payer = fee_payer.map(|fp| fp.keypair()).unwrap_or(&config.keypair);
    let (nonce_authority_pubkey, nonce_authority) = nonce_authority
        .map(|na| (na.pubkey(), na.keypair()))
        .unwrap_or((config.keypair.pubkey(), &config.keypair));
    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, withdraw_authority, nonce_authority],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, withdraw_authority],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<SystemError>(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process_split_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    stake_authority: Option<&SigningAuthority>,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    split_stake_account: &Keypair,
    split_stake_account_seed: &Option<String>,
    lamports: u64,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    let (fee_payer_pubkey, fee_payer) = fee_payer
        .map(|f| (f.pubkey(), f.keypair()))
        .unwrap_or((config.keypair.pubkey(), &config.keypair));
    check_unique_pubkeys(
        (&fee_payer_pubkey, "fee-payer keypair".to_string()),
        (
            &split_stake_account.pubkey(),
            "split_stake_account".to_string(),
        ),
    )?;
    check_unique_pubkeys(
        (&fee_payer_pubkey, "fee-payer keypair".to_string()),
        (&stake_account_pubkey, "stake_account".to_string()),
    )?;
    check_unique_pubkeys(
        (&stake_account_pubkey, "stake_account".to_string()),
        (
            &split_stake_account.pubkey(),
            "split_stake_account".to_string(),
        ),
    )?;

    let stake_authority = stake_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);

    let split_stake_account_address = if let Some(seed) = split_stake_account_seed {
        create_address_with_seed(
            &split_stake_account.pubkey(),
            &seed,
            &solana_stake_program::id(),
        )?
    } else {
        split_stake_account.pubkey()
    };

    if !sign_only {
        if let Ok(stake_account) = rpc_client.get_account(&split_stake_account_address) {
            let err_msg = if stake_account.owner == solana_stake_program::id() {
                format!(
                    "Stake account {} already exists",
                    split_stake_account_address
                )
            } else {
                format!(
                    "Account {} already exists and is not a stake account",
                    split_stake_account_address
                )
            };
            return Err(CliError::BadParameter(err_msg).into());
        }

        let minimum_balance =
            rpc_client.get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())?;

        if lamports < minimum_balance {
            return Err(CliError::BadParameter(format!(
                "need at least {} lamports for stake account to be rent exempt, provided lamports: {}",
                minimum_balance, lamports
            ))
            .into());
        }
    }

    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;

    let ixs = if let Some(seed) = split_stake_account_seed {
        stake_instruction::split_with_seed(
            &stake_account_pubkey,
            &stake_authority.pubkey(),
            lamports,
            &split_stake_account_address,
            &split_stake_account.pubkey(),
            seed,
        )
    } else {
        stake_instruction::split(
            &stake_account_pubkey,
            &stake_authority.pubkey(),
            lamports,
            &split_stake_account_address,
        )
    };

    let (nonce_authority, nonce_authority_pubkey) = nonce_authority
        .map(|a| (a.keypair(), a.pubkey()))
        .unwrap_or((&config.keypair, config.keypair.pubkey()));

    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[
                fee_payer,
                nonce_authority,
                stake_authority,
                split_stake_account,
            ],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, stake_authority, split_stake_account],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<StakeError>(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process_stake_set_lockup(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    lockup: &mut Lockup,
    custodian: Option<&SigningAuthority>,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;
    let custodian = custodian.map(|a| a.keypair()).unwrap_or(&config.keypair);
    // If new custodian is not explicitly set, default to current custodian
    if lockup.custodian == Pubkey::default() {
        lockup.custodian = custodian.pubkey();
    }
    let ixs = vec![stake_instruction::set_lockup(
        stake_account_pubkey,
        lockup,
        &custodian.pubkey(),
    )];
    let (nonce_authority, nonce_authority_pubkey) = nonce_authority
        .map(|a| (a.keypair(), a.pubkey()))
        .unwrap_or((&config.keypair, config.keypair.pubkey()));
    let fee_payer = fee_payer.map(|f| f.keypair()).unwrap_or(&config.keypair);
    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, nonce_authority, custodian],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, custodian],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<StakeError>(result)
    }
}

pub fn print_stake_state(stake_lamports: u64, stake_state: &StakeState, use_lamports_unit: bool) {
    fn show_authorized(authorized: &Authorized) {
        println!("Authorized Staker: {}", authorized.staker);
        println!("Authorized Withdrawer: {}", authorized.withdrawer);
    }
    fn show_lockup(lockup: &Lockup) {
        println!("Lockup Epoch: {}", lockup.epoch);
        println!("Lockup Custodian: {}", lockup.custodian);
    }
    match stake_state {
        StakeState::Stake(
            Meta {
                authorized, lockup, ..
            },
            stake,
        ) => {
            println!(
                "Total Stake: {}",
                build_balance_message(stake_lamports, use_lamports_unit, true)
            );
            println!("Credits Observed: {}", stake.credits_observed);
            println!(
                "Delegated Stake: {}",
                build_balance_message(stake.delegation.stake, use_lamports_unit, true)
            );
            if stake.delegation.voter_pubkey != Pubkey::default() {
                println!("Delegated Voter Pubkey: {}", stake.delegation.voter_pubkey);
            }
            println!(
                "Stake activates starting from epoch: {}",
                if stake.delegation.activation_epoch < std::u64::MAX {
                    stake.delegation.activation_epoch
                } else {
                    0
                }
            );
            if stake.delegation.deactivation_epoch < std::u64::MAX {
                println!(
                    "Stake deactivates starting from epoch: {}",
                    stake.delegation.deactivation_epoch
                );
            }
            show_authorized(&authorized);
            show_lockup(&lockup);
        }
        StakeState::RewardsPool => println!("Stake account is a rewards pool"),
        StakeState::Uninitialized => println!("Stake account is uninitialized"),
        StakeState::Initialized(Meta {
            authorized, lockup, ..
        }) => {
            println!(
                "Total Stake: {}",
                build_balance_message(stake_lamports, use_lamports_unit, true)
            );
            println!("Stake account is undelegated");
            show_authorized(&authorized);
            show_lockup(&lockup);
        }
    }
}

pub fn process_show_stake_account(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let stake_account = rpc_client.get_account(stake_account_pubkey)?;
    if stake_account.owner != solana_stake_program::id() {
        return Err(CliError::RpcRequestError(format!(
            "{:?} is not a stake account",
            stake_account_pubkey
        ))
        .into());
    }
    match stake_account.state() {
        Ok(stake_state) => {
            print_stake_state(stake_account.lamports, &stake_state, use_lamports_unit);
            Ok("".to_string())
        }
        Err(err) => Err(CliError::RpcRequestError(format!(
            "Account data could not be deserialized to stake state: {:?}",
            err
        ))
        .into()),
    }
}

pub fn process_show_stake_history(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    use_lamports_unit: bool,
) -> ProcessResult {
    let stake_history_account = rpc_client.get_account(&stake_history::id())?;
    let stake_history = StakeHistory::from_account(&stake_history_account).ok_or_else(|| {
        CliError::RpcRequestError("Failed to deserialize stake history".to_string())
    })?;

    println!();
    println!(
        "{}",
        style(format!(
            "  {:<5}  {:>20}  {:>20}  {:>20}",
            "Epoch", "Effective Stake", "Activating Stake", "Deactivating Stake",
        ))
        .bold()
    );

    for (epoch, entry) in stake_history.deref() {
        println!(
            "  {:>5}  {:>20}  {:>20}  {:>20} {}",
            epoch,
            build_balance_message(entry.effective, use_lamports_unit, false),
            build_balance_message(entry.activating, use_lamports_unit, false),
            build_balance_message(entry.deactivating, use_lamports_unit, false),
            if use_lamports_unit { "lamports" } else { "SOL" }
        );
    }
    Ok("".to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn process_delegate_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
    stake_authority: Option<&SigningAuthority>,
    force: bool,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<Pubkey>,
    nonce_authority: Option<&SigningAuthority>,
    fee_payer: Option<&SigningAuthority>,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (stake_account_pubkey, "stake_account_pubkey".to_string()),
    )?;
    let stake_authority = stake_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);

    if !sign_only {
        // Sanity check the vote account to ensure it is attached to a validator that has recently
        // voted at the tip of the ledger
        let vote_account_data = rpc_client
            .get_account_data(vote_account_pubkey)
            .map_err(|_| {
                CliError::RpcRequestError(format!(
                    "Vote account not found: {}",
                    vote_account_pubkey
                ))
            })?;

        let vote_state = VoteState::deserialize(&vote_account_data).map_err(|_| {
            CliError::RpcRequestError(
                "Account data could not be deserialized to vote state".to_string(),
            )
        })?;

        let sanity_check_result = match vote_state.root_slot {
            None => Err(CliError::BadParameter(
                "Unable to delegate. Vote account has no root slot".to_string(),
            )),
            Some(root_slot) => {
                let slot = rpc_client.get_slot()?;
                if root_slot + solana_sdk::clock::DEFAULT_SLOTS_PER_TURN < slot {
                    Err(CliError::BadParameter(
                        format!(
                        "Unable to delegate. Vote account root slot ({}) is too old, the current slot is {}", root_slot, slot
                        )
                    ))
                } else {
                    Ok(())
                }
            }
        };

        if sanity_check_result.is_err() {
            if !force {
                sanity_check_result?;
            } else {
                println!("--force supplied, ignoring: {:?}", sanity_check_result);
            }
        }
    }

    let (recent_blockhash, fee_calculator) =
        blockhash_query.get_blockhash_fee_calculator(rpc_client)?;

    let ixs = vec![stake_instruction::delegate_stake(
        stake_account_pubkey,
        &stake_authority.pubkey(),
        vote_account_pubkey,
    )];
    let (nonce_authority, nonce_authority_pubkey) = nonce_authority
        .map(|a| (a.keypair(), a.pubkey()))
        .unwrap_or((&config.keypair, config.keypair.pubkey()));
    let fee_payer = fee_payer.map(|f| f.keypair()).unwrap_or(&config.keypair);
    let mut tx = if let Some(nonce_account) = &nonce_account {
        Transaction::new_signed_with_nonce(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, nonce_authority, stake_authority],
            nonce_account,
            &nonce_authority.pubkey(),
            recent_blockhash,
        )
    } else {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&fee_payer.pubkey()),
            &[fee_payer, stake_authority],
            recent_blockhash,
        )
    };
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = rpc_client.get_account(nonce_account)?;
            check_nonce_account(&nonce_account, &nonce_authority_pubkey, &recent_blockhash)?;
        }
        check_account_for_fee(
            rpc_client,
            &tx.message.account_keys[0],
            &fee_calculator,
            &tx.message,
        )?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<StakeError>(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::{
        fee_calculator::FeeCalculator,
        hash::Hash,
        signature::{keypair_from_seed, read_keypair_file, write_keypair, KeypairUtil},
    };
    use tempfile::NamedTempFile;

    fn make_tmp_file() -> (String, NamedTempFile) {
        let tmp_file = NamedTempFile::new().unwrap();
        (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
    }

    fn parse_authorize_tests(
        test_commands: &App,
        stake_account_pubkey: Pubkey,
        authority_keypair_file: &str,
        stake_authorize: StakeAuthorize,
    ) {
        let stake_account_string = stake_account_pubkey.to_string();

        let (subcommand, authority_flag) = match stake_authorize {
            StakeAuthorize::Staker => ("stake-authorize-staker", "--stake-authority"),
            StakeAuthorize::Withdrawer => ("stake-authorize-withdrawer", "--withdraw-authority"),
        };

        // Test Staker Subcommand
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Staker Subcommand w/ authority
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            &authority_flag,
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: Some(read_keypair_file(&authority_keypair_file).unwrap().into()),
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ sign-only
        let blockhash = Hash::default();
        let blockhash_string = format!("{}", blockhash);
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: true,
                    signers: None,
                    blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ signer
        let keypair = Keypair::new();
        let sig = keypair.sign_message(&[0u8]);
        let signer = format!("{}={}", keypair.pubkey(), sig);
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: Some(vec![(keypair.pubkey(), sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ signers
        let keypair2 = Keypair::new();
        let sig2 = keypair.sign_message(&[0u8]);
        let signer2 = format!("{}={}", keypair2.pubkey(), sig2);
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer,
            "--signer",
            &signer2,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: Some(vec![(keypair.pubkey(), sig), (keypair2.pubkey(), sig2),]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ blockhash
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ nonce
        let (nonce_keypair_file, mut nonce_tmp_file) = make_tmp_file();
        let nonce_authority_keypair = Keypair::new();
        write_keypair(&nonce_authority_keypair, nonce_tmp_file.as_file_mut()).unwrap();
        let nonce_account_pubkey = nonce_authority_keypair.pubkey();
        let nonce_account_string = nonce_account_pubkey.to_string();
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--nonce",
            &nonce_account_string,
            "--nonce-authority",
            &nonce_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: Some(nonce_account_pubkey),
                    nonce_authority: Some(nonce_authority_keypair.into()),
                    fee_payer: None,
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ fee-payer
        let (fee_payer_keypair_file, mut fee_payer_tmp_file) = make_tmp_file();
        let fee_payer_keypair = Keypair::new();
        write_keypair(&fee_payer_keypair, fee_payer_tmp_file.as_file_mut()).unwrap();
        let fee_payer_pubkey = fee_payer_keypair.pubkey();
        let fee_payer_string = fee_payer_pubkey.to_string();
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--fee-payer",
            &fee_payer_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(read_keypair_file(&fee_payer_keypair_file).unwrap().into()),
                },
                require_keypair: true
            }
        );
        // Test Authorize Subcommand w/ absentee fee-payer
        let sig = fee_payer_keypair.sign_message(&[0u8]);
        let signer = format!("{}={}", fee_payer_string, sig);
        let test_authorize = test_commands.clone().get_matches_from(vec![
            "test",
            &subcommand,
            &stake_account_string,
            &stake_account_string,
            "--fee-payer",
            &fee_payer_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer,
        ]);
        assert_eq!(
            parse_command(&test_authorize).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize {
                    stake_account_pubkey,
                    new_authorized_pubkey: stake_account_pubkey,
                    stake_authorize,
                    authority: None,
                    sign_only: false,
                    signers: Some(vec![(fee_payer_pubkey, sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(fee_payer_pubkey.into()),
                },
                require_keypair: true
            }
        );
    }

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let (keypair_file, mut tmp_file) = make_tmp_file();
        let stake_account_keypair = Keypair::new();
        write_keypair(&stake_account_keypair, tmp_file.as_file_mut()).unwrap();
        let stake_account_pubkey = stake_account_keypair.pubkey();
        let (stake_authority_keypair_file, mut tmp_file) = make_tmp_file();
        let stake_authority_keypair = Keypair::new();
        write_keypair(&stake_authority_keypair, tmp_file.as_file_mut()).unwrap();

        parse_authorize_tests(
            &test_commands,
            stake_account_pubkey,
            &stake_authority_keypair_file,
            StakeAuthorize::Staker,
        );
        parse_authorize_tests(
            &test_commands,
            stake_account_pubkey,
            &stake_authority_keypair_file,
            StakeAuthorize::Withdrawer,
        );

        // Test CreateStakeAccount SubCommand
        let custodian = Pubkey::new_rand();
        let custodian_string = format!("{}", custodian);
        let authorized = Pubkey::new_rand();
        let authorized_string = format!("{}", authorized);
        let test_create_stake_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &keypair_file,
            "50",
            "--stake-authority",
            &authorized_string,
            "--withdraw-authority",
            &authorized_string,
            "--custodian",
            &custodian_string,
            "--lockup-epoch",
            "43",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_create_stake_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStakeAccount {
                    stake_account: stake_account_keypair.into(),
                    seed: None,
                    staker: Some(authorized),
                    withdrawer: Some(authorized),
                    lockup: Lockup {
                        epoch: 43,
                        unix_timestamp: 0,
                        custodian,
                    },
                    lamports: 50,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                    from: None,
                },
                require_keypair: true
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let stake_account_keypair = Keypair::new();
        write_keypair(&stake_account_keypair, tmp_file.as_file_mut()).unwrap();
        let stake_account_pubkey = stake_account_keypair.pubkey();
        let stake_account_string = stake_account_pubkey.to_string();

        let test_create_stake_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &keypair_file,
            "50",
            "lamports",
        ]);

        assert_eq!(
            parse_command(&test_create_stake_account2).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStakeAccount {
                    stake_account: read_keypair_file(&keypair_file).unwrap().into(),
                    seed: None,
                    staker: None,
                    withdrawer: None,
                    lockup: Lockup::default(),
                    lamports: 50,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                    from: None,
                },
                require_keypair: true
            }
        );

        // CreateStakeAccount offline and nonce
        let nonce_account = Pubkey::new(&[1u8; 32]);
        let nonce_account_string = nonce_account.to_string();
        let offline = keypair_from_seed(&[2u8; 32]).unwrap();
        let offline_pubkey = offline.pubkey();
        let offline_string = offline_pubkey.to_string();
        let offline_sig = offline.sign_message(&[3u8]);
        let offline_signer = format!("{}={}", offline_pubkey, offline_sig);
        let nonce_hash = Hash::new(&[4u8; 32]);
        let nonce_hash_string = nonce_hash.to_string();
        let test_create_stake_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &keypair_file,
            "50",
            "lamports",
            "--blockhash",
            &nonce_hash_string,
            "--nonce",
            &nonce_account_string,
            "--nonce-authority",
            &offline_string,
            "--fee-payer",
            &offline_string,
            "--from",
            &offline_string,
            "--signer",
            &offline_signer,
        ]);

        assert_eq!(
            parse_command(&test_create_stake_account2).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStakeAccount {
                    stake_account: read_keypair_file(&keypair_file).unwrap().into(),
                    seed: None,
                    staker: None,
                    withdrawer: None,
                    lockup: Lockup::default(),
                    lamports: 50,
                    sign_only: false,
                    signers: Some(vec![(offline_pubkey, offline_sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
                    nonce_account: Some(nonce_account),
                    nonce_authority: Some(offline_pubkey.into()),
                    fee_payer: Some(offline_pubkey.into()),
                    from: Some(offline_pubkey.into()),
                },
                require_keypair: false,
            }
        );

        // Test DelegateStake Subcommand
        let vote_account_pubkey = Pubkey::new_rand();
        let vote_account_string = vote_account_pubkey.to_string();
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test DelegateStake Subcommand w/ authority
        let vote_account_pubkey = Pubkey::new_rand();
        let vote_account_string = vote_account_pubkey.to_string();
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--stake-authority",
            &stake_authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: Some(
                        read_keypair_file(&stake_authority_keypair_file)
                            .unwrap()
                            .into()
                    ),
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test DelegateStake Subcommand w/ force
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            "--force",
            &stake_account_string,
            &vote_account_string,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: true,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test Delegate Subcommand w/ Blockhash
        let blockhash = Hash::default();
        let blockhash_string = format!("{}", blockhash);
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: true,
                    signers: None,
                    blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test Delegate Subcommand w/ signer
        let key1 = Pubkey::new_rand();
        let sig1 = Keypair::new().sign_message(&[0u8]);
        let signer1 = format!("{}={}", key1, sig1);
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer1,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: false
            }
        );

        // Test Delegate Subcommand w/ signers
        let key2 = Pubkey::new_rand();
        let sig2 = Keypair::new().sign_message(&[0u8]);
        let signer2 = format!("{}={}", key2, sig2);
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer1,
            "--signer",
            &signer2,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1), (key2, sig2)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: false
            }
        );

        // Test Delegate Subcommand w/ fee-payer
        let (fee_payer_keypair_file, mut fee_payer_tmp_file) = make_tmp_file();
        let fee_payer_keypair = Keypair::new();
        write_keypair(&fee_payer_keypair, fee_payer_tmp_file.as_file_mut()).unwrap();
        let fee_payer_pubkey = fee_payer_keypair.pubkey();
        let fee_payer_string = fee_payer_pubkey.to_string();
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--fee-payer",
            &fee_payer_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(read_keypair_file(&fee_payer_keypair_file).unwrap().into()),
                },
                require_keypair: true
            }
        );

        // Test Delegate Subcommand w/ absentee fee-payer
        let sig = fee_payer_keypair.sign_message(&[0u8]);
        let signer = format!("{}={}", fee_payer_string, sig);
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--fee-payer",
            &fee_payer_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    stake_authority: None,
                    force: false,
                    sign_only: false,
                    signers: Some(vec![(fee_payer_pubkey, sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(fee_payer_pubkey.into()),
                },
                require_keypair: false
            }
        );

        // Test WithdrawStake Subcommand
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &stake_account_string,
            &stake_account_string,
            "42",
            "lamports",
        ]);

        assert_eq!(
            parse_command(&test_withdraw_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawStake {
                    stake_account_pubkey,
                    destination_account_pubkey: stake_account_pubkey,
                    lamports: 42,
                    withdraw_authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test WithdrawStake Subcommand w/ authority
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &stake_account_string,
            &stake_account_string,
            "42",
            "lamports",
            "--withdraw-authority",
            &stake_authority_keypair_file,
        ]);

        assert_eq!(
            parse_command(&test_withdraw_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawStake {
                    stake_account_pubkey,
                    destination_account_pubkey: stake_account_pubkey,
                    lamports: 42,
                    withdraw_authority: Some(
                        read_keypair_file(&stake_authority_keypair_file)
                            .unwrap()
                            .into()
                    ),
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test WithdrawStake Subcommand w/ authority and offline nonce
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &stake_account_string,
            &stake_account_string,
            "42",
            "lamports",
            "--withdraw-authority",
            &stake_authority_keypair_file,
            "--blockhash",
            &nonce_hash_string,
            "--nonce",
            &nonce_account_string,
            "--nonce-authority",
            &offline_string,
            "--fee-payer",
            &offline_string,
            "--signer",
            &offline_signer,
        ]);

        assert_eq!(
            parse_command(&test_withdraw_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawStake {
                    stake_account_pubkey,
                    destination_account_pubkey: stake_account_pubkey,
                    lamports: 42,
                    withdraw_authority: Some(
                        read_keypair_file(&stake_authority_keypair_file)
                            .unwrap()
                            .into()
                    ),
                    sign_only: false,
                    signers: Some(vec![(offline_pubkey, offline_sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
                    nonce_account: Some(nonce_account),
                    nonce_authority: Some(offline_pubkey.into()),
                    fee_payer: Some(offline_pubkey.into()),
                },
                require_keypair: false,
            }
        );

        // Test DeactivateStake Subcommand
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test DeactivateStake Subcommand w/ authority
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--stake-authority",
            &stake_authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: Some(
                        read_keypair_file(&stake_authority_keypair_file)
                            .unwrap()
                            .into()
                    ),
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test Deactivate Subcommand w/ Blockhash
        let blockhash = Hash::default();
        let blockhash_string = format!("{}", blockhash);
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: true,
                    signers: None,
                    blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Test Deactivate Subcommand w/ signers
        let key1 = Pubkey::new_rand();
        let sig1 = Keypair::new().sign_message(&[0u8]);
        let signer1 = format!("{}={}", key1, sig1);
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer1,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: false
            }
        );

        // Test Deactivate Subcommand w/ signers
        let key2 = Pubkey::new_rand();
        let sig2 = Keypair::new().sign_message(&[0u8]);
        let signer2 = format!("{}={}", key2, sig2);
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer1,
            "--signer",
            &signer2,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1), (key2, sig2)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: None,
                },
                require_keypair: false
            }
        );

        // Test Deactivate Subcommand w/ fee-payer
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--fee-payer",
            &fee_payer_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::All,
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(read_keypair_file(&fee_payer_keypair_file).unwrap().into()),
                },
                require_keypair: true
            }
        );

        // Test Deactivate Subcommand w/ absentee fee-payer
        let sig = fee_payer_keypair.sign_message(&[0u8]);
        let signer = format!("{}={}", fee_payer_string, sig);
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--fee-payer",
            &fee_payer_string,
            "--blockhash",
            &blockhash_string,
            "--signer",
            &signer,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    stake_authority: None,
                    sign_only: false,
                    signers: Some(vec![(fee_payer_pubkey, sig)]),
                    blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
                    nonce_account: None,
                    nonce_authority: None,
                    fee_payer: Some(fee_payer_pubkey.into()),
                },
                require_keypair: false
            }
        );

        // Test SplitStake SubCommand
        let (keypair_file, mut tmp_file) = make_tmp_file();
        let stake_account_keypair = Keypair::new();
        write_keypair(&stake_account_keypair, tmp_file.as_file_mut()).unwrap();
        let (split_stake_account_keypair_file, mut tmp_file) = make_tmp_file();
        let split_stake_account_keypair = Keypair::new();
        write_keypair(&split_stake_account_keypair, tmp_file.as_file_mut()).unwrap();

        let test_split_stake_account = test_commands.clone().get_matches_from(vec![
            "test",
            "split-stake",
            &keypair_file,
            &split_stake_account_keypair_file,
            "50",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_split_stake_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::SplitStake {
                    stake_account_pubkey: stake_account_keypair.pubkey(),
                    stake_authority: None,
                    sign_only: false,
                    signers: None,
                    blockhash_query: BlockhashQuery::default(),
                    nonce_account: None,
                    nonce_authority: None,
                    split_stake_account: read_keypair_file(&split_stake_account_keypair_file)
                        .unwrap()
                        .into(),
                    seed: None,
                    lamports: 50,
                    fee_payer: None,
                },
                require_keypair: true
            }
        );

        // Split stake offline nonced submission
        let nonce_account = Pubkey::new(&[1u8; 32]);
        let nonce_account_string = nonce_account.to_string();
        let nonce_auth = keypair_from_seed(&[2u8; 32]).unwrap();
        let nonce_auth_pubkey = nonce_auth.pubkey();
        let nonce_auth_string = nonce_auth_pubkey.to_string();
        let nonce_sig = nonce_auth.sign_message(&[0u8]);
        let nonce_signer = format!("{}={}", nonce_auth_pubkey, nonce_sig);
        let stake_auth = keypair_from_seed(&[3u8; 32]).unwrap();
        let stake_auth_pubkey = stake_auth.pubkey();
        let stake_auth_string = stake_auth_pubkey.to_string();
        let stake_sig = stake_auth.sign_message(&[0u8]);
        let stake_signer = format!("{}={}", stake_auth_pubkey, stake_sig);
        let nonce_hash = Hash::new(&[4u8; 32]);
        let nonce_hash_string = nonce_hash.to_string();

        let test_split_stake_account = test_commands.clone().get_matches_from(vec![
            "test",
            "split-stake",
            &keypair_file,
            &split_stake_account_keypair_file,
            "50",
            "lamports",
            "--stake-authority",
            &stake_auth_string,
            "--blockhash",
            &nonce_hash_string,
            "--nonce",
            &nonce_account_string,
            "--nonce-authority",
            &nonce_auth_string,
            "--fee-payer",
            &nonce_auth_string, // Arbitrary choice of fee-payer
            "--signer",
            &nonce_signer,
            "--signer",
            &stake_signer,
        ]);
        assert_eq!(
            parse_command(&test_split_stake_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::SplitStake {
                    stake_account_pubkey: stake_account_keypair.pubkey(),
                    stake_authority: Some(stake_auth_pubkey.into()),
                    sign_only: false,
                    signers: Some(vec![
                        (nonce_auth_pubkey, nonce_sig),
                        (stake_auth_pubkey, stake_sig)
                    ]),
                    blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
                    nonce_account: Some(nonce_account.into()),
                    nonce_authority: Some(nonce_auth_pubkey.into()),
                    split_stake_account: read_keypair_file(&split_stake_account_keypair_file)
                        .unwrap()
                        .into(),
                    seed: None,
                    lamports: 50,
                    fee_payer: Some(nonce_auth_pubkey.into()),
                },
                require_keypair: false,
            }
        );
    }
}
