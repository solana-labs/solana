use crate::id;
use crate::vote_state::VoteState;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    // TODO: add signature of the state here as well
    /// A vote for height slot
    pub slot: u64,
}

impl Vote {
    pub fn new(slot: u64) -> Self {
        Self { slot }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// * Instruction::keys[0] - the new "vote account" to be associated with the delegate
    InitializeAccount,

    /// `Delegate` or `Assign` a vote account to a particular node
    DelegateStake(Pubkey),

    /// Authorize a voter to send signed votes.
    AuthorizeVoter(Pubkey),

    Vote(Vote),

    /// Clear the credits in the vote account
    /// * Transaction::keys[0] - the "vote account"
    ClearCredits,
}

fn initialize_account(vote_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, false)];
    Instruction::new(id(), &VoteInstruction::InitializeAccount, account_metas)
}

pub fn create_account(from_id: &Pubkey, staker_id: &Pubkey, lamports: u64) -> Vec<Instruction> {
    let space = VoteState::max_size() as u64;
    let create_ix = system_instruction::create_account(&from_id, staker_id, lamports, space, &id());
    let init_ix = initialize_account(staker_id);
    vec![create_ix, init_ix]
}

pub fn clear_credits(vote_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(id(), &VoteInstruction::ClearCredits, account_metas)
}

pub fn delegate_stake(vote_id: &Pubkey, delegate_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(
        id(),
        &VoteInstruction::DelegateStake(*delegate_id),
        account_metas,
    )
}

pub fn authorize_voter(vote_id: &Pubkey, authorized_voter_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(
        id(),
        &VoteInstruction::AuthorizeVoter(*authorized_voter_id),
        account_metas,
    )
}

pub fn vote(vote_id: &Pubkey, vote: Vote) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(id(), &VoteInstruction::Vote(vote), account_metas)
}
