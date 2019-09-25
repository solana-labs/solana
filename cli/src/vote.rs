use crate::{
    input_parsers::*,
    wallet::{
        check_account_for_fee, check_unique_pubkeys, log_instruction_custom_error, ProcessResult,
        WalletCommand, WalletConfig, WalletError,
    },
};
use clap::{value_t_or_exit, ArgMatches};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction::SystemError,
    transaction::Transaction,
};
use solana_vote_api::{
    vote_instruction::{self, VoteError},
    vote_state::{VoteAuthorize, VoteInit, VoteState},
};

pub fn parse_vote_create_account(matches: &ArgMatches<'_>) -> Result<WalletCommand, WalletError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let node_pubkey = pubkey_of(matches, "node_pubkey").unwrap();
    let commission = value_of(&matches, "commission").unwrap_or(0);
    let authorized_voter = pubkey_of(matches, "authorized_voter").unwrap_or(vote_account_pubkey);
    let authorized_withdrawer =
        pubkey_of(matches, "authorized_withdrawer").unwrap_or(vote_account_pubkey);
    let lamports = matches
        .value_of("lamports")
        .unwrap()
        .parse()
        .map_err(|err| WalletError::BadParameter(format!("Invalid lamports: {:?}", err)))?;
    Ok(WalletCommand::CreateVoteAccount(
        vote_account_pubkey,
        VoteInit {
            node_pubkey,
            authorized_voter,
            authorized_withdrawer,
            commission,
        },
        lamports,
    ))
}

pub fn parse_vote_authorize(
    matches: &ArgMatches<'_>,
    vote_authorize: VoteAuthorize,
) -> Result<WalletCommand, WalletError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let authorized_keypair = keypair_of(matches, "authorized_keypair_file").unwrap();
    let new_authorized_pubkey = pubkey_of(matches, "new_authorized_pubkey").unwrap();

    Ok(WalletCommand::VoteAuthorize(
        vote_account_pubkey,
        authorized_keypair,
        new_authorized_pubkey,
        vote_authorize,
    ))
}

pub fn parse_vote_get_account_command(
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, WalletError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    Ok(WalletCommand::ShowVoteAccount(vote_account_pubkey))
}

pub fn process_create_vote_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
    vote_init: &VoteInit,
    lamports: u64,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (&vote_init.node_pubkey, "node_pubkey".to_string()),
    )?;
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
    )?;
    let ixs = vote_instruction::create_account(
        &config.keypair.pubkey(),
        vote_account_pubkey,
        vote_init,
        lamports,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_vote_authorize(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
    authorized_keypair: &Keypair,
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (new_authorized_pubkey, "new_authorized_pubkey".to_string()),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize(
        vote_account_pubkey,          // vote account to update
        &authorized_keypair.pubkey(), // current authorized voter (often the vote account itself)
        new_authorized_pubkey,        // new vote signer
        vote_authorize,               // vote or withdraw
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &authorized_keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, &authorized_keypair]);
    log_instruction_custom_error::<VoteError>(result)
}

pub fn parse_vote_uptime_command(matches: &ArgMatches<'_>) -> Result<WalletCommand, WalletError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    let aggregate = matches.is_present("aggregate");
    let span = if matches.is_present("span") {
        Some(value_t_or_exit!(matches, "span", u64))
    } else {
        None
    };
    Ok(WalletCommand::Uptime {
        pubkey: vote_account_pubkey,
        aggregate,
        span,
    })
}

pub fn process_show_vote_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
) -> ProcessResult {
    let vote_account = rpc_client.get_account(vote_account_pubkey)?;

    if vote_account.owner != solana_vote_api::id() {
        Err(WalletError::RpcRequestError(
            format!("{:?} is not a vote account", vote_account_pubkey).to_string(),
        ))?;
    }

    let vote_state = VoteState::deserialize(&vote_account.data).map_err(|_| {
        WalletError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    println!("account lamports: {}", vote_account.lamports);
    println!("node id: {}", vote_state.node_pubkey);
    println!("authorized voter: {}", vote_state.authorized_voter);
    println!(
        "authorized withdrawer: {}",
        vote_state.authorized_withdrawer
    );
    println!("credits: {}", vote_state.credits());
    println!(
        "commission: {}%",
        f64::from(vote_state.commission) / f64::from(std::u32::MAX)
    );
    println!(
        "root slot: {}",
        match vote_state.root_slot {
            Some(slot) => slot.to_string(),
            None => "~".to_string(),
        }
    );
    if !vote_state.votes.is_empty() {
        println!("recent votes:");
        for vote in &vote_state.votes {
            println!(
                "- slot: {}\n  confirmation count: {}",
                vote.slot, vote.confirmation_count
            );
        }

        // TODO: Use the real GenesisBlock from the cluster.
        let genesis_block = solana_sdk::genesis_block::GenesisBlock::default();
        let epoch_schedule = solana_runtime::epoch_schedule::EpochSchedule::new(
            genesis_block.slots_per_epoch,
            genesis_block.stakers_slot_offset,
            genesis_block.epoch_warmup,
        );

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
    _config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
    aggregate: bool,
    span: Option<u64>,
) -> ProcessResult {
    let vote_account = rpc_client.get_account(vote_account_pubkey)?;

    if vote_account.owner != solana_vote_api::id() {
        Err(WalletError::RpcRequestError(
            format!("{:?} is not a vote account", vote_account_pubkey).to_string(),
        ))?;
    }

    let vote_state = VoteState::deserialize(&vote_account.data).map_err(|_| {
        WalletError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    println!("Node id: {}", vote_state.node_pubkey);
    println!("Authorized voter: {}", vote_state.authorized_voter);
    if !vote_state.votes.is_empty() {
        println!("Uptime:");

        // TODO: Use the real GenesisBlock from the cluster.
        let genesis_block = solana_sdk::genesis_block::GenesisBlock::default();
        let epoch_schedule = solana_runtime::epoch_schedule::EpochSchedule::new(
            genesis_block.slots_per_epoch,
            genesis_block.stakers_slot_offset,
            genesis_block.epoch_warmup,
        );

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
    use crate::wallet::{app, parse_command};
    use solana_sdk::signature::write_keypair;
    use std::fs;

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let pubkey = Pubkey::new_rand();
        let pubkey_string = format!("{}", pubkey);

        // Test AuthorizeVoter Subcommand
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();
        let keypair_file = format!("{}/tmp/keypair_file-{}", out_dir, keypair.pubkey());
        let _ = write_keypair(&keypair, &keypair_file).unwrap();

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "vote-authorize-voter",
            &pubkey_string,
            &keypair_file,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_authorize_voter).unwrap(),
            WalletCommand::VoteAuthorize(pubkey, keypair, pubkey, VoteAuthorize::Voter)
        );
        fs::remove_file(&keypair_file).unwrap();

        // Test CreateVoteAccount SubCommand
        let node_pubkey = Pubkey::new_rand();
        let node_pubkey_string = format!("{}", node_pubkey);
        let test_create_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
            "--commission",
            "10",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account).unwrap(),
            WalletCommand::CreateVoteAccount(
                pubkey,
                VoteInit {
                    node_pubkey,
                    authorized_voter: pubkey,
                    authorized_withdrawer: pubkey,
                    commission: 10
                },
                50
            )
        );
        let test_create_vote_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account2).unwrap(),
            WalletCommand::CreateVoteAccount(
                pubkey,
                VoteInit {
                    node_pubkey,
                    authorized_voter: pubkey,
                    authorized_withdrawer: pubkey,
                    commission: 0
                },
                50
            )
        );
        // test init with an authed voter
        let authed = Pubkey::new_rand();
        let test_create_vote_account3 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
            "--authorized-voter",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account3).unwrap(),
            WalletCommand::CreateVoteAccount(
                pubkey,
                VoteInit {
                    node_pubkey,
                    authorized_voter: authed,
                    authorized_withdrawer: pubkey,
                    commission: 0
                },
                50
            )
        );
        // test init with an authed withdrawer
        let test_create_vote_account4 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
            "--authorized-withdrawer",
            &authed.to_string(),
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account4).unwrap(),
            WalletCommand::CreateVoteAccount(
                pubkey,
                VoteInit {
                    node_pubkey,
                    authorized_voter: pubkey,
                    authorized_withdrawer: authed,
                    commission: 0
                },
                50
            )
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
            parse_command(&pubkey, &matches).unwrap(),
            WalletCommand::Uptime {
                pubkey,
                aggregate: true,
                span: Some(4)
            }
        );
    }
    // TODO: Add process tests
}
