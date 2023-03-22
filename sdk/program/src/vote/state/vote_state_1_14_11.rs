use super::*;

#[frozen_abi(digest = "CZTgLymuevXjAx6tM8X8T5J3MCx9AkEsFSmu4FJrEpkG")]
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone, AbiExample)]
pub struct VoteState1_14_11 {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<Lockout>,

    // This usually the last Lockout which was popped from self.votes.
    // However, it can be arbitrary slot, when being used inside Tower
    pub root_slot: Option<Slot>,

    /// the signer for vote transactions
    pub authorized_voters: AuthorizedVoters,

    /// history of prior authorized voters and the epochs for which
    /// they were set, the bottom end of the range is inclusive,
    /// the top of the range is exclusive
    pub prior_voters: CircBuf<(Pubkey, Epoch, Epoch)>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    pub epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

impl From<VoteState> for VoteState1_14_11 {
    fn from(vote_state: VoteState) -> Self {
        Self {
            node_pubkey: vote_state.node_pubkey,
            authorized_withdrawer: vote_state.authorized_withdrawer,
            commission: vote_state.commission,
            votes: vote_state
                .votes
                .into_iter()
                .map(|landed_vote| landed_vote.into())
                .collect(),
            root_slot: vote_state.root_slot,
            authorized_voters: vote_state.authorized_voters,
            prior_voters: vote_state.prior_voters,
            epoch_credits: vote_state.epoch_credits,
            last_timestamp: vote_state.last_timestamp,
        }
    }
}
