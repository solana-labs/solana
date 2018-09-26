//! The `vote_transaction` module provides functionality for creating vote transactions.

use bincode::{deserialize, serialize};
use hash::Hash;
use signature::Keypair;
use solana_program_interface::pubkey::Pubkey;
use transaction::Transaction;
use vote_program::{Vote, VoteProgram};

pub trait VoteTransaction {
    fn vote_new(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self;
    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl VoteTransaction for Transaction {
    /// Create and sign new SystemProgram::CreateAccount transaction
    fn vote_new(from: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        let instruction = VoteProgram::NewVote(vote);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Transaction::new(from, &[], VoteProgram::id(), userdata, last_id, fee)
    }

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        let mut votes = vec![];
        for i in 0..self.instructions.len() {
            let tx_program_id = self.program_id(i);
            if VoteProgram::check_id(&tx_program_id) {
                if let Ok(Some(VoteProgram::NewVote(vote))) = deserialize(&self.userdata(i)) {
                    votes.push((self.account_keys[0], vote, self.last_id))
                }
            }
        }
        votes
    }
}

#[cfg(test)]
mod tests {}
