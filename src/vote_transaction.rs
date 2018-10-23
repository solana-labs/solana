//! The `vote_transaction` module provides functionality for creating vote transactions.

use bincode::{deserialize, serialize};
use hash::Hash;
use signature::Keypair;
use solana_sdk::pubkey::Pubkey;
use system_transaction::SystemTransaction;
use transaction::Transaction;
use vote_program::{Vote, VoteInstruction, VoteProgram, MAX_STATE_SIZE};

pub trait VoteTransaction {
    fn vote_new(vote_account: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self;
    fn vote_account_new(
        validator_id: &Keypair,
        new_vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: i64,
    ) -> Self;
    fn vote_account_register(
        validator_id: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        fee: i64,
    ) -> Self;
    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl VoteTransaction for Transaction {
    fn vote_new(vote_account: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        let instruction = VoteInstruction::NewVote(vote);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Transaction::new(vote_account, &[], VoteProgram::id(), userdata, last_id, fee)
    }

    fn vote_account_new(
        validator_id: &Keypair,
        new_vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: i64,
    ) -> Self {
        Transaction::system_create(
            validator_id,
            new_vote_account_id,
            last_id,
            num_tokens,
            MAX_STATE_SIZE as u64,
            VoteProgram::id(),
            0,
        )
    }

    fn vote_account_register(
        validator_id: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let register_tx = VoteInstruction::RegisterAccount;
        let userdata = serialize(&register_tx).unwrap();
        Transaction::new(
            validator_id,
            &[vote_account_id],
            VoteProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        let mut votes = vec![];
        for i in 0..self.instructions.len() {
            let tx_program_id = self.program_id(i);
            if VoteProgram::check_id(&tx_program_id) {
                if let Ok(Some(VoteInstruction::NewVote(vote))) = deserialize(&self.userdata(i)) {
                    votes.push((self.account_keys[0], vote, self.last_id))
                }
            }
        }
        votes
    }
}

#[cfg(test)]
mod tests {}
