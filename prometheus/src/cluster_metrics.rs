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
}

fn get_vote_state(bank: &Bank, vote_pubkey: &Pubkey) -> Option<ValidatorVoteInfo> {
    let default_vote_state = VoteState::default();
    let vote_accounts = bank.vote_accounts();
    let (_activated_stake, vote_account) = vote_accounts.get(vote_pubkey)?;
    let vote_state = vote_account.vote_state();
    let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);

    let last_vote = vote_state.votes.back()?.slot;
    let balance = Lamports(bank.get_balance(&vote_pubkey));
    Some(ValidatorVoteInfo { balance, last_vote })
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
            help: "The node's finalized identity balance",
            type_: "gauge",
            metrics: banks_with_commitments.for_each_commitment(|bank| {
                Metric::new_sol(Lamports(bank.get_balance(&identity_pubkey)))
                    .with_label("identity_account", identity_pubkey.to_string())
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

    let validator_vote_info = ValidatorVoteInfo::new_from_bank(bank, &identity_pubkey);
    if let Some(vote_info) = validator_vote_info {
        vote_info.write_prometheus(out)?;
    }

    Ok(())
}
