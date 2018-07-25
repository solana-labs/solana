use entry::Entry;
use hash::Hash;
use signature::PublicKey;
use transaction::{Instruction, Transaction, Vote};

pub fn entries_to_votes(entries: &[Entry]) -> Vec<(PublicKey, Vote, Hash)> {
    entries
        .iter()
        .flat_map(|entry| {
            entry
                .transactions
                .iter()
                .flat_map(|tx| transaction_to_votes(tx))
        })
        .collect()
}

fn transaction_to_votes(tx: &Transaction) -> Vec<(PublicKey, Vote, Hash)> {
    tx.instructions
        .iter()
        .filter_map(|i| match *i {
            Instruction::NewVote(ref vote) => Some((tx.from, vote.clone(), tx.last_id)),
            _ => None,
        })
        .collect()
}
