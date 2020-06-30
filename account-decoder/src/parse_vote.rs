use crate::parse_account_data::ParseAccountError;
use solana_sdk::{
    clock::{Epoch, Slot},
    pubkey::Pubkey,
};
use solana_vote_program::vote_state::{BlockTimestamp, Lockout, VoteState};

pub fn parse_vote(data: &[u8]) -> Result<DisplayVoteState, ParseAccountError> {
    let mut vote_state = VoteState::deserialize(data).map_err(ParseAccountError::from)?;
    let epoch_credits = vote_state
        .epoch_credits()
        .iter()
        .map(|(epoch, credits, previous_credits)| DisplayEpochCredits {
            epoch: *epoch,
            credits: *credits,
            previous_credits: *previous_credits,
        })
        .collect();
    let votes = vote_state
        .votes
        .iter()
        .map(|lockout| DisplayLockout {
            slot: lockout.slot,
            confirmation_count: lockout.confirmation_count,
        })
        .collect();
    let authorized_voters = vote_state
        .authorized_voters()
        .iter()
        .map(|(epoch, authorized_voter)| DisplayAuthorizedVoters {
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
            |(authorized_pubkey, epoch_of_last_authorized_switch, target_epoch)| {
                DisplayPriorVoters {
                    authorized_pubkey: authorized_pubkey.to_string(),
                    epoch_of_last_authorized_switch: *epoch_of_last_authorized_switch,
                    target_epoch: *target_epoch,
                }
            },
        )
        .collect();
    Ok(DisplayVoteState {
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
pub struct DisplayVoteState {
    node_pubkey: String,
    authorized_withdrawer: String,
    commission: u8,
    votes: Vec<DisplayLockout>,
    root_slot: Option<Slot>,
    authorized_voters: Vec<DisplayAuthorizedVoters>,
    prior_voters: Vec<DisplayPriorVoters>,
    epoch_credits: Vec<DisplayEpochCredits>,
    last_timestamp: BlockTimestamp,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisplayLockout {
    slot: Slot,
    confirmation_count: u32,
}

impl From<&Lockout> for DisplayLockout {
    fn from(lockout: &Lockout) -> Self {
        Self {
            slot: lockout.slot,
            confirmation_count: lockout.confirmation_count,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisplayAuthorizedVoters {
    epoch: Epoch,
    authorized_voter: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisplayPriorVoters {
    authorized_pubkey: String,
    epoch_of_last_authorized_switch: Epoch,
    target_epoch: Epoch,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisplayEpochCredits {
    epoch: Epoch,
    credits: u64,
    previous_credits: u64,
}
