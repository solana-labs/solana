use crate::cli::{
    build_balance_message, check_account_for_fee, check_unique_pubkeys,
    log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult,
};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_sdk::{
    account::Account, pubkey::Pubkey, signature::KeypairUtil, system_instruction::SystemError,
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
                    Arg::with_name("node_pubkey")
                        .index(2)
                        .value_name("VALIDATOR PUBKEY")
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
                        .help("The commission taken on reward redemption (0-100), default: 0"),
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
                ),
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
            SubCommand::with_name("show-vote-account")
                .about("Show the contents of a vote account")
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
        .subcommand(
            SubCommand::with_name("uptime")
                .about("Show the uptime of a validator, based on epoch voting history")
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
                    Arg::with_name("span")
                        .long("span")
                        .value_name("NUM OF EPOCHS")
                        .takes_value(true)
                        .help("Number of recent epochs to examine"),
                )
                .arg(
                    Arg::with_name("aggregate")
                        .long("aggregate")
                        .help("Aggregate uptime data across span"),
                ),
        )
    }
}

pub fn parse_vote_create_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let vote_account = keypair_of(matches, "vote_account").unwrap();
    let node_pubkey = pubkey_of(matches, "node_pubkey").unwrap();
    let commission = value_of(&matches, "commission").unwrap_or(0);
    let authorized_voter = pubkey_of(matches, "authorized_voter");
    let authorized_withdrawer = pubkey_of(matches, "authorized_withdrawer");

    Ok(CliCommandInfo {
        command: CliCommand::CreateVoteAccount {
            vote_account: vote_account.into(),
            node_pubkey,
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
        command: CliCommand::VoteAuthorize(
            vote_account_pubkey,
            new_authorized_pubkey,
            vote_authorize,
        ),
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

pub fn parse_vote_uptime_command(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let aggregate = matches.is_present("aggregate");
    let span = if matches.is_present("span") {
        Some(value_t_or_exit!(matches, "span", u64))
    } else {
        None
    };
    Ok(CliCommandInfo {
        command: CliCommand::Uptime {
            pubkey: vote_account_pubkey,
            aggregate,
            span,
        },
        require_keypair: false,
    })
}

pub fn process_create_vote_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    vote_account: &Keypair,
    node_pubkey: &Pubkey,
    authorized_voter: &Option<Pubkey>,
    authorized_withdrawer: &Option<Pubkey>,
    commission: u8,
) -> ProcessResult {
    let vote_account_pubkey = vote_account.pubkey();
    check_unique_pubkeys(
        (&vote_account_pubkey, "vote_account_pubkey".to_string()),
        (&node_pubkey, "node_pubkey".to_string()),
    )?;
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&vote_account_pubkey, "vote_account_pubkey".to_string()),
    )?;
    let required_balance =
        rpc_client.get_minimum_balance_for_rent_exemption(VoteState::size_of())?;
    let lamports = if required_balance > 0 {
        required_balance
    } else {
        1
    };
    let vote_init = VoteInit {
        node_pubkey: *node_pubkey,
        authorized_voter: authorized_voter.unwrap_or(vote_account_pubkey),
        authorized_withdrawer: authorized_withdrawer.unwrap_or(config.keypair.pubkey()),
        commission,
    };
    let ixs = vote_instruction::create_account(
        &config.keypair.pubkey(),
        &vote_account_pubkey,
        &vote_init,
        lamports,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_instructions(
        &[&config.keypair, vote_account],
        ixs,
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, vote_account]);
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

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<VoteError>(result)
}

fn get_vote_account(
    rpc_client: &RpcClient,
    vote_account_pubkey: &Pubkey,
) -> Result<(Account, VoteState), Box<dyn std::error::Error>> {
    let vote_account = rpc_client.get_account(vote_account_pubkey)?;

    if vote_account.owner != solana_vote_program::id() {
        return Err(CliError::RpcRequestError(
            format!("{:?} is not a vote account", vote_account_pubkey).to_string(),
        )
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
        "account balance: {}",
        build_balance_message(vote_account.lamports, use_lamports_unit, true)
    );
    println!("node id: {}", vote_state.node_pubkey);
    println!("authorized voter: {}", vote_state.authorized_voter);
    println!(
        "authorized withdrawer: {}",
        vote_state.authorized_withdrawer
    );
    println!("credits: {}", vote_state.credits());
    println!("commission: {}%", vote_state.commission);
    println!(
        "root slot: {}",
        match vote_state.root_slot {
            Some(slot) => slot.to_string(),
            None => "~".to_string(),
        }
    );
    println!("recent timestamp: {:?}", vote_state.last_timestamp);
    if !vote_state.votes.is_empty() {
        println!("recent votes:");
        for vote in &vote_state.votes {
            println!(
                "- slot: {}\n  confirmation count: {}",
                vote.slot, vote.confirmation_count
            );
        }

        println!("epoch voting history:");
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

pub fn process_uptime(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    vote_account_pubkey: &Pubkey,
    aggregate: bool,
    span: Option<u64>,
) -> ProcessResult {
    let (_vote_account, vote_state) = get_vote_account(rpc_client, vote_account_pubkey)?;

    let epoch_schedule = rpc_client.get_epoch_schedule()?;

    println!("Node id: {}", vote_state.node_pubkey);
    println!("Authorized voter: {}", vote_state.authorized_voter);
    if !vote_state.votes.is_empty() {
        println!("Uptime:");

        let epoch_credits_vec: Vec<(u64, u64, u64)> = vote_state.epoch_credits().copied().collect();

        let epoch_credits = if let Some(x) = span {
            epoch_credits_vec.iter().rev().take(x as usize)
        } else {
            epoch_credits_vec.iter().rev().take(epoch_credits_vec.len())
        };

        if aggregate {
            let (credits_earned, slots_in_epoch, epochs): (u64, u64, u64) =
                epoch_credits.fold((0, 0, 0), |acc, (epoch, credits, prev_credits)| {
                    let credits_earned = credits - prev_credits;
                    let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
                    (acc.0 + credits_earned, acc.1 + slots_in_epoch, acc.2 + 1)
                });
            let total_uptime = credits_earned as f64 / slots_in_epoch as f64;
            println!("{:.2}% over {} epochs", total_uptime * 100_f64, epochs,);
        } else {
            for (epoch, credits, prev_credits) in epoch_credits {
                let credits_earned = credits - prev_credits;
                let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
                let uptime = credits_earned as f64 / slots_in_epoch as f64;
                println!("- epoch: {} {:.2}% uptime", epoch, uptime * 100_f64,);
            }
        }
        if let Some(x) = span {
            if x > epoch_credits_vec.len() as u64 {
                println!("(span longer than available epochs)");
            }
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

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-authorize-voter",
            &pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&test_authorize_voter).unwrap(),
            CliCommandInfo {
                command: CliCommand::VoteAuthorize(pubkey, pubkey, VoteAuthorize::Voter),
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
                    node_pubkey,
                    authorized_voter: None,
                    authorized_withdrawer: None,
                    commission: 0,
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
                    node_pubkey,
                    authorized_voter: Some(authed),
                    authorized_withdrawer: None,
                    commission: 0
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
                    node_pubkey,
                    authorized_voter: None,
                    authorized_withdrawer: Some(authed),
                    commission: 0
                },
                require_keypair: true
            }
        );

        // Test Uptime Subcommand
        let pubkey = Pubkey::new_rand();
        let matches = test_commands.clone().get_matches_from(vec![
            "test",
            "uptime",
            &pubkey.to_string(),
            "--span",
            "4",
            "--aggregate",
        ]);
        assert_eq!(
            parse_command(&matches).unwrap(),
            CliCommandInfo {
                command: CliCommand::Uptime {
                    pubkey,
                    aggregate: true,
                    span: Some(4)
                },
                require_keypair: false
            }
        );
    }
}
