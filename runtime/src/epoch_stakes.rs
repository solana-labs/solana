use crate::stakes::Stakes;
use serde::{Deserialize, Serialize};
use solana_sdk::{account::Account, clock::Epoch, pubkey::Pubkey};
use solana_vote_program::vote_state::VoteState;
use std::{collections::HashMap, sync::Arc};

pub type NodeIdToVoteAccounts = HashMap<Pubkey, Vec<Pubkey>>;
pub type EpochAuthorizedVoters = HashMap<Pubkey, Pubkey>;

#[derive(Clone, Serialize, Deserialize)]
pub struct EpochStakes {
    stakes: Arc<Stakes>,
    total_stake: u64,
    node_id_to_vote_accounts: Arc<NodeIdToVoteAccounts>,
    epoch_authorized_voters: Arc<EpochAuthorizedVoters>,
}

impl EpochStakes {
    pub fn new(stakes: &Stakes, leader_schedule_epoch: Epoch) -> Self {
        let total_stake = stakes
            .vote_accounts()
            .iter()
            .map(|(_, (stake, _))| stake)
            .sum();
        let epoch_vote_accounts = Stakes::vote_accounts(stakes);
        let (node_id_to_vote_accounts, epoch_authorized_voters) =
            Self::parse_epoch_vote_accounts(&epoch_vote_accounts, leader_schedule_epoch);
        Self {
            stakes: Arc::new(stakes.clone()),
            total_stake,
            node_id_to_vote_accounts: Arc::new(node_id_to_vote_accounts),
            epoch_authorized_voters: Arc::new(epoch_authorized_voters),
        }
    }

    pub fn stakes(&self) -> &Stakes {
        &self.stakes
    }

    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    pub fn node_id_to_vote_accounts(&self) -> &NodeIdToVoteAccounts {
        &self.node_id_to_vote_accounts
    }

    pub fn epoch_authorized_voters(&self) -> &EpochAuthorizedVoters {
        &self.epoch_authorized_voters
    }

    fn parse_epoch_vote_accounts(
        epoch_vote_accounts: &HashMap<Pubkey, (u64, Account)>,
        leader_schedule_epoch: Epoch,
    ) -> (NodeIdToVoteAccounts, EpochAuthorizedVoters) {
        let mut node_id_to_vote_accounts: NodeIdToVoteAccounts = HashMap::new();
        let epoch_authorized_voters = epoch_vote_accounts
            .iter()
            .filter_map(|(key, (stake, account))| {
                let vote_state = VoteState::from(&account);
                if vote_state.is_none() {
                    datapoint_warn!(
                        "parse_epoch_vote_accounts",
                        (
                            "warn",
                            format!("Unable to get vote_state from account {}", key),
                            String
                        ),
                    );
                    return None;
                }
                let vote_state = vote_state.unwrap();
                if *stake > 0 {
                    // Read out the authorized voters
                    let authorized_voter = vote_state
                        .authorized_voters()
                        .get_authorized_voter(leader_schedule_epoch)
                        .expect("Authorized voter for current epoch must be known");

                    node_id_to_vote_accounts
                        .entry(vote_state.node_pubkey)
                        .or_default()
                        .push(*key);

                    Some((*key, authorized_voter))
                } else {
                    None
                }
            })
            .collect();
        (node_id_to_vote_accounts, epoch_authorized_voters)
    }
}
