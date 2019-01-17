//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::Keypair;
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::{Instruction, Transaction};
use crate::vote_program::{self, BlockDescription, Vote, VoteInstruction};
use crate::weighted_election::WeightedElection;
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
    fn vote_propose_block(
        leader_id: &Keypair,
        election_id: Pubkey,
        desc: BlockDescription,
        last_id: Hash,
        tokens: u64,
        fee: u64,
    ) -> Self;
    fn vote_ballot_new(voter_id: Pubkey, election_id: Pubkey, last_id: Hash, fee: u64) -> Self;

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl VoteTransaction for Transaction {
    fn vote_new(vote_account: &Pubkey, vote: Vote, last_id: Hash, fee: u64) -> Self {
        let instruction = VoteInstruction::NewVote(vote);
        Transaction::new_unsigned(
            vote_account,
            &[],
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

    fn vote_propose_block(
        leader_id: &Keypair,
        election_id: Pubkey,
        desc: BlockDescription,
        last_id: Hash,
        tokens: u64,
        fee: u64,
    ) -> Self {
        let program_ids = vec![system_program::id(), vote_program::id()];

        let space = WeightedElection::serialized_size(desc.weights.len()) as u64;
        let create_ix = SystemInstruction::CreateAccount {
            tokens,
            space,
            program_id: vote_program::id(),
        };
        let propose_ix = VoteInstruction::ProposeBlock(desc);
        let ixs = vec![
            Instruction::new(0, &create_ix, vec![0, 1]),
            Instruction::new(1, &propose_ix, vec![0, 1]),
        ];

        Transaction::new_with_instructions(
            &[leader_id],
            &[election_id],
            last_id,
            fee,
            program_ids,
            ixs,
        )
    }

    fn vote_ballot_new(voter_id: Pubkey, election_id: Pubkey, last_id: Hash, fee: u64) -> Self {
        Transaction::new_unsigned(
            &voter_id,
            &[election_id],
            vote_program::id(),
            &VoteInstruction::Vote,
            last_id,
            fee,
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
