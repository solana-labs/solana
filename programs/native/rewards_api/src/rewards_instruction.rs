use crate::rewards_program;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction_builder::BuilderInstruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum RewardsInstruction {
    RedeemVoteCredits,
}

impl RewardsInstruction {
    pub fn new_redeem_vote_credits(vote_id: Pubkey, rewards_id: Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            rewards_program::id(),
            &RewardsInstruction::RedeemVoteCredits,
            vec![(vote_id, true), (rewards_id, false)],
        )
    }
}
