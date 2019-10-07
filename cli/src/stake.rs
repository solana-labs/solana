use crate::{
    cli::{
        build_balance_message, check_account_for_fee, check_unique_pubkeys,
        log_instruction_custom_error, CliCommand, CliConfig, CliError, ProcessResult,
    },
    input_parsers::*,
    input_validators::*,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    account_utils::State, pubkey::Pubkey, signature::KeypairUtil, system_instruction::SystemError,
    transaction::Transaction,
};
use solana_stake_api::{
    stake_instruction::{self, StakeError},
    stake_state::{Authorized, Lockup, StakeAuthorize, StakeState},
};
use solana_vote_api::vote_state::VoteState;

pub trait StakeSubCommands {
    fn stake_subcommands(self) -> Self;
}

impl StakeSubCommands for App<'_, '_> {
    fn stake_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("create-stake-account")
                .about("Create a stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the stake account to fund (pubkey or keypair)")
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
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
                        .help("The account to which the lamports should be transfered")
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
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
    }
}

pub fn parse_stake_create_account(
    pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let slot = value_of(&matches, "lockup").unwrap_or(0);
    let custodian = pubkey_of(matches, "custodian").unwrap_or_default();
    let staker = pubkey_of(matches, "authorized_staker").unwrap_or(*pubkey); // defaults to config
    let withdrawer = pubkey_of(matches, "authorized_withdrawer").unwrap_or(*pubkey); // defaults to config
    let lamports = amount_of(matches, "amount", "unit").expect("Invalid amount");

    Ok(CliCommand::CreateStakeAccount(
        stake_account_pubkey,
        Authorized { staker, withdrawer },
        Lockup { custodian, slot },
        lamports,
    ))
}

pub fn parse_stake_delegate_stake(matches: &ArgMatches<'_>) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let force = matches.is_present("force");

    Ok(CliCommand::DelegateStake(
        stake_account_pubkey,
        vote_account_pubkey,
        force,
    ))
}

pub fn parse_stake_authorize(
    matches: &ArgMatches<'_>,
    stake_authorize: StakeAuthorize,
) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let authorized_pubkey = pubkey_of(matches, "authorized_pubkey").unwrap();

    Ok(CliCommand::StakeAuthorize(
        stake_account_pubkey,
        authorized_pubkey,
        stake_authorize,
    ))
}

pub fn parse_redeem_vote_credits(matches: &ArgMatches<'_>) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    Ok(CliCommand::RedeemVoteCredits(
        stake_account_pubkey,
        vote_account_pubkey,
    ))
}

pub fn parse_stake_deactivate_stake(matches: &ArgMatches<'_>) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    Ok(CliCommand::DeactivateStake(stake_account_pubkey))
}

pub fn parse_stake_withdraw_stake(matches: &ArgMatches<'_>) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let destination_account_pubkey = pubkey_of(matches, "destination_account_pubkey").unwrap();
    let lamports = amount_of(matches, "amount", "unit").expect("Invalid amount");

    Ok(CliCommand::WithdrawStake(
        stake_account_pubkey,
        destination_account_pubkey,
        lamports,
    ))
}

pub fn parse_show_stake_account(matches: &ArgMatches<'_>) -> Result<CliCommand, CliError> {
    let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
    let use_lamports_unit = matches.is_present("lamports");
    Ok(CliCommand::ShowStakeAccount {
        pubkey: stake_account_pubkey,
        use_lamports_unit,
    })
}

pub fn process_create_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    authorized: &Authorized,
    lockup: &Lockup,
    lamports: u64,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (stake_account_pubkey, "stake_account_pubkey".to_string()),
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

    let ixs = stake_instruction::create_stake_account_with_lockup(
        &config.keypair.pubkey(),
        stake_account_pubkey,
        authorized,
        lockup,
        lamports,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
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

    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
}

pub fn process_deactivate_stake_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::deactivate_stake(
        stake_account_pubkey,
        &config.keypair.pubkey(),
    )];
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
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

    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
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
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
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
    if stake_account.owner != solana_stake_api::id() {
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
        println!("lockup slot: {}", lockup.slot);
        println!("lockup custodian: {}", lockup.custodian);
    }
    match stake_account.state() {
        Ok(StakeState::Stake(authorized, lockup, stake)) => {
            println!(
                "total stake: {}",
                build_balance_message(stake_account.lamports, use_lamports_unit)
            );
            println!("credits observed: {}", stake.credits_observed);
            println!(
                "delegated stake: {}",
                build_balance_message(stake.stake, use_lamports_unit)
            );
            if stake.voter_pubkey != Pubkey::default() {
                println!("delegated voter pubkey: {}", stake.voter_pubkey);
            }
            println!(
                "stake activates starting from epoch: {}",
                stake.activation_epoch
            );
            if stake.deactivation_epoch < std::u64::MAX {
                println!(
                    "stake deactivates starting from epoch: {}",
                    stake.deactivation_epoch
                );
            }
            show_authorized(&authorized);
            show_lockup(&lockup);
            Ok("".to_string())
        }
        Ok(StakeState::RewardsPool) => Ok("Stake account is a rewards pool".to_string()),
        Ok(StakeState::Uninitialized) => Ok("Stake account is uninitialized".to_string()),
        Ok(StakeState::Initialized(authorized, lockup)) => {
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

pub fn process_delegate_stake(
    rpc_client: &RpcClient,
    config: &CliConfig,
    stake_account_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
    force: bool,
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

    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ixs = vec![stake_instruction::delegate_stake(
        stake_account_pubkey,
        &config.keypair.pubkey(),
        vote_account_pubkey,
    )];

    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<StakeError>(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let pubkey = Pubkey::new_rand();
        let pubkey_string = format!("{}", pubkey);

        let test_authorize_staker = test_commands.clone().get_matches_from(vec![
            "test",
            "stake-authorize-staker",
            &pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_authorize_staker).unwrap(),
            CliCommand::StakeAuthorize(pubkey, pubkey, StakeAuthorize::Staker)
        );
        let test_authorize_withdrawer = test_commands.clone().get_matches_from(vec![
            "test",
            "stake-authorize-withdrawer",
            &pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_authorize_withdrawer).unwrap(),
            CliCommand::StakeAuthorize(pubkey, pubkey, StakeAuthorize::Withdrawer)
        );

        // Test CreateStakeAccount SubCommand
        let custodian = Pubkey::new_rand();
        let custodian_string = format!("{}", custodian);
        let authorized = Pubkey::new_rand();
        let authorized_string = format!("{}", authorized);
        let test_create_stake_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &pubkey_string,
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
            parse_command(&pubkey, &test_create_stake_account).unwrap(),
            CliCommand::CreateStakeAccount(
                pubkey,
                Authorized {
                    staker: authorized,
                    withdrawer: authorized,
                },
                Lockup {
                    slot: 43,
                    custodian,
                },
                50
            )
        );
        let test_create_stake_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &pubkey_string,
            "50",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_stake_account2).unwrap(),
            CliCommand::CreateStakeAccount(
                pubkey,
                Authorized {
                    staker: pubkey,
                    withdrawer: pubkey,
                },
                Lockup {
                    slot: 0,
                    custodian: Pubkey::default(),
                },
                50
            )
        );

        // Test DelegateStake Subcommand
        let stake_pubkey = Pubkey::new_rand();
        let stake_pubkey_string = stake_pubkey.to_string();
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &stake_pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_delegate_stake).unwrap(),
            CliCommand::DelegateStake(stake_pubkey, pubkey, false,)
        );

        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            "--force",
            &stake_pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_delegate_stake).unwrap(),
            CliCommand::DelegateStake(stake_pubkey, pubkey, true)
        );

        // Test WithdrawStake Subcommand
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &stake_pubkey_string,
            &pubkey_string,
            "42",
            "lamports",
        ]);

        assert_eq!(
            parse_command(&pubkey, &test_withdraw_stake).unwrap(),
            CliCommand::WithdrawStake(stake_pubkey, pubkey, 42)
        );

        // Test DeactivateStake Subcommand
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &stake_pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_deactivate_stake).unwrap(),
            CliCommand::DeactivateStake(stake_pubkey)
        );
    }
    // TODO: Add process tests
}
