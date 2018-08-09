use entry::Entry;
use hash::Hash;
use signature::Pubkey;
use transaction::{Instruction, Transaction, Vote};

pub fn entries_to_votes(entries: &[Entry]) -> Vec<(Pubkey, Vote, Hash)> {
    entries
        .iter()
        .flat_map(|entry| entry.transactions.iter().filter_map(transaction_to_vote))
        .collect()
}

pub fn transaction_to_vote(tx: &Transaction) -> Option<(Pubkey, Vote, Hash)> {
    match tx.instruction {
        Instruction::NewVote(ref vote) => Some((tx.from, vote.clone(), tx.last_id)),
        _ => None,
    }
}
