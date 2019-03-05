//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::vote_instruction::{Vote, VoteInstruction};
use crate::vote_state::VoteState;
use crate::{check_id, id};
use bincode::deserialize;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::Transaction;
use solana_sdk::transaction_builder::TransactionBuilder;

pub struct VoteTransaction {}

impl VoteTransaction {
    pub fn new_vote<T: KeypairUtil>(
        voting_keypair: &T,
        slot_height: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let vote = Vote { slot_height };
        TransactionBuilder::new(fee)
            .push(VoteInstruction::new_vote(voting_keypair.pubkey(), vote))
            .sign(&[voting_keypair], recent_blockhash)
    }

    /// Fund or create the staking account with tokens
    pub fn new_account(
        from_keypair: &Keypair,
        voter_id: Pubkey,
        recent_blockhash: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Transaction {
        let from_id = from_keypair.pubkey();
        let space = VoteState::max_size() as u64;
        TransactionBuilder::new(fee)
            .push(SystemInstruction::new_program_account(
                from_id,
                voter_id,
                num_tokens,
                space,
                id(),
            ))
            .push(VoteInstruction::new_initialize_account(voter_id))
            .sign(&[from_keypair], recent_blockhash)
    }

    /// Fund or create the staking account with tokens
    pub fn new_account_with_delegate(
        from_keypair: &Keypair,
        voter_keypair: &Keypair,
        delegate_id: Pubkey,
        recent_blockhash: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Transaction {
        let from_id = from_keypair.pubkey();
        let voter_id = voter_keypair.pubkey();
        let space = VoteState::max_size() as u64;
        TransactionBuilder::new(fee)
            .push(SystemInstruction::new_program_account(
                from_id,
                voter_id,
                num_tokens,
                space,
                id(),
            ))
            .push(VoteInstruction::new_initialize_account(voter_id))
            .push(VoteInstruction::new_delegate_stake(voter_id, delegate_id))
            .sign(&[from_keypair, voter_keypair], recent_blockhash)
    }

    /// Choose a node id to `delegate` or `assign` this vote account to
    pub fn delegate_vote_account<T: KeypairUtil>(
        vote_keypair: &T,
        recent_blockhash: Hash,
        node_id: Pubkey,
        fee: u64,
    ) -> Transaction {
        TransactionBuilder::new(fee)
            .push(VoteInstruction::new_delegate_stake(
                vote_keypair.pubkey(),
                node_id,
            ))
            .sign(&[vote_keypair], recent_blockhash)
    }

    fn get_vote(tx: &Transaction, ix_index: usize) -> Option<(Pubkey, Vote, Hash)> {
        if !check_id(&tx.program_id(ix_index)) {
            return None;
        }
        let instruction = deserialize(&tx.userdata(ix_index)).unwrap();
        if let VoteInstruction::Vote(vote) = instruction {
            Some((tx.account_keys[0], vote, tx.recent_blockhash))
        } else {
            None
        }
    }

    pub fn get_votes(tx: &Transaction) -> Vec<(Pubkey, Vote, Hash)> {
        (0..tx.instructions.len())
            .filter_map(|i| Self::get_vote(tx, i))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_votes() {
        let keypair = Keypair::new();
        let slot_height = 1;
        let recent_blockhash = Hash::default();
        let transaction = VoteTransaction::new_vote(&keypair, slot_height, recent_blockhash, 0);
        assert_eq!(
            VoteTransaction::get_votes(&transaction),
            vec![(keypair.pubkey(), Vote::new(slot_height), recent_blockhash)]
        );
    }
}
