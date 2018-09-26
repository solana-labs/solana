//! Vote program
//! Receive and processes votes from validators

use bank::Bank;
use bincode::deserialize;
use leader_scheduler::LeaderScheduler;
use solana_program_interface::pubkey::Pubkey;
use std;
use transaction::Transaction;

#[derive(Debug, PartialEq)]
pub enum Error {
    UserdataDeserializeFailure,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    /// We send some gossip specific membership information through the vote to shortcut
    /// liveness voting
    /// The version of the ClusterInfo struct that the last_id of this network voted with
    pub version: u64,
    /// The version of the ClusterInfo struct that has the same network configuration as this one
    pub contact_info_version: u64,
    // TODO: add signature of the state here as well
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteProgram {
    NewVote(Vote),
}

pub const VOTE_PROGRAM_ID: [u8; 32] = [
    6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl VoteProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == VOTE_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&VOTE_PROGRAM_ID)
    }

    pub fn process_transaction(
        tx: &Transaction,
        instruction_index: usize,
        leader_scheduler: &mut LeaderScheduler,
        tick_height: u64,
        bank: &Bank,
    ) -> Result<()> {
        match deserialize(tx.userdata(instruction_index)) {
            Ok(VoteProgram::NewVote(_vote)) => {
                trace!("GOT VOTE! last_id={}", tx.last_id);
                // Update the active set in the leader scheduler
                leader_scheduler.push_vote(*tx.from(), tick_height);
                leader_scheduler.update_height(tick_height, bank);
                Ok(())
            }
            Err(_) => {
                info!(
                    "Invalid vote transaction userdata: {:?}",
                    tx.userdata(instruction_index)
                );
                Err(Error::UserdataDeserializeFailure)
            }
        }
    }
}
