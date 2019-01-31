//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_instruction::SystemInstruction;
use crate::system_program;
use crate::transaction::{Instruction, Transaction};
use crate::vote_program::{self, Vote, VoteInstruction};
use bincode::deserialize;

pub trait VoteTransaction {
    fn vote_new<T: KeypairUtil>(
        vote_account: &T,
        tick_height: u64,
        last_id: Hash,
        fee: u64,
    ) -> Self;
    fn vote_account_new(
        validator_id: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Self;
    fn vote_delegate<T: KeypairUtil>(signer: &T, node_id: Pubkey, last_id: Hash, fee: u64) -> Self;

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl VoteTransaction for Transaction {
    /// Delegate block producing to the Fullnode with the given `node_id`
    fn vote_delegate<T: KeypairUtil>(signer: &T, node_id: Pubkey, last_id: Hash, fee: u64) -> Self {
        let ix = VoteInstruction::Delegate(node_id);
        Transaction::new(signer, &[], vote_program::id(), &ix, last_id, fee)
    }

    fn vote_new<T: KeypairUtil>(signer: &T, tick_height: u64, last_id: Hash, fee: u64) -> Self {
        let ix = VoteInstruction::Vote(Vote { tick_height });
        Transaction::new(signer, &[], vote_program::id(), &ix, last_id, fee)
    }

    fn vote_account_new(
        from_keypair: &Keypair,
        vote_account_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Self {
        let create_ix = SystemInstruction::CreateAccount {
            tokens: num_tokens,
            space: vote_program::get_max_size() as u64,
            program_id: vote_program::id(),
        };
        Transaction::new_with_instructions(
            &[from_keypair],
            &[vote_account_id],
            last_id,
            fee,
            vec![system_program::id(), vote_program::id()],
            vec![
                Instruction::new(0, &create_ix, vec![0, 1]),
                Instruction::new(1, &VoteInstruction::InitializeAccount, vec![1]),
            ],
        )
    }

    fn get_votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        let mut votes = vec![];
        for i in 0..self.instructions.len() {
            let tx_program_id = self.program_id(i);
            if vote_program::check_id(&tx_program_id) {
                if let Ok(Some(VoteInstruction::Vote(vote))) = deserialize(&self.userdata(i)) {
                    votes.push((self.account_keys[0], vote, self.last_id))
                }
            }
        }
        votes
    }
}
