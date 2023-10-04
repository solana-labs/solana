use super::{vote_state_0_23_5::VoteState0_23_5, vote_state_1_14_11::VoteState1_14_11, *};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum VoteStateVersions {
    V0_23_5(Box<VoteState0_23_5>),
    V1_14_11(Box<VoteState1_14_11>),
    Current(Box<VoteState>),
}

impl VoteStateVersions {
    pub fn new_current(vote_state: VoteState) -> Self {
        Self::Current(Box::new(vote_state))
    }

    pub fn convert_to_current(self) -> VoteState {
        match self {
            VoteStateVersions::V0_23_5(state) => {
                let authorized_voters =
                    AuthorizedVoters::new(state.authorized_voter_epoch, state.authorized_voter);

                VoteState {
                    node_pubkey: state.node_pubkey,

                    authorized_withdrawer: state.authorized_withdrawer,

                    commission: state.commission,

                    votes: Self::landed_votes_from_lockouts(state.votes),

                    root_slot: state.root_slot,

                    authorized_voters,

                    prior_voters: CircBuf::default(),

                    epoch_credits: state.epoch_credits.clone(),

                    last_timestamp: state.last_timestamp.clone(),
                }
            }

            VoteStateVersions::V1_14_11(state) => VoteState {
                node_pubkey: state.node_pubkey,
                authorized_withdrawer: state.authorized_withdrawer,
                commission: state.commission,

                votes: Self::landed_votes_from_lockouts(state.votes),

                root_slot: state.root_slot,

                authorized_voters: state.authorized_voters.clone(),

                prior_voters: state.prior_voters,

                epoch_credits: state.epoch_credits,

                last_timestamp: state.last_timestamp,
            },

            VoteStateVersions::Current(state) => *state,
        }
    }

    fn landed_votes_from_lockouts(lockouts: VecDeque<Lockout>) -> VecDeque<LandedVote> {
        lockouts.into_iter().map(|lockout| lockout.into()).collect()
    }

    pub fn is_uninitialized(&self) -> bool {
        match self {
            VoteStateVersions::V0_23_5(vote_state) => {
                vote_state.authorized_voter == Pubkey::default()
            }

            VoteStateVersions::V1_14_11(vote_state) => vote_state.authorized_voters.is_empty(),

            VoteStateVersions::Current(vote_state) => vote_state.authorized_voters.is_empty(),
        }
    }

    pub fn vote_state_size_of(is_current: bool) -> usize {
        if is_current {
            VoteState::size_of()
        } else {
            VoteState1_14_11::size_of()
        }
    }

    pub fn is_correct_size_and_initialized(data: &[u8]) -> bool {
        VoteState::is_correct_size_and_initialized(data)
            || VoteState1_14_11::is_correct_size_and_initialized(data)
    }
}
