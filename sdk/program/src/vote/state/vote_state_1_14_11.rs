use super::*;
#[cfg(test)]
use arbitrary::Arbitrary;

// Offset used for VoteState version 1_14_11
const DEFAULT_PRIOR_VOTERS_OFFSET: usize = 82;

#[cfg_attr(
    feature = "frozen-abi",
    frozen_abi(digest = "CZTgLymuevXjAx6tM8X8T5J3MCx9AkEsFSmu4FJrEpkG"),
    derive(AbiExample)
)]
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(Arbitrary))]
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

impl VoteState1_14_11 {
    pub fn get_rent_exempt_reserve(rent: &Rent) -> u64 {
        rent.minimum_balance(Self::size_of())
    }

    /// Upper limit on the size of the Vote State
    /// when votes.len() is MAX_LOCKOUT_HISTORY.
    pub fn size_of() -> usize {
        3731 // see test_vote_state_size_of
    }

    pub fn is_correct_size_and_initialized(data: &[u8]) -> bool {
        const VERSION_OFFSET: usize = 4;
        const DEFAULT_PRIOR_VOTERS_END: usize = VERSION_OFFSET + DEFAULT_PRIOR_VOTERS_OFFSET;
        data.len() == VoteState1_14_11::size_of()
            && data[VERSION_OFFSET..DEFAULT_PRIOR_VOTERS_END] != [0; DEFAULT_PRIOR_VOTERS_OFFSET]
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_deserialize_1_14_11() {
        // base case
        let target_vote_state = VoteState1_14_11::default();
        let target_vote_state_versions = VoteStateVersions::V1_14_11(Box::new(target_vote_state));
        let vote_state_buf = bincode::serialize(&target_vote_state_versions).unwrap();

        let mut test_vote_state = MaybeUninit::uninit();
        VoteState::deserialize_into_uninit(&vote_state_buf, &mut test_vote_state).unwrap();
        let test_vote_state = unsafe { test_vote_state.assume_init() };

        assert_eq!(
            target_vote_state_versions.convert_to_current(),
            test_vote_state
        );

        // variant
        // provide 4x the minimum struct size in bytes to ensure we typically touch every field
        let struct_bytes_x4 = std::mem::size_of::<VoteState1_14_11>() * 4;
        for _ in 0..1000 {
            let raw_data: Vec<u8> = (0..struct_bytes_x4).map(|_| rand::random::<u8>()).collect();
            let mut unstructured = Unstructured::new(&raw_data);

            let arbitrary_vote_state = VoteState1_14_11::arbitrary(&mut unstructured).unwrap();
            let target_vote_state_versions =
                VoteStateVersions::V1_14_11(Box::new(arbitrary_vote_state));

            let vote_state_buf = bincode::serialize(&target_vote_state_versions).unwrap();
            let target_vote_state = target_vote_state_versions.convert_to_current();

            let mut test_vote_state = MaybeUninit::uninit();
            VoteState::deserialize_into_uninit(&vote_state_buf, &mut test_vote_state).unwrap();
            let test_vote_state = unsafe { test_vote_state.assume_init() };

            assert_eq!(target_vote_state, test_vote_state);
        }
    }
}
