use solana_config_program::ConfigKeys;
use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;

use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
    Lamports,
};
use bincode;
use serde::Deserialize;
use serde_json;
use solana_runtime::accounts_index::ScanConfig;
use solana_sdk::account::ReadableAccount;
use std::collections::HashMap;
use std::{collections::HashSet, io, sync::Arc};

struct ValidatorVoteInfo {
    balance: Lamports,
    last_vote: Slot,
    vote_credits: u64,
    identity: Pubkey,
    activated_stake: Lamports,
    validator_info: ValidatorInfo,
}

/// ValidatorInfo represents selected fields from the config account data.
#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
pub struct ValidatorInfo {
    pub name: String,
}

fn get_vote_state(bank: &Bank, vote_pubkey: &Pubkey) -> Option<ValidatorVoteInfo> {
    let default_vote_state = VoteState::default();
    let vote_accounts = bank.vote_accounts();
    let (activated_stake, vote_account) = vote_accounts.get(vote_pubkey)?;
    let vote_state = vote_account.vote_state();
    let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);

    let identity = vote_state.node_pubkey;

    let validator_info: ValidatorInfo = match get_validator_info_accounts(bank) {
        None => ValidatorInfo {
            name: "unknown".to_string(),
        },
        Some(mut info_map) => match info_map.remove(&identity) {
            None => ValidatorInfo {
                name: "unknown".to_string(),
            },
            Some(info) => info,
        },
    };

    let last_vote = vote_state.votes.back()?.slot;
    let balance = Lamports(bank.get_balance(&vote_pubkey));
    let vote_credits = vote_state.credits();
    Some(ValidatorVoteInfo {
        balance,
        last_vote,
        vote_credits,
        identity,
        activated_stake: Lamports(*activated_stake),
        validator_info,
    })
}

/// Return a map from validator identity account to ValidatorInfo.
///
/// To get the validator info (the validator metadata, such as name and Keybase
/// username), we have to extract that from the config account that stores the
/// validator info for a particular validator. But there is no way a priori to
/// know the address of the config account for a given validator; the only way
/// is to enumerate all config accounts and then find the one you are looking
/// for. This function builds a map from identity account to validator info.
pub fn get_validator_info_accounts(bank: &Bank) -> Option<HashMap<Pubkey, ValidatorInfo>> {
    use solana_sdk::config::program as config_program;

    let all_accounts = bank
        .get_program_accounts(&config_program::id(), &ScanConfig::default())
        .expect("failed to get program accounts");

    let mut mapping = HashMap::new();

    // Due to the structure of validator info (config accounts pointing to identity
    // accounts), it is possible for multiple config accounts to describe the same
    // validator. This is invalid, if it happens, we wouldn't know which config
    // account is the right one, so instead of making an arbitrary decision, we
    // ignore all validator infos for that identity.
    let mut bad_identities = HashSet::new();

    for (_, account) in &all_accounts {
        if let Some((validator_identity, info)) = deserialize_validator_info(account.data()) {
            let old_config_addr = mapping.insert(validator_identity, info);
            if old_config_addr.is_some() {
                bad_identities.insert(validator_identity);
            }
        }
    }

    for bad_identity in &bad_identities {
        mapping.remove(bad_identity);
    }

    Some(mapping)
}

/// Deserialize a config account that contains validator info.
///
/// Returns the validator identity account address, and the validator info for
/// that validator.
pub fn deserialize_validator_info(account_data: &[u8]) -> Option<(Pubkey, ValidatorInfo)> {
    use solana_account_decoder::validator_info;

    let key_list: ConfigKeys = bincode::deserialize(account_data).ok()?;

    if !key_list.keys.contains(&(validator_info::id(), false)) {
        return None;
    }

    // The validator identity pubkey lives at index 1.
    let (validator_identity, identity_signed_config) = key_list.keys[1];
    if !identity_signed_config {
        return None;
    }

    // A config account stores a list of (pubkey, bool) pairs, followed by json
    // data. To figure out where the json data starts, we need to know the size
    // fo the key list. The json data is not stored directly, it is serialized
    // with bincode as a string.
    let key_list_len = bincode::serialized_size(&key_list)
        .expect("We deserialized it, therefore it must be serializable.")
        as usize;
    let json_data: String = bincode::deserialize(&account_data[key_list_len..]).ok()?;
    let validator_info: ValidatorInfo = serde_json::from_str(&json_data).ok()?;

    Some((validator_identity, validator_info))
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
                name: "solana_validator_last_vote_slot",
                help:
                    "The voted-on slot of the validator's last vote that got included in the chain",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new(vote_info.last_vote)
                            .with_label("identity_account", vote_info.identity.to_string())
                            .with_label("vote_account", vote_account.to_string())
                            .with_label("validator_name", vote_info.validator_info.name),
                    )
                }),
            },
        )?;

        write_metric(
            out,
            &MetricFamily {
                name: "solana_validator_vote_account_balance_sol",
                help: "The balance of the vote account at the given address",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new_sol(vote_info.balance)
                            .with_label("identity_account", vote_info.identity.to_string())
                            .with_label("vote_account", vote_account.to_string())
                            .with_label("validator_name", vote_info.validator_info.name),
                    )
                }),
            },
        )?;

        write_metric(
            out,
            &MetricFamily {
                name: "solana_validator_vote_credits",
                help: "The total number of vote credits credited to this vote account",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new(vote_info.vote_credits)
                            .with_label("identity_account", vote_info.identity.to_string())
                            .with_label("vote_account", vote_account.to_string())
                            .with_label("validator_name", vote_info.validator_info.name),
                    )
                }),
            },
        )?;

        write_metric(
            out,
            &MetricFamily {
                name: "solana_validator_active_stake_sol",
                help: "The total amount of Sol actively staked to this validator",
                type_: "gauge",
                metrics: banks_with_commitments.for_each_commitment(|bank| {
                    let vote_info = get_vote_state(bank, vote_account)?;
                    Some(
                        Metric::new_sol(vote_info.activated_stake)
                            .with_label("identity_account", vote_info.identity.to_string())
                            .with_label("vote_account", vote_account.to_string())
                            .with_label("validator_name", vote_info.validator_info.name),
                    )
                }),
            },
        )?;
    }

    Ok(())
}
