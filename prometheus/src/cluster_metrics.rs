use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;

use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
    Lamports,
};
use std::{io, sync::Arc};

struct ValidatorVoteInfo {
    vote_address: Pubkey,
    balance: Lamports,
    last_vote: Slot,
}

impl ValidatorVoteInfo {
    fn new_from_bank(bank: &Arc<Bank>, identity_pubkey: &Pubkey) -> Option<Self> {
        let vote_accounts = bank.vote_accounts();
        let vote_state_default = VoteState::default();
        vote_accounts
            .iter()
            .filter_map(|(&vote_pubkey, (_activated_stake, account))| {
                let vote_state = account.vote_state();
                let vote_state = vote_state.as_ref().unwrap_or(&vote_state_default);
                if identity_pubkey != &vote_state.node_pubkey {
                    return None;
                }
                let last_vote = vote_state.votes.back()?.slot;
                let vote_balance = Lamports(bank.get_balance(&vote_pubkey));
                Some(ValidatorVoteInfo {
                    vote_address: vote_pubkey,
                    balance: vote_balance,
                    last_vote,
                })
            })
            .next()
    }

    fn write_prometheus<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_vote_public_key_info",
                help: "The current Solana node's vote public key",
                type_: "count",
                metrics: vec![
                    Metric::new(1).with_label("vote_account", self.vote_address.to_string())
                ],
            },
        )?;
        // We can use this metric to track if the validator is making progress
        // by voting on the last slots.
        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_last_vote_slot",
                help:
                    "The voted-on slot of the validator's last vote that got included in the chain",
                type_: "gauge",
                metrics: vec![
                    Metric::new(self.last_vote).with_label("pubkey", self.vote_address.to_string())
                ],
            },
        )?;
        // Validator rewards go to vote account, we use this to track our own
        // rewards.
        write_metric(
            out,
            &MetricFamily {
                name: "solana_node_vote_balance_sol",
                help: "The current node's vote account balance",
                type_: "gauge",
                metrics: vec![Metric::new_sol(self.balance.clone())],
            },
        )
    }
}

pub fn write_cluster_metrics<W: io::Write>(
    banks_with_commitments: &BanksWithCommitments,
    cluster_info: &Arc<ClusterInfo>,
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
