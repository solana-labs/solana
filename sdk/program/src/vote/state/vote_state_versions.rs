use super::{vote_state_0_23_5::VoteState0_23_5, vote_state_1_10_40::VoteState1_10_40, *};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum VoteStateVersions {
    V0_23_5(Box<VoteState0_23_5>),
    V1_10_40(Box<VoteState1_10_40>),
    Current(Box<VoteState>),
}

impl VoteStateVersions {
    pub fn new(vote_state: VoteState, use_current: bool) -> Self {
        // The version of vote state to use depends on the timely vote credits feature.  When this feature is enabled,
        // it introduces the use of new fields, and so the vote state must include values for those new fields.
        // Before this feature is enabled, the older version of vote state can be used since the fields in question
        // have sensible default values and are not used until the feature is enabled anyway.  Allowing the older
        // version to continue to be used until the feature is enabled will prevent incompatibilities in snapshot
        // account loading.
        if use_current {
            Self::Current(Box::new(vote_state))
        } else {
            // Create a downgraded version of the vote state
            Self::V1_10_40(Box::new(VoteState1_10_40 {
                node_pubkey: vote_state.node_pubkey,
                authorized_withdrawer: vote_state.authorized_withdrawer,
                commission: vote_state.commission,
                votes: vote_state
                    .votes
                    .iter()
                    .map(|landed_vote| landed_vote.lockout)
                    .collect(),
                root_slot: vote_state.root_slot,
                authorized_voters: vote_state.authorized_voters,
                prior_voters: vote_state_1_10_40::CircBuf::<(Pubkey, Epoch, Epoch)> {
                    buf: vote_state.prior_voters.buf,
                    idx: vote_state.prior_voters.idx,
                    is_empty: vote_state.prior_voters.is_empty,
                },
                epoch_credits: vote_state.epoch_credits,
                last_timestamp: vote_state.last_timestamp,
            }))
        }
    }

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

                    votes: state
                        .votes
                        .iter()
                        .map(|lockout| LandedVote {
                            latency: 0,
                            lockout: lockout.clone(),
                        })
                        .collect(),

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

            VoteStateVersions::V1_10_40(state) => {
                // Current version after v1.10.40 converted votes from Lockouts to LandedVotes, all other fields are
                // unchanged
                VoteState {
                    node_pubkey: state.node_pubkey,
                    authorized_withdrawer: state.authorized_withdrawer,
                    commission: state.commission,
                    /// When converting from an older version, 0 latency
                    /// is stored here, which the voting code will understand as "latency
                    /// not provided"
                    votes: state
                        .votes
                        .iter()
                        .map(|lockout| LandedVote {
                            latency: 0,
                            lockout: lockout.clone(),
                        })
                        .collect(),
                    root_slot: state.root_slot,
                    authorized_voters: state.authorized_voters.clone(),
                    prior_voters: CircBuf::<(Pubkey, Epoch, Epoch)>::new_from_1_10_40(
                        state.prior_voters,
                    ),
                    epoch_credits: state.epoch_credits.clone(),
                    last_timestamp: state.last_timestamp.clone(),
                }
            }

            VoteStateVersions::Current(state) => *state,
        }
    }

    pub fn is_uninitialized(&self) -> bool {
        match self {
            VoteStateVersions::V0_23_5(vote_state) => {
                vote_state.authorized_voter == Pubkey::default()
            }

            VoteStateVersions::V1_10_40(vote_state) => vote_state.authorized_voters.is_empty(),

            VoteStateVersions::Current(vote_state) => vote_state.authorized_voters.is_empty(),
        }
    }
}
