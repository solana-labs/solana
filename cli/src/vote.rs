use crate::cli::{
    build_balance_message, check_account_for_fee, check_unique_pubkeys,
    log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult,
};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    account::Account,
    message::Message,
    pubkey::Pubkey,
    signature::Keypair,
    signature::Signer,
    system_instruction::{create_address_with_seed, SystemError},
    transaction::Transaction,
};
use solana_vote_program::{
    vote_instruction::{self, VoteError},
    vote_state::{VoteAuthorize, VoteInit, VoteState},
};

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
                        .value_name("VOTE ACCOUNT KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword)
                        .help("Vote account keypair to fund"),
                )
                .arg(
                    Arg::with_name("identity_pubkey")
                        .index(2)
                        .value_name("VALIDATOR IDENTITY PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Validator that will vote with this account"),
                )
                .arg(
                    Arg::with_name("commission")
                        .long("commission")
                        .value_name("NUM")
                        .takes_value(true)
                        .default_value("100")
                        .help("The commission taken on reward redemption (0-100)"),
                )
                .arg(
                    Arg::with_name("authorized_voter")
                        .long("authorized-voter")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Public key of the authorized voter (defaults to vote account)"),
                )
                .arg(
                    Arg::with_name("authorized_withdrawer")
                        .long("authorized-withdrawer")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Public key of the authorized withdrawer (defaults to cli config pubkey)"),
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("SEED STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the VOTE ACCOUNT pubkey")
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-update-validator")
                .about("Update the vote account's validator identity")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account to update"),
                )
                .arg(
                    Arg::with_name("new_identity_pubkey")
                        .index(2)
                        .value_name("NEW VALIDATOR IDENTITY PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New validator that will vote with this account"),
                )
                .arg(
                    Arg::with_name("authorized_voter")
                        .index(3)
                        .value_name("AUTHORIZED VOTER KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair)
                        .help("Authorized voter keypair"),
                )
        )
        .subcommand(
            SubCommand::with_name("vote-authorize-voter")
                .about("Authorize a new vote signing keypair for the given vote account")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account in which to set the authorized voter"),
                )
                .arg(
                    Arg::with_name("new_authorized_pubkey")
                        .index(2)
                        .value_name("NEW VOTER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New vote signer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-authorize-withdrawer")
                .about("Authorize a new withdraw signing keypair for the given vote account")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account in which to set the authorized withdrawer"),
                )
                .arg(
                    Arg::with_name("new_authorized_pubkey")
                        .index(2)
                        .value_name("NEW WITHDRAWER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New withdrawer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("vote-account")
                .about("Show the contents of a vote account")
                .alias("show-vote-account")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account pubkey"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
    }
}

pub fn parse_vote_create_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let vote_account = keypair_of(matches, "vote_account").unwrap();
    let seed = matches.value_of("seed").map(|s| s.to_string());
    let identity_pubkey = pubkey_of(matches, "identity_pubkey").unwrap();
    let commission = value_t_or_exit!(matches, "commission", u8);
    let authorized_voter = pubkey_of(matches, "authorized_voter");
    let authorized_withdrawer = pubkey_of(matches, "authorized_withdrawer");

    Ok(CliCommandInfo {
        command: CliCommand::CreateVoteAccount {
            vote_account: vote_account.into(),
            seed,
            node_pubkey: identity_pubkey,
            authorized_voter,
            authorized_withdrawer,
            commission,
        },
        require_keypair: true,
    })
}

pub fn parse_vote_authorize(
    matches: &ArgMatches<'_>,
    vote_authorize: VoteAuthorize,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let new_authorized_pubkey = pubkey_of(matches, "new_authorized_pubkey").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::VoteAuthorize {
            vote_account_pubkey,
            new_authorized_pubkey,
            vote_authorize,
        },
        require_keypair: true,
    })
}

pub fn parse_vote_update_validator(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let new_identity_pubkey = pubkey_of(matches, "new_identity_pubkey").unwrap();
    let authorized_voter = keypair_of(matches, "authorized_voter").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::VoteUpdateValidator {
            vote_account_pubkey,
            new_identity_pubkey,
            authorized_voter: authorized_voter.into(),
        },
        require_keypair: true,
    })
}

pub fn parse_vote_get_account_command(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let use_lamports_unit = matches.is_present("lamports");
    Ok(CliCommandInfo {
        command: CliCommand::ShowVoteAccount {
            pubkey: vote_account_pubkey,
            use_lamports_unit,
        },
        require_keypair: false,
    })
}

pub fn process_create_vote_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account: &Keypair,
    seed: &Option<String>,
    identity_pubkey: &Pubkey,
    authorized_voter: &Option<Pubkey>,
    authorized_withdrawer: &Option<Pubkey>,
    commission: u8,
) -> ProcessResult {
    let vote_account_pubkey = vote_account.pubkey();
    let vote_account_address = if let Some(seed) = seed {
        create_address_with_seed(&vote_account_pubkey, &seed, &solana_vote_program::id())?
    } else {
        vote_account_pubkey
    };
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&vote_account_address, "vote_account".to_string()),
    )?;

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
        node_pubkey: *identity_pubkey,
        authorized_voter: authorized_voter.unwrap_or(vote_account_pubkey),
        authorized_withdrawer: authorized_withdrawer.unwrap_or(vote_account_pubkey),
        commission,
    };

    let ixs = if let Some(seed) = seed {
        vote_instruction::create_account_with_seed(
            &config.keypair.pubkey(), // from
            &vote_account_address,    // to
            &vote_account_pubkey,     // base
            seed,                     // seed
            &vote_init,
            required_balance,
        )
    } else {
        vote_instruction::create_account(
            &config.keypair.pubkey(),
            &vote_account_pubkey,
            &vote_init,
            required_balance,
        )
    };
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let signers = if vote_account_pubkey != config.keypair.pubkey() {
        vec![config.keypair.as_ref(), vote_account] // both must sign if `from` and `to` differ
    } else {
        vec![config.keypair.as_ref()] // when stake_account == config.keypair and there's a seed, we only need one signature
    };

    let message = Message::new(ixs);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &signers);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_vote_authorize(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (new_authorized_pubkey, "new_authorized_pubkey".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize(
        vote_account_pubkey,      // vote account to update
        &config.keypair.pubkey(), // current authorized voter
        new_authorized_pubkey,    // new vote signer/withdrawer
        vote_authorize,           // vote or withdraw
    )];

    let message = Message::new_with_payer(ixs, Some(&config.keypair.pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&[config.keypair.as_ref()], recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[config.keypair.as_ref()]);
    log_instruction_custom_error::<VoteError>(result)
}

pub fn process_vote_update_validator(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    new_identity_pubkey: &Pubkey,
    authorized_voter: &Keypair,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (new_identity_pubkey, "new_identity_pubkey".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::update_node(
        vote_account_pubkey,
        &authorized_voter.pubkey(),
        new_identity_pubkey,
    )];

    let message = Message::new_with_payer(ixs, Some(&config.keypair.pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(
        &[config.keypair.as_ref(), authorized_voter],
        recent_blockhash,
    )?;
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[config.keypair.as_ref()]);
    log_instruction_custom_error::<VoteError>(result)
}

fn get_vote_account(
    rpc_client: &RpcClient,
    vote_account_pubkey: &Pubkey,
) -> Result<(Account, VoteState), Box<dyn std::error::Error>> {
    let vote_account = rpc_client.get_account(vote_account_pubkey)?;

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
    _config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let (vote_account, vote_state) = get_vote_account(rpc_client, vote_account_pubkey)?;

    let epoch_schedule = rpc_client.get_epoch_schedule()?;

    println!(
        "Account Balance: {}",
        build_balance_message(vote_account.lamports, use_lamports_unit, true)
    );
    println!("Validator Identity: {}", vote_state.node_pubkey);
    println!("Authorized Voter: {}", vote_state.authorized_voter);
    println!(
        "Authorized Withdrawer: {}",
        vote_state.authorized_withdrawer
    );
    println!("Credits: {}", vote_state.credits());
    println!("Commission: {}%", vote_state.commission);
    println!(
        "Root Slot: {}",
        match vote_state.root_slot {
            Some(slot) => slot.to_string(),
            None => "~".to_string(),
        }
    );
    println!("Recent Timestamp: {:?}", vote_state.last_timestamp);
    if !vote_state.votes.is_empty() {
        println!("recent votes:");
        for vote in &vote_state.votes {
            println!(
                "- slot: {}\n  confirmation count: {}",
                vote.slot, vote.confirmation_count
            );
        }

        println!("Epoch Voting History:");
        for (epoch, credits, prev_credits) in vote_state.epoch_credits() {
            let credits_earned = credits - prev_credits;
            let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
            println!(
                "- epoch: {}\n  slots in epoch: {}\n  credits earned: {}",
                epoch, slots_in_epoch, credits_earned,
            );
        }
    }
    Ok("".to_string())
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
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let pubkey_string = pubkey.to_string();
        let keypair2 = Keypair::new();
        let pubkey2 = keypair2.pubkey();
        let pubkey2_string = pubkey2.to_string();

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-authorize-voter",
            &pubkey_string,
            &pubkey2_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_voter).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteAuthorize {
                    vote_account_pubkey: pubkey,
                    new_authorized_pubkey: pubkey2,
                    vote_authorize: VoteAuthorize::Voter
                },
                require_keypair: true
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();
        // Test CreateVoteAccount SubCommand
        let node_pubkey = Pubkey::new_rand();
        let node_pubkey_string = format!("{}", node_pubkey);
        let test_create_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &node_pubkey_string,
            "--commission",
            "10",
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    vote_account: keypair.into(),
                    seed: None,
                    node_pubkey,
                    authorized_voter: None,
                    authorized_withdrawer: None,
                    commission: 10,
                },
                require_keypair: true
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let keypair = Keypair::new();
        write_keypair(&keypair, tmp_file.as_file_mut()).unwrap();

        let test_create_vote_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &keypair_file,
            &node_pubkey_string,
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account2).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    vote_account: keypair.into(),
                    seed: None,
                    node_pubkey,
                    authorized_voter: None,
                    authorized_withdrawer: None,
                    commission: 100,
                },
                require_keypair: true
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
            &node_pubkey_string,
            "--authorized-voter",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account3).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    vote_account: keypair.into(),
                    seed: None,
                    node_pubkey,
                    authorized_voter: Some(authed),
                    authorized_withdrawer: None,
                    commission: 100
                },
                require_keypair: true
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
            &node_pubkey_string,
            "--authorized-withdrawer",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_create_vote_account4).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateVoteAccount {
                    vote_account: keypair.into(),
                    seed: None,
                    node_pubkey,
                    authorized_voter: None,
                    authorized_withdrawer: Some(authed),
                    commission: 100
                },
                require_keypair: true
            }
        );

        let test_update_validator = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-update-validator",
            &pubkey_string,
            &pubkey2_string,
            &keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_update_validator).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteUpdateValidator {
                    vote_account_pubkey: pubkey,
                    new_identity_pubkey: pubkey2,
                    authorized_voter: solana_sdk::signature::read_keypair_file(&keypair_file)
                        .unwrap()
                        .into(),
                },
                require_keypair: true
            }
        );
    }
}
