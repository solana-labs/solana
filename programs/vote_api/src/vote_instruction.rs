use crate::id;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction_builder::BuilderInstruction;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height slot_height
    pub slot_height: u64,
}

impl Vote {
    pub fn new(slot_height: u64) -> Self {
        Self { slot_height }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// * Transaction::keys[0] - the staker id that is also assumed to be the delegate by default
    /// * Transaction::keys[1] - the new "vote account" to be associated with the delegate
    InitializeAccount,
    /// `Delegate` or `Assign` a vote account to a particular node
    DelegateStake(Pubkey),
    Vote(Vote),
    /// Clear the credits in the vote account
    /// * Transaction::keys[0] - the "vote account"
    ClearCredits,
}

impl VoteInstruction {
    pub fn new_clear_credits(vote_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(id(), &VoteInstruction::ClearCredits, vec![(vote_id, true)])
    }
    pub fn new_delegate_stake(vote_id: Pubkey, delegate_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &VoteInstruction::DelegateStake(delegate_id),
            vec![(vote_id, true)],
        )
    }
    pub fn new_initialize_account(vote_id: Pubkey, staker_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &VoteInstruction::InitializeAccount,
            vec![(staker_id, true), (vote_id, false)],
        )
    }
    pub fn new_vote(vote_id: Pubkey, vote: Vote) -> BuilderInstruction {
        BuilderInstruction::new(id(), &VoteInstruction::Vote(vote), vec![(vote_id, true)])
    }
}
