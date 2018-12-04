//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::Keypair;
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::{Instruction, Transaction};
use crate::vote_program::{self, Vote, VoteInstruction};
use bincode::deserialize;

pub trait VoteTransaction {
    fn vote_new(vote_account: &Pubkey, vote: Vote, last_id: Hash, fee: u64) -> Self;
    fn vote_account_new(
        validator_id: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Self;

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl VoteTransaction for Transaction {
    fn vote_new(vote_account: &Pubkey, vote: Vote, last_id: Hash, fee: u64) -> Self {
        let instruction = VoteInstruction::NewVote(vote);
        Transaction::new_unsigned(
            &[vote_account.clone()],
            vote_program::id(),
            &instruction,
            last_id,
            fee,
        )
    }

    fn vote_account_new(
        validator_id: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Self {
        Transaction::new_with_instructions(
            &[validator_id],
            &[vote_account_id],
            last_id,
            fee,
            vec![system_program::id(), vote_program::id()],
            vec![
                Instruction::new(
                    0,
                    &SystemInstruction::CreateAccount {
                        tokens: num_tokens,
                        space: vote_program::get_max_size() as u64,
                        program_id: vote_program::id(),
                    },
                    vec![0, 1],
                ),
                Instruction::new(1, &VoteInstruction::RegisterAccount, vec![0, 1]),
            ],
        )
    }

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        let mut votes = vec![];
        for i in 0..self.instructions.len() {
            let tx_program_id = self.program_id(i);
            if vote_program::check_id(&tx_program_id) {
                if let Ok(Some(VoteInstruction::NewVote(vote))) = deserialize(&self.userdata(i)) {
                    votes.push((self.account_keys[0], vote, self.last_id))
                }
            }
        }
        votes
    }
}
