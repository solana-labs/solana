use crate::{
    input_parsers::*,
    wallet::{ProcessResult, WalletCommand, WalletConfig, WalletError},
};
use clap::{value_t_or_exit, ArgMatches};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_state::VoteState;

pub fn parse_vote_get_account_command(
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, WalletError> {
    let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
    Ok(WalletCommand::ShowVoteAccount(vote_account_pubkey))
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
    println!(
        "authorized voter pubkey: {}",
        vote_state.authorized_voter_pubkey
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
    println!(
        "Authorized voter pubkey: {}",
        vote_state.authorized_voter_pubkey
    );
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
            let (credits_earned, slots_in_epoch): (u64, u64) =
                epoch_credits.fold((0, 0), |acc, (epoch, credits, prev_credits)| {
                    let credits_earned = credits - prev_credits;
                    let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
                    (acc.0 + credits_earned, acc.1 + slots_in_epoch)
                });
            let total_uptime = credits_earned as f64 / slots_in_epoch as f64;
            println!(
                "{:.2}% over {} epochs",
                total_uptime * 100_f64,
                span.unwrap_or(epoch_credits_vec.len() as u64)
            );
        } else {
            for (epoch, credits, prev_credits) in epoch_credits {
                let credits_earned = credits - prev_credits;
                let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
                let uptime = credits_earned as f64 / slots_in_epoch as f64;
                println!("- epoch: {} {:.2}% uptime", epoch, uptime * 100_f64,);
            }
        }
    }
    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::{app, parse_command};

    #[test]
    fn test_parse_command() {
        let pubkey = Pubkey::new_rand();
        let matches = app("test", "desc", "version").get_matches_from(vec![
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

        let matches = app("test", "desc", "version").get_matches_from(vec![
            "test",
            "uptime",
            "badpubkey",
        ]);
        assert!(parse_command(&pubkey, &matches).is_err());

        let matches = app("test", "desc", "version").get_matches_from(vec![
            "test",
            "uptime",
            &pubkey.to_string(),
            "--span",
            "NAN",
        ]);
        assert!(parse_command(&pubkey, &matches).is_err());
    }
// TODO: Add process tests
}
