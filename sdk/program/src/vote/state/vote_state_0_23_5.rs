#![allow(clippy::arithmetic_side_effects)]
use super::*;
#[cfg(test)]
use arbitrary::{Arbitrary, Unstructured};

const MAX_ITEMS: usize = 32;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct VoteState0_23_5 {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for vote transactions
    pub authorized_voter: Pubkey,
    /// when the authorized voter was set/initialized
    pub authorized_voter_epoch: Epoch,

    /// history of prior authorized voters and the epoch ranges for which
    ///  they were set
    pub prior_voters: CircBuf<(Pubkey, Epoch, Epoch, Slot)>,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<Lockout>,
    pub root_slot: Option<u64>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    pub epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct CircBuf<I> {
    pub buf: [I; MAX_ITEMS],
    /// next pointer
    pub idx: usize,
}

impl<I: Default + Copy> Default for CircBuf<I> {
    fn default() -> Self {
        Self {
            buf: [I::default(); MAX_ITEMS],
            idx: MAX_ITEMS - 1,
        }
    }
}

impl<I> CircBuf<I> {
    pub fn append(&mut self, item: I) {
        // remember prior delegate and when we switched, to support later slashing
        self.idx += 1;
        self.idx %= MAX_ITEMS;

        self.buf[self.idx] = item;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_deserialize_0_23_5() {
        // base case
        let target_vote_state = VoteState0_23_5::default();
        let target_vote_state_versions = VoteStateVersions::V0_23_5(Box::new(target_vote_state));
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
        let struct_bytes_x4 = std::mem::size_of::<VoteState0_23_5>() * 4;
        for _ in 0..100 {
            let raw_data: Vec<u8> = (0..struct_bytes_x4).map(|_| rand::random::<u8>()).collect();
            let mut unstructured = Unstructured::new(&raw_data);

            let arbitrary_vote_state = VoteState0_23_5::arbitrary(&mut unstructured).unwrap();
            let target_vote_state_versions =
                VoteStateVersions::V0_23_5(Box::new(arbitrary_vote_state));

            let vote_state_buf = bincode::serialize(&target_vote_state_versions).unwrap();
            let target_vote_state = target_vote_state_versions.convert_to_current();

            let mut test_vote_state = MaybeUninit::uninit();
            VoteState::deserialize_into_uninit(&vote_state_buf, &mut test_vote_state).unwrap();
            let test_vote_state = unsafe { test_vote_state.assume_init() };

            assert_eq!(target_vote_state, test_vote_state);
        }
    }
}
