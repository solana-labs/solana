use crate::parse_account_data::ParseAccountError;
use solana_sdk::{
    clock::{Epoch, Slot},
    pubkey::Pubkey,
};
use solana_vote_program::vote_state::{BlockTimestamp, Lockout, VoteState};

pub fn parse_vote(data: &[u8]) -> Result<RpcVoteState, ParseAccountError> {
    let mut vote_state = VoteState::deserialize(data).map_err(ParseAccountError::from)?;
    let epoch_credits = vote_state
        .epoch_credits()
        .iter()
        .map(|(epoch, credits, previous_credits)| RpcEpochCredits {
            epoch: *epoch,
            credits: *credits,
            previous_credits: *previous_credits,
        })
        .collect();
    let votes = vote_state
        .votes
        .iter()
        .map(|lockout| RpcLockout {
            slot: lockout.slot,
            confirmation_count: lockout.confirmation_count,
        })
        .collect();
    let authorized_voters = vote_state
        .authorized_voters()
        .iter()
        .map(|(epoch, authorized_voter)| RpcAuthorizedVoters {
            epoch: *epoch,
            authorized_voter: authorized_voter.to_string(),
        })
        .collect();
    let prior_voters = vote_state
        .prior_voters()
        .buf()
        .iter()
        .filter(|(pubkey, _, _)| pubkey != &Pubkey::default())
        .map(
            |(authorized_pubkey, epoch_of_last_authorized_switch, target_epoch)| RpcPriorVoters {
                authorized_pubkey: authorized_pubkey.to_string(),
                epoch_of_last_authorized_switch: *epoch_of_last_authorized_switch,
                target_epoch: *target_epoch,
            },
        )
        .collect();
    Ok(RpcVoteState {
        node_pubkey: vote_state.node_pubkey.to_string(),
        authorized_withdrawer: vote_state.authorized_withdrawer.to_string(),
        commission: vote_state.commission,
        votes,
        root_slot: vote_state.root_slot,
        authorized_voters,
        prior_voters,
        epoch_credits,
        last_timestamp: vote_state.last_timestamp,
    })
}

/// A duplicate representation of VoteState for pretty JSON serialization
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcVoteState {
    node_pubkey: String,
    authorized_withdrawer: String,
    commission: u8,
    votes: Vec<RpcLockout>,
    root_slot: Option<Slot>,
    authorized_voters: Vec<RpcAuthorizedVoters>,
    prior_voters: Vec<RpcPriorVoters>,
    epoch_credits: Vec<RpcEpochCredits>,
    last_timestamp: BlockTimestamp,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcLockout {
    slot: Slot,
    confirmation_count: u32,
}

impl From<&Lockout> for RpcLockout {
    fn from(lockout: &Lockout) -> Self {
        Self {
            slot: lockout.slot,
            confirmation_count: lockout.confirmation_count,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcAuthorizedVoters {
    epoch: Epoch,
    authorized_voter: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcPriorVoters {
    authorized_pubkey: String,
    epoch_of_last_authorized_switch: Epoch,
    target_epoch: Epoch,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcEpochCredits {
    epoch: Epoch,
    credits: u64,
    previous_credits: u64,
}
