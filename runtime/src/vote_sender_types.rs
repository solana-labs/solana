use {
    crossbeam_channel::{Receiver, Sender},
    solana_vote_program::vote_transaction::ParsedVote,
};

pub type ReplayVoteSender = Sender<ParsedVote>;
pub type ReplayVoteReceiver = Receiver<ParsedVote>;
