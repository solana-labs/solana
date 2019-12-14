use crate::cli::{
    build_balance_message, check_account_for_fee, check_unique_pubkeys,
    get_blockhash_fee_calculator, log_instruction_custom_error, replace_signatures,
    required_lamports_from, return_signers, CliCommand, CliCommandInfo, CliConfig, CliError,
    ProcessResult,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use console::style;
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::{
    account_utils::State,
    hash::Hash,
    pubkey::Pubkey,
    signature::KeypairUtil,
    system_instruction::SystemError,
    sysvar::{
        stake_history::{self, StakeHistory},
        Sysvar,
    },
    transaction::Transaction,
};
use solana_stake_program::stake_state::Meta;
use solana_stake_program::{
    stake_instruction::{self, StakeError},
    stake_state::{Authorized, Lockup, StakeAuthorize, StakeState},
};
use solana_vote_program::vote_state::VoteState;
use std::ops::Deref;

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
                        .validator(is_keypair_or_ask_keyword)
                        .help("Keypair of the stake account to fund")
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
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Identity of the custodian (can withdraw before lockup expires)")
                )
                .arg(
                    Arg::with_name("lockup")
                        .long("lockup")
                        .value_name("SLOT")
                        .takes_value(true)
                        .help("The slot height at which this account will be available for withdrawal")
                )
                .arg(
                    Arg::with_name("authorized_staker")
                        .long("authorized-staker")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Public key of authorized staker (defaults to cli config pubkey)")
                )
                .arg(
                    Arg::with_name("authorized_withdrawer")
                        .long("authorized-withdrawer")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Public key of the authorized withdrawer (defaults to cli config pubkey)")
                )
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
                .arg(
                    Arg::with_name("sign_only")
                        .long("sign-only")
                        .takes_value(false)
                        .help("Sign the transaction offline"),
                )
                .arg(
                    Arg::with_name("signer")
                        .long("signer")
                        .value_name("PUBKEY=BASE58_SIG")
                        .takes_value(true)
                        .validator(is_pubkey_sig)
                        .multiple(true)
                        .help("Provide a public-key/signature pair for the transaction"),
                )
                .arg(
                    Arg::with_name("blockhash")
                        .long("blockhash")
                        .value_name("BLOCKHASH")
                        .takes_value(true)
                        .validator(is_hash)
                        .help("Use the supplied blockhash"),
                ),
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
                .arg(
                    Arg::with_name("sign_only")
                        .long("sign-only")
                        .takes_value(false)
                        .help("Sign the transaction offline"),
                )
                .arg(
                    Arg::with_name("signer")
                        .long("signer")
                        .value_name("PUBKEY=BASE58_SIG")
                        .takes_value(true)
                        .validator(is_pubkey_sig)
                        .multiple(true)
                        .help("Provide a public-key/signature pair for the transaction"),
                )
                .arg(
                    Arg::with_name("blockhash")
                        .long("blockhash")
                        .value_name("BLOCKHASH")
                        .takes_value(true)
                        .validator(is_hash)
                        .help("Use the supplied blockhash"),
                ),
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
           )
        .subcommand(
            SubCommand::with_name("redeem-vote-credits")
                .about("Redeem credits in the stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the stake account in which to redeem credits")
                )
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(2)
                        .value_name("VOTE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The vote account to which the stake is currently delegated.")
                )
        )
        .subcommand(
            SubCommand::with_name("show-stake-account")
                .about("Show the contents of a stake account")
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
            SubCommand::with_name("show-stake-history")
                .about("Show the stake history")
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
    let stake_account = keypair_of(matches, "stake_account").unwrap();
    let epoch = value_of(&matches, "lockup").unwrap_or(0);
    let custodian = pubkey_of(matches, "custodian").unwrap_or_default();
    let staker = pubkey_of(matches, "authorized_staker");
    let withdrawer = pubkey_of(matches, "authorized_withdrawer");
    let lamports = required_lamports_from(matches, "amount", "unit")?;

    Ok(CliCommandInfo {
        command: CliCommand::CreateStakeAccount {
            stake_account: stake_account.into(),
            staker,
            withdrawer,
            lockup: Lockup { custodian, epoch },
            lamports,
        },
        require_keypair: true,
    })
}

pub fn parse_stake_delegate_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let force = matches.is_present("force");
    let sign_only = matches.is_present("sign_only");
    let signers = pubkeys_sigs_of(&matches, "signer");
    let blockhash = value_of(matches, "blockhash");
    let require_keypair = signers.is_none();

    Ok(CliCommandInfo {
        command: CliCommand::DelegateStake {
            stake_account_pubkey,
            vote_account_pubkey,
            force,
            sign_only,
            signers,
            blockhash,
        },
        require_keypair,
    })
}

pub fn parse_stake_authorize(
    matches: &ArgMatches<'_>,
    stake_authorize: StakeAuthorize,
) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let authorized_pubkey = pubkey_of(matches, "authorized_pubkey").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::StakeAuthorize(
            stake_account_pubkey,
            authorized_pubkey,
            stake_authorize,
        ),
        require_keypair: true,
    })
}

pub fn parse_redeem_vote_credits(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::RedeemVoteCredits(stake_account_pubkey, vote_account_pubkey),
        require_keypair: true,
    })
}

pub fn parse_stake_deactivate_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let sign_only = matches.is_present("sign_only");
    let signers = pubkeys_sigs_of(&matches, "signer");
    let blockhash = value_of(matches, "blockhash");
    let require_keypair = signers.is_none();

    Ok(CliCommandInfo {
        command: CliCommand::DeactivateStake {
            stake_account_pubkey,
            sign_only,
            signers,
            blockhash,
        },
        require_keypair,
    })
}

pub fn parse_stake_withdraw_stake(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let destination_account_pubkey = pubkey_of(matches, "destination_account_pubkey").unwrap();
    let lamports = required_lamports_from(matches, "amount", "unit")?;

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawStake(
            stake_account_pubkey,
            destination_account_pubkey,
            lamports,
        ),
        require_keypair: true,
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

pub fn process_create_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account: &Keypair,
    staker: &Option<Pubkey>,
    withdrawer: &Option<Pubkey>,
    lockup: &Lockup,
    lamports: u64,
) -> ProcessResult {
    let stake_account_pubkey = stake_account.pubkey();
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&stake_account_pubkey, "stake_account_pubkey".to_string()),
    )?;

    if rpc_client.get_account(&stake_account_pubkey).is_ok() {
        return Err(CliError::BadParameter(format!(
            "Unable to create stake account. Stake account already exists: {}",
            stake_account_pubkey
        ))
        .into());
    }

    let minimum_balance =
        rpc_client.get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())?;

    if lamports < minimum_balance {
        return Err(CliError::BadParameter(format!(
            "need atleast {} lamports for stake account to be rent exempt, provided lamports: {}",
            minimum_balance, lamports
        ))
        .into());
    }

    let authorized = Authorized {
        staker: staker.unwrap_or(config.keypair.pubkey()),
        withdrawer: withdrawer.unwrap_or(config.keypair.pubkey()),
    };
    println!("{:?}", authorized);

    let ixs = stake_instruction::create_account(
        &config.keypair.pubkey(),
        &stake_account_pubkey,
        &authorized,
        lockup,
        lamports,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, stake_account],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, stake_account]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_stake_authorize(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    stake_authorize: StakeAuthorize,
) -> ProcessResult {
    check_unique_pubkeys(
        (stake_account_pubkey, "stake_account_pubkey".to_string()),
        (authorized_pubkey, "new_authorized_pubkey".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::authorize(
        stake_account_pubkey,     // stake account to update
        &config.keypair.pubkey(), // currently authorized
        authorized_pubkey,        // new stake signer
        stake_authorize,          // stake or withdraw
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
}

pub fn process_deactivate_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash: Option<Hash>,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) =
        get_blockhash_fee_calculator(rpc_client, sign_only, blockhash)?;
    let ixs = vec![stake_instruction::deactivate_stake(
        stake_account_pubkey,
        &config.keypair.pubkey(),
    )];
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
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

pub fn process_withdraw_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ixs = vec![stake_instruction::withdraw(
        stake_account_pubkey,
        &config.keypair.pubkey(),
        destination_account_pubkey,
        lamports,
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
}

pub fn process_redeem_vote_credits(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::redeem_vote_credits(
        stake_account_pubkey,
        vote_account_pubkey,
    )];
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
}

pub fn process_show_stake_account(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let stake_account = rpc_client.get_account(stake_account_pubkey)?;
    if stake_account.owner != solana_stake_program::id() {
        return Err(CliError::RpcRequestError(
            format!("{:?} is not a stake account", stake_account_pubkey).to_string(),
        )
        .into());
    }
    fn show_authorized(authorized: &Authorized) {
        println!("authorized staker: {}", authorized.staker);
        println!("authorized withdrawer: {}", authorized.staker);
    }
    fn show_lockup(lockup: &Lockup) {
        println!("lockup epoch: {}", lockup.epoch);
        println!("lockup custodian: {}", lockup.custodian);
    }
    match stake_account.state() {
        Ok(StakeState::Stake(
            Meta {
                authorized, lockup, ..
            },
            stake,
        )) => {
            println!(
                "total stake: {}",
                build_balance_message(stake_account.lamports, use_lamports_unit, true)
            );
            println!("credits observed: {}", stake.credits_observed);
            println!(
                "delegated stake: {}",
                build_balance_message(stake.delegation.stake, use_lamports_unit, true)
            );
            if stake.delegation.voter_pubkey != Pubkey::default() {
                println!("delegated voter pubkey: {}", stake.delegation.voter_pubkey);
            }
            println!(
                "stake activates starting from epoch: {}",
                if stake.delegation.activation_epoch < std::u64::MAX {
                    stake.delegation.activation_epoch
                } else {
                    0
                }
            );
            if stake.delegation.deactivation_epoch < std::u64::MAX {
                println!(
                    "stake deactivates starting from epoch: {}",
                    stake.delegation.deactivation_epoch
                );
            }
            show_authorized(&authorized);
            show_lockup(&lockup);
            Ok("".to_string())
        }
        Ok(StakeState::RewardsPool) => Ok("Stake account is a rewards pool".to_string()),
        Ok(StakeState::Uninitialized) => Ok("Stake account is uninitialized".to_string()),
        Ok(StakeState::Initialized(Meta {
            authorized, lockup, ..
        })) => {
            println!("Stake account is undelegated");
            show_authorized(&authorized);
            show_lockup(&lockup);
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

pub fn process_delegate_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
    force: bool,
    sign_only: bool,
    signers: &Option<Vec<(Pubkey, Signature)>>,
    blockhash: Option<Hash>,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (stake_account_pubkey, "stake_account_pubkey".to_string()),
    )?;

    // Sanity check the vote account to ensure it is attached to a validator that has recently
    // voted at the tip of the ledger
    let vote_account_data = rpc_client
        .get_account_data(vote_account_pubkey)
        .map_err(|_| {
            CliError::RpcRequestError(format!("Vote account not found: {}", vote_account_pubkey))
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

    let (recent_blockhash, fee_calculator) =
        get_blockhash_fee_calculator(rpc_client, sign_only, blockhash)?;

    let ixs = vec![stake_instruction::delegate_stake(
        stake_account_pubkey,
        &config.keypair.pubkey(),
        vote_account_pubkey,
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    if let Some(signers) = signers {
        replace_signatures(&mut tx, &signers)?;
    }
    if sign_only {
        return_signers(&tx)
    } else {
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
    use solana_sdk::signature::write_keypair;
    use tempfile::NamedTempFile;

    fn make_tmp_file() -> (String, NamedTempFile) {
        let tmp_file = NamedTempFile::new().unwrap();
        (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
    }

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let (keypair_file, mut tmp_file) = make_tmp_file();
        let stake_account_keypair = Keypair::new();
        write_keypair(&stake_account_keypair, tmp_file.as_file_mut()).unwrap();
        let stake_account_pubkey = stake_account_keypair.pubkey();
        let stake_account_string = stake_account_pubkey.to_string();

        let test_authorize_staker = test_commands.clone().get_matches_from(vec![
            "test",
            "stake-authorize-staker",
            &stake_account_string,
            &stake_account_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_staker).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize(
                    stake_account_pubkey,
                    stake_account_pubkey,
                    StakeAuthorize::Staker
                ),
                require_keypair: true
            }
        );
        let test_authorize_withdrawer = test_commands.clone().get_matches_from(vec![
            "test",
            "stake-authorize-withdrawer",
            &stake_account_string,
            &stake_account_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_withdrawer).unwrap(),
            CliCommandInfo {
                command: CliCommand::StakeAuthorize(
                    stake_account_pubkey,
                    stake_account_pubkey,
                    StakeAuthorize::Withdrawer
                ),
                require_keypair: true
            }
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
            "--authorized-staker",
            &authorized_string,
            "--authorized-withdrawer",
            &authorized_string,
            "--custodian",
            &custodian_string,
            "--lockup",
            "43",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_create_stake_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStakeAccount {
                    stake_account: stake_account_keypair.into(),
                    staker: Some(authorized),
                    withdrawer: Some(authorized),
                    lockup: Lockup {
                        epoch: 43,
                        custodian,
                    },
                    lamports: 50
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
                    stake_account: stake_account_keypair.into(),
                    staker: None,
                    withdrawer: None,
                    lockup: Lockup::default(),
                    lamports: 50
                },
                require_keypair: true
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
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash: None
                },
                require_keypair: true
            }
        );

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
                    force: true,
                    sign_only: false,
                    signers: None,
                    blockhash: None
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
                    force: false,
                    sign_only: false,
                    signers: None,
                    blockhash: Some(blockhash)
                },
                require_keypair: true
            }
        );

        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_account_string,
            &vote_account_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    force: false,
                    sign_only: true,
                    signers: None,
                    blockhash: None
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
            "--signer",
            &signer1,
        ]);
        assert_eq!(
            parse_command(&test_delegate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DelegateStake {
                    stake_account_pubkey,
                    vote_account_pubkey,
                    force: false,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1)]),
                    blockhash: None
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
                    force: false,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1), (key2, sig2)]),
                    blockhash: None
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
                command: CliCommand::WithdrawStake(stake_account_pubkey, stake_account_pubkey, 42),
                require_keypair: true
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
                    sign_only: false,
                    signers: None,
                    blockhash: None
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
                    sign_only: false,
                    signers: None,
                    blockhash: Some(blockhash)
                },
                require_keypair: true
            }
        );

        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_account_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    sign_only: true,
                    signers: None,
                    blockhash: None
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
            "--signer",
            &signer1,
        ]);
        assert_eq!(
            parse_command(&test_deactivate_stake).unwrap(),
            CliCommandInfo {
                command: CliCommand::DeactivateStake {
                    stake_account_pubkey,
                    sign_only: false,
                    signers: Some(vec![(key1, sig1)]),
                    blockhash: None
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
                    sign_only: false,
                    signers: Some(vec![(key1, sig1), (key2, sig2)]),
                    blockhash: None
                },
                require_keypair: false
            }
        );
    }
}
