use super::*;
use crate::vote_state::vote_state_0_23_5::VoteState0_23_5;
use solana_sdk::account_utils::StateMut;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum VoteStateVersions {
    V0_23_5(Box<VoteState0_23_5>),
    Current(Box<VoteState>),
}

impl VoteStateVersions {
    pub fn convert_to_current(self) -> VoteState {
        match self {
            VoteStateVersions::V0_23_5(state) => {
                let authorized_voters =
                    AuthorizedVoters::new(state.authorized_voter_epoch, state.authorized_voter);

                VoteState {
                    node_pubkey: state.node_pubkey,

                    /// the signer for withdrawals
                    authorized_withdrawer: state.authorized_withdrawer,

                    /// percentage (0-100) that represents what part of a rewards
                    ///  payout should be given to this VoteAccount
                    commission: state.commission,

                    votes: state.votes.clone(),

                    root_slot: state.root_slot,

                    /// the signer for vote transactions
                    authorized_voters,

                    /// history of prior authorized voters and the epochs for which
                    /// they were set, the bottom end of the range is inclusive,
                    /// the top of the range is exclusive
                    prior_voters: CircBuf::default(),

                    /// history of how many credits earned by the end of each epoch
                    ///  each tuple is (Epoch, credits, prev_credits)
                    epoch_credits: state.epoch_credits.clone(),

                    /// most recent timestamp submitted with a vote
                    last_timestamp: state.last_timestamp.clone(),
                }
            }
            VoteStateVersions::Current(state) => *state,
        }
    }

    pub fn convert_from_raw(account: &mut Account, pubkey: &Pubkey) {
        let vote_state: VoteState0_23_5 = account.state().unwrap_or_else(|e| {
            panic!(
                "Couldn't deserialize vote account {}, error: {:?}",
                pubkey, e
            )
        });

        let current_vote_state =
            VoteStateVersions::V0_23_5(Box::new(vote_state)).convert_to_current();
        VoteState::to(
            &VoteStateVersions::Current(Box::new(current_vote_state)),
            account,
        );
    }

    pub fn is_uninitialized(&self) -> bool {
        match self {
            VoteStateVersions::V0_23_5(vote_state) => {
                vote_state.authorized_voter == Pubkey::default()
            }

            VoteStateVersions::Current(vote_state) => vote_state.authorized_voters.is_empty(),
        }
    }
}
