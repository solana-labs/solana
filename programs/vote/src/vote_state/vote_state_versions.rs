extern crate solana_vote_program_0_23_5;

use super::*;
use solana_vote_program_0_23_5::vote_state::VoteState as VoteState_0_23_5;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum VoteStateVersions {
    V0_23_5(Box<VoteState_0_23_5>),
    Current(Box<VoteState>),
}

impl VoteStateVersions {
    pub fn to_current(&self) -> VoteStateVersions {
        match self {
            VoteStateVersions::V0_23_5(state) => {
                let authorized_voter = Pubkey::new(state.authorized_withdrawer.as_ref());
                let authorized_voters =
                    AuthorizedVoters::new(state.authorized_voter_epoch, authorized_voter);
                let last_timestamp = BlockTimestamp {
                    slot: state.last_timestamp.slot,
                    timestamp: state.last_timestamp.timestamp,
                };
                let votes: VecDeque<_> = state
                    .votes
                    .iter()
                    .map(|lockout| {
                        let mut current_lockout = Lockout::new(lockout.slot);
                        current_lockout.confirmation_count = lockout.confirmation_count;
                        current_lockout
                    })
                    .collect();

                let current_state = VoteState {
                    node_pubkey: Pubkey::new(state.node_pubkey.as_ref()),

                    /// the signer for withdrawals
                    authorized_withdrawer: Pubkey::new(state.authorized_withdrawer.as_ref()),

                    /// percentage (0-100) that represents what part of a rewards
                    ///  payout should be given to this VoteAccount
                    commission: state.commission,

                    votes,

                    root_slot: state.root_slot,

                    /// the signer for vote transactions
                    authorized_voters,

                    /// history of prior authorized voters and the epochs for which
                    /// they were set, the bottom end of the range is inclusive,
                    /// the top of the range is exclusive
                    prior_voters: CircBuf::default(),

                    /// history of how many credits earned by the end of each epoch
                    ///  each tuple is (Epoch, credits, prev_credits)
                    epoch_credits: state.epoch_credits().clone(),

                    /// most recent timestamp submitted with a vote
                    last_timestamp,
                };
                VoteStateVersions::Current(Box::new(current_state))
            }
            VoteStateVersions::Current(state) => VoteStateVersions::Current(state.clone()),
        }
    }
}
