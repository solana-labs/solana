use entry::Entry;
use hash::Hash;
use signature::PublicKey;
use transaction::{Instruction, Vote};

pub fn entries_to_votes(entries: &[Entry]) -> Vec<(PublicKey, Vote, Hash)> {
    entries
        .iter()
        .flat_map(|entry| {
            let vs: Vec<(PublicKey, Vote, Hash)> = entry
                .transactions
                .iter()
                .filter_map(|tx| match tx.instruction {
                    Instruction::NewVote(ref vote) => Some((tx.from, vote.clone(), tx.last_id)),
                    _ => None,
                })
                .collect();
            vs
        })
        .collect()
}
