use crate::id;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::{AccountMeta, Instruction};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum RewardsInstruction {
    RedeemVoteCredits,
}

impl RewardsInstruction {
    pub fn new_redeem_vote_credits(vote_id: &Pubkey, rewards_id: &Pubkey) -> Instruction {
        let account_metas = vec![
            AccountMeta::new(*vote_id, true),
            AccountMeta::new(*rewards_id, false),
        ];
        Instruction::new(id(), &RewardsInstruction::RedeemVoteCredits, account_metas)
    }
}
