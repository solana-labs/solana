use crate::{
    cli::{
        check_account_for_fee, check_unique_pubkeys, generate_unique_signers,
        log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError,
        ProcessResult, SignerIndex,
    },
    cli_output::{CliEpochVotingHistory, CliLockout, CliVoteAccount},
};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{
    commitment::{commitment_arg, COMMITMENT_ARG},
    input_parsers::*,
    input_validators::*,
};
use solana_client::rpc_client::RpcClient;
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, message::Message, pubkey::Pubkey,
    system_instruction::SystemError, transaction::Transaction,
};
use solana_vote_program::{
    vote_instruction::{self, withdraw, VoteError},
    vote_state::{VoteAuthorize, VoteInit, VoteState},
};
use std::sync::Arc;

pub trait VoteSubCommands {
    fn vote_subcommands(self) -> Self;
}

impl VoteSubCommands for App<'_, '_> {
    fn vote_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("create-vote-account")
                .about("Create a vote account")
                .arg(
                    Arg::with_name("vote_account")
                        .index(1)
                        .value_name("ACCOUNT_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Vote account keypair to create"),
                )
                .arg(
                    Arg::with_name("identity_account")
                        .index(2)
                        .value_name("IDENTITY_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Keypair of validator that will vote with this account"),
                )
                .arg(
                    Arg::with_name("commission")
                        .long("commission")
                        .value_name("PERCENTAGE")
                        .takes_value(true)
                        .default_value("100")
                        .help("The commission taken on reward redemption (0-100)"),
                )
                .arg(
                    pubkey!(Arg::with_name("authorized_voter")
                        .long("authorized-voter")
                        .value_name("VOTER_PUBKEY"),
                        "Public key of the authorized voter [default: validator identity pubkey]. "),
                )
                .arg(
                    pubkey!(Arg::with_name("authorized_withdrawer")
                        .long("authorized-withdrawer")
                        .value_name("WITHDRAWER_PUBKEY"),
                        "Public key of the authorized withdrawer [default: validator identity pubkey]. "),
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the VOTE ACCOUNT pubkey")
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-authorize-voter")
                .about("Authorize a new vote signing keypair for the given vote account")
                .arg(
                    pubkey!(Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Vote account in which to set the authorized voter. "),
                )
                .arg(
                    Arg::with_name("authorized")
                        .index(2)
                        .value_name("AUTHORIZED_KEYPAIR")
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Current authorized vote signer."),
                )
                .arg(
                    pubkey!(Arg::with_name("new_authorized_pubkey")
                        .index(3)
                        .value_name("NEW_AUTHORIZED_PUBKEY")
                        .required(true),
                        "New authorized vote signer. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-authorize-withdrawer")
                .about("Authorize a new withdraw signing keypair for the given vote account")
                .arg(
                    pubkey!(Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Vote account in which to set the authorized withdrawer. "),
                )
                .arg(
                    Arg::with_name("authorized")
                        .index(2)
                        .value_name("AUTHORIZED_KEYPAIR")
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Current authorized withdrawer."),
                )
                .arg(
                    pubkey!(Arg::with_name("new_authorized_pubkey")
                        .index(3)
                        .value_name("AUTHORIZED_PUBKEY")
                        .required(true),
                        "New authorized withdrawer. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-update-validator")
                .about("Update the vote account's validator identity")
                .arg(
                    pubkey!(Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Vote account to update. "),
                )
                .arg(
                    Arg::with_name("new_identity_account")
                        .index(2)
                        .value_name("IDENTITY_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Keypair of new validator that will vote with this account"),
                )
                .arg(
                    Arg::with_name("authorized_withdrawer")
                        .index(3)
                        .value_name("AUTHORIZED_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Authorized withdrawer keypair"),
                )
        )
        .subcommand(
            SubCommand::with_name("vote-account")
                .about("Show the contents of a vote account")
                .alias("show-vote-account")
                .arg(
                    pubkey!(Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Vote account pubkey. "),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                )
                .arg(commitment_arg()),
        )
        .subcommand(
            SubCommand::with_name("withdraw-from-vote-account")
                .about("Withdraw lamports from a vote account into a specified account")
                .arg(
                    pubkey!(Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Vote account from which to withdraw. "),
                )
                .arg(
                    pubkey!(Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("RECIPIENT_ADDRESS")
                        .required(true),
                        "The recipient of withdrawn SOL. "),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_amount)
                        .help("The amount to withdraw, in SOL"),
                )
                .arg(
                    Arg::with_name("authorized_withdrawer")
                        .long("authorized-withdrawer")
                        .value_name("AUTHORIZED_KEYPAIR")
                        .takes_value(true)
                        .validator(is_valid_signer)
                        .help("Authorized withdrawer [default: cli config keypair]"),
                )
        )
    }
}

pub fn parse_create_vote_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let (vote_account, _) = signer_of(matches, "vote_account", wallet_manager)?;
    let seed = matches.value_of("seed").map(|s| s.to_string());
    let (identity_account, identity_pubkey) =
        signer_of(matches, "identity_account", wallet_manager)?;
    let commission = value_t_or_exit!(matches, "commission", u8);
    let authorized_voter = pubkey_of_signer(matches, "authorized_voter", wallet_manager)?;
    let authorized_withdrawer = pubkey_of_signer(matches, "authorized_withdrawer", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, vote_account, identity_account],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::CreateVoteAccount {
            seed,
            identity_account: signer_info.index_of(identity_pubkey).unwrap(),
            authorized_voter,
            authorized_withdrawer,
            commission,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_vote_authorize(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    vote_authorize: VoteAuthorize,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey =
        pubkey_of_signer(matches, "vote_account_pubkey", wallet_manager)?.unwrap();
    let new_authorized_pubkey =
        pubkey_of_signer(matches, "new_authorized_pubkey", wallet_manager)?.unwrap();
    let (authorized, _) = signer_of(matches, "authorized", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, authorized],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::VoteAuthorize {
            vote_account_pubkey,
            new_authorized_pubkey,
            vote_authorize,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_vote_update_validator(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey =
        pubkey_of_signer(matches, "vote_account_pubkey", wallet_manager)?.unwrap();
    let (new_identity_account, new_identity_pubkey) =
        signer_of(matches, "new_identity_account", wallet_manager)?;
    let (authorized_withdrawer, _) = signer_of(matches, "authorized_withdrawer", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, authorized_withdrawer, new_identity_account],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::VoteUpdateValidator {
            vote_account_pubkey,
            new_identity_account: signer_info.index_of(new_identity_pubkey).unwrap(),
        },
        signers: signer_info.signers,
    })
}

pub fn parse_vote_get_account_command(
    matches: &ArgMatches<'_>,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey =
        pubkey_of_signer(matches, "vote_account_pubkey", wallet_manager)?.unwrap();
    let use_lamports_unit = matches.is_present("lamports");
    let commitment_config = commitment_of(matches, COMMITMENT_ARG.long).unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::ShowVoteAccount {
            pubkey: vote_account_pubkey,
            use_lamports_unit,
            commitment_config,
        },
        signers: vec![],
    })
}

pub fn parse_withdraw_from_vote_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey =
        pubkey_of_signer(matches, "vote_account_pubkey", wallet_manager)?.unwrap();
    let destination_account_pubkey =
        pubkey_of_signer(matches, "destination_account_pubkey", wallet_manager)?.unwrap();
    let lamports = lamports_of_sol(matches, "amount").unwrap();
    let (withdraw_authority, withdraw_authority_pubkey) =
        signer_of(matches, "authorized_withdrawer", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, withdraw_authority],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawFromVoteAccount {
            vote_account_pubkey,
            destination_account_pubkey,
            withdraw_authority: signer_info.index_of(withdraw_authority_pubkey).unwrap(),
            lamports,
        },
        signers: signer_info.signers,
    })
}

pub fn process_create_vote_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    seed: &Option<String>,
    identity_account: SignerIndex,
    authorized_voter: &Option<Pubkey>,
    authorized_withdrawer: &Option<Pubkey>,
    commission: u8,
) -> ProcessResult {
    let vote_account = config.signers[1];
    let vote_account_pubkey = vote_account.pubkey();
    let vote_account_address = if let Some(seed) = seed {
        Pubkey::create_with_seed(&vote_account_pubkey, &seed, &solana_vote_program::id())?
    } else {
        vote_account_pubkey
    };
    check_unique_pubkeys(
        (&config.signers[0].pubkey(), "cli keypair".to_string()),
        (&vote_account_address, "vote_account".to_string()),
    )?;

    let identity_account = config.signers[identity_account];
    let identity_pubkey = identity_account.pubkey();
    check_unique_pubkeys(
        (&vote_account_address, "vote_account".to_string()),
        (&identity_pubkey, "identity_pubkey".to_string()),
    )?;

    if let Ok(vote_account) = rpc_client.get_account(&vote_account_address) {
        let err_msg = if vote_account.owner == solana_vote_program::id() {
            format!("Vote account {} already exists", vote_account_address)
        } else {
            format!(
                "Account {} already exists and is not a vote account",
                vote_account_address
            )
        };
        return Err(CliError::BadParameter(err_msg).into());
    }

    let required_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(VoteState::size_of())?
        .max(1);

    let vote_init = VoteInit {
        node_pubkey: identity_pubkey,
        authorized_voter: authorized_voter.unwrap_or(identity_pubkey),
        authorized_withdrawer: authorized_withdrawer.unwrap_or(identity_pubkey),
        commission,
    };

    let ixs = if let Some(seed) = seed {
        vote_instruction::create_account_with_seed(
            &config.signers[0].pubkey(), // from
            &vote_account_address,       // to
            &vote_account_pubkey,        // base
            seed,                        // seed
            &vote_init,
            required_balance,
        )
    } else {
        vote_instruction::create_account(
            &config.signers[0].pubkey(),
            &vote_account_pubkey,
            &vote_init,
            required_balance,
        )
    };
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let message = Message::new(&ixs);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
    log_instruction_custom_error::<SystemError>(result, &config)
}

pub fn process_vote_authorize(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> ProcessResult {
    // If the `authorized_account` is also the fee payer, `config.signers` will only have one
    // keypair in it
    let authorized = if config.signers.len() == 2 {
        config.signers[1]
    } else {
        config.signers[0]
    };

    check_unique_pubkeys(
        (&authorized.pubkey(), "authorized_account".to_string()),
        (new_authorized_pubkey, "new_authorized_pubkey".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize(
        vote_account_pubkey,   // vote account to update
        &authorized.pubkey(),  // current authorized
        new_authorized_pubkey, // new vote signer/withdrawer
        vote_authorize,        // vote or withdraw
    )];

    let message = Message::new_with_payer(&ixs, Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &[config.signers[0]]);
    log_instruction_custom_error::<VoteError>(result, &config)
}

pub fn process_vote_update_validator(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    new_identity_account: SignerIndex,
) -> ProcessResult {
    let authorized_withdrawer = config.signers[1];
    let new_identity_account = config.signers[new_identity_account];
    let new_identity_pubkey = new_identity_account.pubkey();
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (&new_identity_pubkey, "new_identity_account".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::update_validator_identity(
        vote_account_pubkey,
        &authorized_withdrawer.pubkey(),
        &new_identity_pubkey,
    )];

    let message = Message::new_with_payer(&ixs, Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
    log_instruction_custom_error::<VoteError>(result, &config)
}

fn get_vote_account(
    rpc_client: &RpcClient,
    vote_account_pubkey: &Pubkey,
    commitment_config: CommitmentConfig,
) -> Result<(Account, VoteState), Box<dyn std::error::Error>> {
    let vote_account = rpc_client
        .get_account_with_commitment(vote_account_pubkey, commitment_config)?
        .value
        .ok_or_else(|| {
            CliError::RpcRequestError(format!("{:?} account does not exist", vote_account_pubkey))
        })?;

    if vote_account.owner != solana_vote_program::id() {
        return Err(CliError::RpcRequestError(format!(
            "{:?} is not a vote account",
            vote_account_pubkey
        ))
        .into());
    }
    let vote_state = VoteState::deserialize(&vote_account.data).map_err(|_| {
        CliError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    Ok((vote_account, vote_state))
}

pub fn process_show_vote_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
    commitment_config: CommitmentConfig,
) -> ProcessResult {
    let (vote_account, vote_state) =
        get_vote_account(rpc_client, vote_account_pubkey, commitment_config)?;

    let epoch_schedule = rpc_client.get_epoch_schedule()?;

    let mut votes: Vec<CliLockout> = vec![];
    let mut epoch_voting_history: Vec<CliEpochVotingHistory> = vec![];
    if !vote_state.votes.is_empty() {
        for vote in &vote_state.votes {
            votes.push(vote.into());
        }
        for (epoch, credits, prev_credits) in vote_state.epoch_credits() {
            let credits_earned = credits - prev_credits;
            let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
            epoch_voting_history.push(CliEpochVotingHistory {
                epoch: *epoch,
                slots_in_epoch,
                credits_earned,
            });
        }
    }

    let vote_account_data = CliVoteAccount {
        account_balance: vote_account.lamports,
        validator_identity: vote_state.node_pubkey.to_string(),
        authorized_voters: vote_state.authorized_voters().into(),
        authorized_withdrawer: vote_state.authorized_withdrawer.to_string(),
        credits: vote_state.credits(),
        commission: vote_state.commission,
        root_slot: vote_state.root_slot,
        recent_timestamp: vote_state.last_timestamp.clone(),
        votes,
        epoch_voting_history,
        use_lamports_unit,
    };

    Ok(config.output_format.formatted_string(&vote_account_data))
}

pub fn process_withdraw_from_vote_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    withdraw_authority: SignerIndex,
    lamports: u64,
    destination_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let withdraw_authority = config.signers[withdraw_authority];

    let ix = withdraw(
        vote_account_pubkey,
        &withdraw_authority.pubkey(),
        lamports,
        destination_account_pubkey,
    );

    let message = Message::new_with_payer(&[ix], Some(&config.signers[0].pubkey()));
    let mut transaction = Transaction::new_unsigned(message);
    transaction.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &transaction.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction_with_spinner(&mut transaction, &config.signers);
    log_instruction_custom_error::<VoteError>(result, &config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::signature::{read_keypair_file, write_keypair, Keypair, Signer};
    use tempfile::NamedTempFile;

    fn make_tmp_file() -> (String, NamedTempFile) {
        let tmp_file = NamedTempFile::new().unwrap();
        (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
    }

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let pubkey_string = pubkey.to_string();
        let keypair2 = Keypair::new();
        let pubkey2 = keypair2.pubkey();
        let pubkey2_string = pubkey2.to_string();

        let default_keypair = Keypair::new();
        let (default_keypair_file, mut tmp_file) = make_tmp_file();
        write_keypair(&default_keypair, tmp_file.as_file_mut()).unwrap();

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-authorize-voter",
            &pubkey_string,
            &default_keypair_file,
            &pubkey2_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_voter, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteAuthorize {
                    vote_account_pubkey: pubkey,
                    new_authorized_pubkey: pubkey2,
                    vote_authorize: VoteAuthorize::Voter
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        let authorized_keypair = Keypair::new();
        let (authorized_keypair_file, mut tmp_file) = make_tmp_file();
        write_keypair(&authorized_keypair, tmp_file.as_file_mut()).unwrap();

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-authorize-voter",
            &pubkey_string,
            &authorized_keypair_file,
            &pubkey2_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_voter, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteAuthorize {
                    vote_account_pubkey: pubkey,
                    new_authorized_pubkey: pubkey2,
                    vote_authorize: VoteAuthorize::Voter
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&authorized_keypair_file).unwrap().into(),
                ],
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();
        // Test CreateVoteAccount SubCommand
        let (identity_keypair_file, mut tmp_file) = make_tmp_file();
        let identity_keypair = Keypair::new();
        write_keypair(&identity_keypair, tmp_file.as_file_mut()).unwrap();
        let test_create_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &identity_keypair_file,
            "--commission",
            "10",
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    seed: None,
                    identity_account: 2,
                    authorized_voter: None,
                    authorized_withdrawer: None,
                    commission: 10,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    Box::new(keypair),
                    read_keypair_file(&identity_keypair_file).unwrap().into(),
                ],
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();

        let test_create_vote_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &identity_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account2, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    seed: None,
                    identity_account: 2,
                    authorized_voter: None,
                    authorized_withdrawer: None,
                    commission: 100,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    Box::new(keypair),
                    read_keypair_file(&identity_keypair_file).unwrap().into(),
                ],
            }
        );

        // test init with an authed voter
        let authed = Pubkey::new_rand();
        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();

        let test_create_vote_account3 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &identity_keypair_file,
            "--authorized-voter",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account3, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    seed: None,
                    identity_account: 2,
                    authorized_voter: Some(authed),
                    authorized_withdrawer: None,
                    commission: 100
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    Box::new(keypair),
                    read_keypair_file(&identity_keypair_file).unwrap().into(),
                ],
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();
        // test init with an authed withdrawer
        let test_create_vote_account4 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &identity_keypair_file,
            "--authorized-withdrawer",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account4, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    seed: None,
                    identity_account: 2,
                    authorized_voter: None,
                    authorized_withdrawer: Some(authed),
                    commission: 100
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    Box::new(keypair),
                    read_keypair_file(&identity_keypair_file).unwrap().into(),
                ],
            }
        );

        let test_update_validator = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-update-validator",
            &pubkey_string,
            &identity_keypair_file,
            &keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_update_validator, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteUpdateValidator {
                    vote_account_pubkey: pubkey,
                    new_identity_account: 2,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    read_keypair_file(&identity_keypair_file).unwrap().into(),
                ],
            }
        );

        // Test WithdrawFromVoteAccount subcommand
        let test_withdraw_from_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-vote-account",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        assert_eq!(
            parse_command(
                &test_withdraw_from_vote_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromVoteAccount {
                    vote_account_pubkey: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    destination_account_pubkey: pubkey,
                    withdraw_authority: 0,
                    lamports: 42_000_000_000
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        // Test WithdrawFromVoteAccount subcommand with authority
        let withdraw_authority = Keypair::new();
        let (withdraw_authority_file, mut tmp_file) = make_tmp_file();
        write_keypair(&withdraw_authority, tmp_file.as_file_mut()).unwrap();
        let test_withdraw_from_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-vote-account",
            &keypair_file,
            &pubkey_string,
            "42",
            "--authorized-withdrawer",
            &withdraw_authority_file,
        ]);
        assert_eq!(
            parse_command(
                &test_withdraw_from_vote_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromVoteAccount {
                    vote_account_pubkey: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    destination_account_pubkey: pubkey,
                    withdraw_authority: 1,
                    lamports: 42_000_000_000
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&withdraw_authority_file).unwrap().into()
                ],
            }
        );
    }
}
