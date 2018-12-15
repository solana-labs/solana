//! Vote program
//! Receive and processes votes from validators

use crate::native_program::ProgramError;
use crate::pubkey::Pubkey;
use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian};
use std::collections::VecDeque;
use std::mem;

pub const VOTE_PROGRAM_ID: [u8; 32] = [
    132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == VOTE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&VOTE_PROGRAM_ID)
}

// Maximum number of votes to keep around
pub const MAX_VOTE_HISTORY: usize = 32;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height tick_height
    pub tick_height: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Register a new "vote account" to represent a particular validator in the Vote Contract,
    /// and initialize the VoteState for this "vote account"
    /// * Transaction::keys[0] - the validator id
    /// * Transaction::keys[1] - the new "vote account" to be associated with the validator
    /// identified by keys[0] for voting
    RegisterAccount,
    NewVote(Vote),
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteProgram {
    pub votes: VecDeque<Vote>,
    pub node_id: Pubkey,
}

pub fn get_max_size() -> usize {
    // Upper limit on the size of the Vote State. Equal to
    // sizeof(VoteProgram) + MAX_VOTE_HISTORY * sizeof(Vote) +
    // 32 (the size of the Pubkey) + 2 (2 bytes for the size)
    mem::size_of::<VoteProgram>()
        + MAX_VOTE_HISTORY * mem::size_of::<Vote>()
        + mem::size_of::<Pubkey>()
        + mem::size_of::<u16>()
}

impl VoteProgram {
    pub fn deserialize(input: &[u8]) -> Result<VoteProgram, ProgramError> {
        let len = LittleEndian::read_u16(&input[0..2]) as usize;

        if len == 0 || input.len() < len + 2 {
            Err(ProgramError::InvalidUserdata)
        } else {
            deserialize(&input[2..=len + 1]).map_err(|_| ProgramError::InvalidUserdata)
        }
    }

    pub fn serialize(self: &VoteProgram, output: &mut [u8]) -> Result<(), ProgramError> {
        let self_serialized = serialize(self).unwrap();

        if output.len() + 2 < self_serialized.len() {
            return Err(ProgramError::UserdataTooSmall);
        }

        let serialized_len = self_serialized.len() as u16;
        LittleEndian::write_u16(&mut output[0..2], serialized_len);
        output[2..=serialized_len as usize + 1].clone_from_slice(&self_serialized);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde() {
        let mut buffer: Vec<u8> = vec![0; get_max_size()];
        let mut vote_program = VoteProgram::default();
        vote_program.votes = (0..MAX_VOTE_HISTORY).map(|_| Vote::default()).collect();
        vote_program.serialize(&mut buffer).unwrap();
        assert_eq!(VoteProgram::deserialize(&buffer).unwrap(), vote_program);
    }
}
