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
use solana_sdk::syscall;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// takes a node_pubkey and commission
    InitializeAccount(Pubkey, u8),

    /// Authorize a voter to send signed votes.
    AuthorizeVoter(Pubkey),

    /// A Vote instruction with recent votes
    Vote(Vec<Vote>),
}

fn initialize_account(vote_pubkey: &Pubkey, node_pubkey: &Pubkey, commission: u8) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_pubkey, false)];
    Instruction::new(
        id(),
        &VoteInstruction::InitializeAccount(*node_pubkey, commission),
        account_metas,
    )
}

pub fn create_account(
    from_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix =
        system_instruction::create_account(from_pubkey, vote_pubkey, lamports, space, &id());
    let init_ix = initialize_account(vote_pubkey, node_pubkey, commission);
    vec![create_ix, init_ix]
}

fn metas_for_authorized_signer(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey, // currently authorized
    other_params: &[AccountMeta],
) -> Vec<AccountMeta> {
    let is_own_signer = authorized_voter_pubkey == vote_pubkey;

    // vote account
    let mut account_metas = vec![AccountMeta::new(*vote_pubkey, is_own_signer)];

    for meta in other_params {
        account_metas.push(meta.clone());
    }

    // append signer at the end
    if !is_own_signer {
        account_metas.push(AccountMeta::new(*authorized_voter_pubkey, true)) // signer
    }

    account_metas
}

pub fn authorize_voter(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey, // currently authorized
    new_authorized_voter_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(vote_pubkey, authorized_voter_pubkey, &[]);

    Instruction::new(
        id(),
        &VoteInstruction::AuthorizeVoter(*new_authorized_voter_pubkey),
        account_metas,
    )
}

pub fn vote(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    recent_votes: Vec<Vote>,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(
        vote_pubkey,
        authorized_voter_pubkey,
        &[
            // request slot_hashes syscall account after vote_pubkey
            AccountMeta::new(syscall::slot_hashes::id(), false),
            // request current syscall account after that
            AccountMeta::new(syscall::current::id(), false),
        ],
    );

    Instruction::new(id(), &VoteInstruction::Vote(recent_votes), account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    if keyed_accounts.is_empty() {
        Err(InstructionError::InvalidInstructionData)?;
    }

    // 0th index is vote account
    let (me, rest) = &mut keyed_accounts.split_at_mut(1);
    let me = &mut me[0];

    // TODO: data-driven unpack and dispatch of KeyedAccounts
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount(node_pubkey, commission) => {
            vote_state::initialize_account(me, &node_pubkey, commission)
        }
        VoteInstruction::AuthorizeVoter(voter_pubkey) => {
            vote_state::authorize_voter(me, rest, &voter_pubkey)
        }
        VoteInstruction::Vote(votes) => {
            datapoint_warn!("vote-native", ("count", 1, i64));
            if rest.len() < 2 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (slot_hashes_and_current, other_signers) = rest.split_at_mut(2);

            vote_state::process_votes(
                me,
                &syscall::slot_hashes::from_keyed_account(&slot_hashes_and_current[0])?,
                &syscall::current::from_keyed_account(&slot_hashes_and_current[1])?,
                other_signers,
                &votes,
            )
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
            super::process_instruction(&Pubkey::default(), &mut [], &[],),
            Err(InstructionError::InvalidInstructionData),
        );
    }

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                if syscall::current::check_id(&meta.pubkey) {
                    syscall::current::create_account(1, 0, 0, 0)
                } else if syscall::slot_hashes::check_id(&meta.pubkey) {
                    syscall::slot_hashes::create_account(1, &[])
                } else {
                    Account::default()
                }
            })
            .collect();

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
            super::process_instruction(&Pubkey::default(), &mut keyed_accounts, &instruction.data)
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
                vec![Vote::default()]
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize_voter(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

}
