use {
    crate::vote_parser::ParsedVote,
    crossbeam_channel::{Receiver, Sender},
<<<<<<< HEAD
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    solana_vote_program::vote_state::Vote,
=======
>>>>>>> 7f20c6149 (Refactor: move simple vote parsing to runtime (#22537))
};

pub type ReplayedVote = (Pubkey, Vote, Option<Hash>);
pub type ReplayVoteSender = Sender<ReplayedVote>;
pub type ReplayVoteReceiver = Receiver<ReplayedVote>;
