use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;

use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
    Lamports,
};
use std::{collections::HashSet, io, sync::Arc};

struct ValidatorVoteInfo {
    balance: Lamports,
    last_vote: Slot,
    vote_credits: u64,
}

fn get_vote_state(bank: &Bank, vote_pubkey: &Pubkey) -> Option<ValidatorVoteInfo> {
    let default_vote_state = VoteState::default();
    let vote_accounts = bank.vote_accounts();
    let (_activated_stake, vote_account) = vote_accounts.get(vote_pubkey)?;
    let vote_state = vote_account.vote_state();
    let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);

    let last_vote = vote_state.votes.back()?.slot;
    let balance = Lamports(bank.get_balance(&vote_pubkey));
    let vote_credits = vote_state.credits();
    Some(ValidatorVoteInfo {
        balance,
        last_vote,
        vote_credits,
    })
}

pub fn write_cluster_metrics<W: io::Write>(
    banks_with_commitments: &BanksWithCommitments,
    cluster_info: &Arc<ClusterInfo>,
    vote_accounts: &Arc<HashSet<Pubkey>>,
    out: &mut W,
) -> io::Result<()> {
    let identity_pubkey = cluster_info.id();
    let version = cluster_info
        .get_node_version(&identity_pubkey)
        .unwrap_or_default();

    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_identity_public_key_info",
            help: "The node's current identity",
            type_: "counter",
            metrics: vec![
                Metric::new(1).with_label("identity_account", identity_pubkey.to_string())
            ],
        },
    )?;

    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_identity_balance_sol",
            help: "The balance of the node's identity account",
            type_: "gauge",
            metrics: banks_with_commitments.for_each_commitment(|bank| {
                Some(
                    Metric::new_sol(Lamports(bank.get_balance(&identity_pubkey)))
                        .with_label("identity_account", identity_pubkey.to_string()),
                )
            }),
        },
    )?;

    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_version_info",
            help: "The current Solana node's version",
            type_: "counter",
            metrics: vec![Metric::new(1).with_label("version", version.to_string())],
        },
    )?;

    // Vote accounts information
    for vote_account in vote_accounts.iter() {
        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_last_vote_slot",
                help:
                    "The voted-on slot of the validator's last vote that got included in the chain",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new(vote_info.last_vote)
                            .with_label("identity_account", identity_pubkey.to_string())
                            .with_label("vote_account", vote_account.to_string()),
                    )
                }),
            },
        )?;

        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_vote_balance_sol",
                help: "The balance of the vote account at the given address",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new_sol(vote_info.balance)
                            .with_label("identity_account", identity_pubkey.to_string())
                            .with_label("vote_account", vote_account.to_string()),
                    )
                }),
            },
        )?;

        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_vote_credits",
                help: "The total number of vote credits credited to this vote account",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new(vote_info.vote_credits)
                            .with_label("identity_account", identity_pubkey.to_string())
                            .with_label("vote_account", vote_account.to_string()),
                    )
                }),
            },
        )?;
    }

    Ok(())
}
