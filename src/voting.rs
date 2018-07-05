use entry::Entry;
use hash::Hash;
use signature::PublicKey;
use transaction::{Instruction, Vote};

pub fn entries_to_votes(entries: &Vec<Entry>) -> Vec<(PublicKey, Vote, Hash)> {
    entries
        .iter()
        .flat_map(|entry| {
            let vs: Vec<(PublicKey, Vote, Hash)> = entry
                .transactions
                .iter()
                .filter_map(|tx| match &tx.instruction {
                    &Instruction::NewVote(ref vote) => {
                        Some((tx.from.clone(), vote.clone(), tx.last_id.clone()))
                    }
                    _ => None,
                })
                .collect();
            vs
        })
        .collect()
}
