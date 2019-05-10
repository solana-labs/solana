//! Vote program
//! Receive and processes votes from validators

use crate::id;
use crate::vote_state::{self, Vote, VoteState};
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_metrics::datapoint;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// takes a node_id and commission
    InitializeAccount(Pubkey, u32),

    /// Authorize a voter to send signed votes.
    AuthorizeVoter(Pubkey),

    /// A Vote instruction with recent votes
    Vote(Vec<Vote>),
}

fn initialize_account(vote_id: &Pubkey, node_id: &Pubkey, commission: u32) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, false)];
    Instruction::new(
        id(),
        &VoteInstruction::InitializeAccount(*node_id, commission),
        account_metas,
    )
}

pub fn create_account(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    node_id: &Pubkey,
    commission: u32,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix = system_instruction::create_account(&from_id, vote_id, lamports, space, &id());
    let init_ix = initialize_account(vote_id, node_id, commission);
    vec![create_ix, init_ix]
}

pub fn authorize_voter(vote_id: &Pubkey, authorized_voter_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(
        id(),
        &VoteInstruction::AuthorizeVoter(*authorized_voter_id),
        account_metas,
    )
}

pub fn vote(vote_id: &Pubkey, recent_votes: Vec<Vote>) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(id(), &VoteInstruction::Vote(recent_votes), account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    if keyed_accounts.is_empty() {
        Err(InstructionError::InvalidInstructionData)?;
    }

    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount(node_id, commission) => {
            let vote_account = &mut keyed_accounts[0];
            vote_state::initialize_account(vote_account, &node_id, commission)
        }
        VoteInstruction::AuthorizeVoter(voter_id) => {
            let (vote_account, other_signers) = keyed_accounts.split_at_mut(1);
            let vote_account = &mut vote_account[0];

            vote_state::authorize_voter(vote_account, other_signers, &voter_id)
        }
        VoteInstruction::Vote(votes) => {
            datapoint!("vote-native", ("count", 1, i64));
            let (vote_account, other_signers) = keyed_accounts.split_at_mut(1);
            let vote_account = &mut vote_account[0];

            vote_state::process_votes(vote_account, other_signers, &votes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    // these are for 100% coverage
    #[test]
    fn test_vote_process_instruction_decode_bail() {
        assert_eq!(
            super::process_instruction(&Pubkey::default(), &mut [], &[], 0,),
            Err(InstructionError::InvalidInstructionData),
        );
    }

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts = vec![];
        for _ in 0..instruction.accounts.len() {
            accounts.push(Account::default());
        }
        {
            let mut keyed_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter_mut())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            super::process_instruction(
                &Pubkey::default(),
                &mut keyed_accounts,
                &instruction.data,
                0,
            )
        }
    }

    #[test]
    fn test_vote_process_instruction() {
        let instructions = create_account(
            &Pubkey::default(),
            &Pubkey::default(),
            &Pubkey::default(),
            0,
            100,
        );
        assert_eq!(
            process_instruction(&instructions[1]),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&vote(&Pubkey::default(), vec![Vote::default()])),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize_voter(&Pubkey::default(), &Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
    }

}
