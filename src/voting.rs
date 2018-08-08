use entry::Entry;
use signature::PublicKey;
use transaction::{Instruction, LastId, Transaction, Vote};

pub fn entries_to_votes(entries: &[Entry]) -> Vec<(PublicKey, Vote, LastId)> {
    entries
        .iter()
        .flat_map(|entry| entry.transactions.iter().filter_map(transaction_to_vote))
        .collect()
}

pub fn transaction_to_vote(tx: &Transaction) -> Option<(PublicKey, Vote, LastId)> {
    match tx.instruction {
        Instruction::NewVote(ref vote) => Some((tx.from, vote.clone(), tx.last_id.clone())),
        _ => None,
    }
}
