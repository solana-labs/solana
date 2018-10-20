//! Vote program
//! Receive and processes votes from validators

use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std;
use std::collections::VecDeque;
use transaction::Transaction;

// Upper limit on the size of the Vote State
pub const MAX_STATE_SIZE: usize = 1024;

// Maximum number of votes to keep around
const MAX_VOTE_HISTORY: usize = 32;

#[derive(Debug, PartialEq)]
pub enum Error {
    UserdataDeserializeFailure,
    InvalidArguments,
    InvalidUserdata,
    UserdataTooSmall,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height tick_height
    pub tick_height: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    NewVote(Vote),
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VoteProgram {
    pub votes: VecDeque<Vote>,
    pub node_id: Pubkey,
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

    pub fn deserialize(input: &[u8]) -> Result<VoteProgram> {
        let len = LittleEndian::read_u16(&input[0..2]) as usize;

        if len == 0 {
            Ok(VoteProgram::default())
        } else if input.len() < len + 1 {
            Err(Error::InvalidUserdata)
        } else {
            deserialize(&input[2..=len + 1]).map_err(|err| {
                error!("Unable to deserialize vote state: {:?}", err);
                Error::InvalidUserdata
            })
        }
    }

    pub fn serialize(self: &VoteProgram, output: &mut [u8]) -> Result<()> {
        let self_serialized = serialize(self).unwrap();

        if output.len() + 2 < self_serialized.len() {
            warn!(
                "{} bytes required to serialize but only have {} bytes",
                self_serialized.len(),
                output.len() + 2,
            );
            return Err(Error::UserdataTooSmall);
        }

        let serialized_len = self_serialized.len() as u16;
        LittleEndian::write_u16(&mut output[0..2], serialized_len);
        output[2..=serialized_len as usize + 1].clone_from_slice(&self_serialized);
        Ok(())
    }

    pub fn process_transaction(
        tx: &Transaction,
        instruction_index: usize,
        accounts: &mut [&mut Account],
    ) -> Result<()> {
        if !Self::check_id(&accounts[1].program_id) {
            error!("accounts[1] is not assigned to the VOTE_PROGRAM");
            Err(Error::InvalidArguments)?;
        }

        let mut vote_state = Self::deserialize(&accounts[1].userdata)?;

        match deserialize(tx.userdata(instruction_index)) {
            Ok(VoteInstruction::NewVote(vote)) => {
                if vote_state.node_id == Pubkey::default() {
                    // If it's a new account, setup this account for voting
                    vote_state.node_id = *tx.from();
                    vote_state.votes = VecDeque::new();
                } else if vote_state.node_id != *tx.from() {
                    // If it's an old account, verify the state's node id and
                    // the sender's id match (ensures nodes can't vote for other nodes)
                    Err(Error::InvalidArguments)?;
                }

                // TODO: Integrity checks
                // a) Verify the vote's bank hash matches what is expected
                // b) Verify vote is older than previous votes

                // Only keep around the most recent MAX_VOTE_HISTORY votes
                if vote_state.votes.len() == MAX_VOTE_HISTORY {
                    vote_state.votes.pop_front();
                }

                vote_state.votes.push_back(vote);
                vote_state.serialize(&mut accounts[1].userdata)?;

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
