//! The `rewards_transaction` module provides functionality for creating a global
//! rewards account and enabling stakers to redeem credits from their vote accounts.

use crate::rewards_instruction::RewardsInstruction;
use crate::rewards_program;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::{Instruction, Transaction};
use solana_sdk::vote_program::{self, VoteInstruction};

pub struct RewardsTransaction {}

impl RewardsTransaction {
    pub fn new_account(
        from_keypair: &Keypair,
        rewards_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Transaction {
        SystemTransaction::new_program_account(
            from_keypair,
            rewards_id,
            last_id,
            num_tokens,
            rewards_program::get_max_size() as u64,
            rewards_program::id(),
            fee,
        )
    }

    pub fn new_redeem_credits(
        vote_keypair: &Keypair,
        rewards_id: Pubkey,
        last_id: Hash,
        fee: u64,
    ) -> Transaction {
        Transaction::new_with_instructions(
            &[vote_keypair],
            &[rewards_id],
            last_id,
            fee,
            vec![rewards_program::id(), vote_program::id()],
            vec![
                Instruction::new(0, &RewardsInstruction::RedeemVoteCredits, vec![0, 1]),
                Instruction::new(1, &VoteInstruction::ClearCredits, vec![0]),
            ],
        )
    }
}
