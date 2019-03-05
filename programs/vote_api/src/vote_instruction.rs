use crate::id;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction_builder::BuilderInstruction;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height slot
    pub slot: u64,
}

impl Vote {
    pub fn new(slot: u64) -> Self {
        Self { slot }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// * Instruction::keys[0] - the new "vote account" to be associated with the delegate
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
    pub fn new_initialize_account(vote_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &VoteInstruction::InitializeAccount,
            vec![(vote_id, false)],
        )
    }
    pub fn new_vote(vote_id: Pubkey, vote: Vote) -> BuilderInstruction {
        BuilderInstruction::new(id(), &VoteInstruction::Vote(vote), vec![(vote_id, true)])
    }
}
