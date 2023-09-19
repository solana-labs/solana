use {
    crate::vote_parser::ParsedVote,
    crossbeam_channel::{Receiver, Sender},
};

pub type ReplayVoteSender = Sender<ParsedVote>;
pub type ReplayVoteReceiver = Receiver<ParsedVote>;
