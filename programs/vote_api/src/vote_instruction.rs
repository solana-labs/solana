//! Vote program
//! Receive and processes votes from validators

use crate::id;
use crate::vote_state::{self, Vote, VoteState};
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_metrics::datapoint_warn;
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

fn initialize_account(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    node_id: &Pubkey,
    commission: u32,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_id, true),
        AccountMeta::new(*vote_id, false),
    ];
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
    let create_ix = system_instruction::create_account(from_id, vote_id, lamports, space, &id());
    let init_ix = initialize_account(from_id, vote_id, node_id, commission);
    vec![create_ix, init_ix]
}

fn metas_for_authorized_signer(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    authorized_voter_id: &Pubkey, // currently authorized
) -> Vec<AccountMeta> {
    let mut account_metas = vec![AccountMeta::new(*from_id, true)]; // sender

    let is_own_signer = authorized_voter_id == vote_id;

    account_metas.push(AccountMeta::new(*vote_id, is_own_signer)); // vote account

    if !is_own_signer {
        account_metas.push(AccountMeta::new(*authorized_voter_id, true)) // signer
    }
    account_metas
}

pub fn authorize_voter(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    authorized_voter_id: &Pubkey, // currently authorized
    new_authorized_voter_id: &Pubkey,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(from_id, vote_id, authorized_voter_id);

    Instruction::new(
        id(),
        &VoteInstruction::AuthorizeVoter(*new_authorized_voter_id),
        account_metas,
    )
}

pub fn vote(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    authorized_voter_id: &Pubkey,
    recent_votes: Vec<Vote>,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(from_id, vote_id, authorized_voter_id);

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

    if keyed_accounts.len() < 2 {
        Err(InstructionError::InvalidInstructionData)?;
    }

    // 0th index is the guy who paid for the transaction
    let (me, other_signers) = &mut keyed_accounts.split_at_mut(2);
    let me = &mut me[1];

    // TODO: data-driven unpack and dispatch of KeyedAccounts
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount(node_id, commission) => {
            vote_state::initialize_account(me, &node_id, commission)
        }
        VoteInstruction::AuthorizeVoter(voter_id) => {
            vote_state::authorize_voter(me, other_signers, &voter_id)
        }
        VoteInstruction::Vote(votes) => {
            datapoint_warn!("vote-native", ("count", 1, i64));
            vote_state::process_votes(me, other_signers, &votes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    // these are for 100% coverage in this file
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
            process_instruction(&vote(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                vec![Vote::default()]
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize_voter(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

}
