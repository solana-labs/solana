use {
    crossbeam_channel::{Receiver, Sender},
    solana_vote::vote_parser::ParsedVote,
};

pub type ReplayVoteSender = Sender<ParsedVote>;
pub type ReplayVoteReceiver = Receiver<ParsedVote>;
