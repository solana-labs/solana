//! The `rewards_transaction` module provides functionality for creating a global
//! rewards account and enabling stakers to redeem credits from their vote accounts.

use crate::rewards_instruction::RewardsInstruction;
use crate::rewards_program;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use solana_sdk::transaction_builder::TransactionBuilder;
use solana_sdk::vote_program::VoteInstruction;

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
        to_id: Pubkey,
        last_id: Hash,
        fee: u64,
    ) -> Transaction {
        let vote_id = vote_keypair.pubkey();
        TransactionBuilder::new(fee)
            .push(RewardsInstruction::new_redeem_vote_credits(
                vote_id, rewards_id, to_id,
            ))
            .push(VoteInstruction::new_clear_credits(vote_id))
            .sign(&[vote_keypair], last_id)
    }
}
