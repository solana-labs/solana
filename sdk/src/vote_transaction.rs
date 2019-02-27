//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::Transaction;
use crate::vote_program::{self, Vote, VoteInstruction};
use bincode::deserialize;

pub struct VoteTransaction {}

impl VoteTransaction {
    pub fn new_vote<T: KeypairUtil>(
        voting_keypair: &T,
        slot_height: u64,
        last_id: Hash,
        fee: u64,
    ) -> Transaction {
        let vote = Vote { slot_height };
        let instruction = VoteInstruction::Vote(vote);
        Transaction::new(
            voting_keypair,
            &[],
            vote_program::id(),
            &instruction,
            last_id,
            fee,
        )
    }

    pub fn fund_vote_account(
        from_keypair: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Transaction {
        let create_tx = SystemInstruction::CreateAccount {
            tokens: num_tokens,
            space: vote_program::get_max_size() as u64,
            program_id: vote_program::id(),
        };
        Transaction::new(
            from_keypair,
            &[vote_account_id],
            system_program::id(),
            &create_tx,
            last_id,
            fee,
        )
    }

    pub fn register_vote_account<T: KeypairUtil>(
        vote_keypair: &T,
        last_id: Hash,
        node_id: Pubkey,
        fee: u64,
    ) -> Transaction {
        let instruction = VoteInstruction::RegisterAccount(node_id);
        Transaction::new(
            vote_keypair,
            &[],
            vote_program::id(),
            &instruction,
            last_id,
            fee,
        )
    }

    fn get_vote(tx: &Transaction, ix_index: usize) -> Option<(Pubkey, Vote, Hash)> {
        if !vote_program::check_id(&tx.program_id(ix_index)) {
            return None;
        }
        let instruction = deserialize(&tx.userdata(ix_index)).unwrap();
        if let VoteInstruction::Vote(vote) = instruction {
            Some((tx.account_keys[0], vote, tx.last_id))
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
        let last_id = Hash::default();
        let transaction = VoteTransaction::new_vote(&keypair, slot_height, last_id, 0);
        assert_eq!(
            VoteTransaction::get_votes(&transaction),
            vec![(keypair.pubkey(), Vote::new(slot_height), last_id)]
        );
    }
}
