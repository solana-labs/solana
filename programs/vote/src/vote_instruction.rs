//! Vote program
//! Receive and processes votes from validators

use crate::{
    id,
    vote_state::{self, Vote, VoteAuthorize, VoteInit, VoteState},
};
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_metrics::datapoint_debug;
use solana_sdk::{
    account::{get_signers, KeyedAccount},
    instruction::{AccountMeta, Instruction, InstructionError, WithSigner},
    instruction_processor_utils::{limited_deserialize, next_keyed_account, DecodeError},
    pubkey::Pubkey,
    system_instruction,
    sysvar::{self, clock::Clock, slot_hashes::SlotHashes, Sysvar},
};
use thiserror::Error;

/// Reasons the stake might have had an error
#[derive(Error, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum VoteError {
    #[error("vote already recorded or not in slot hashes history")]
    VoteTooOld,

    #[error("vote slots do not match bank history")]
    SlotsMismatch,

    #[error("vote hash does not match bank hash")]
    SlotHashMismatch,

    #[error("vote has no slots, invalid")]
    EmptySlots,

    #[error("vote timestamp not recent")]
    TimestampTooOld,
}
impl<E> DecodeError<E> for VoteError {
    fn type_of() -> &'static str {
        "VoteError"
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    InitializeAccount(VoteInit),

    /// Authorize a voter to send signed votes or a withdrawer
    ///  to withdraw
    Authorize(Pubkey, VoteAuthorize),

    /// A Vote instruction with recent votes
    Vote(Vote),

    /// Withdraw some amount of funds
    Withdraw(u64),

    /// Update the vote account's validator identity (node id)
    UpdateNode(Pubkey),
}

fn initialize_account(vote_pubkey: &Pubkey, vote_init: &VoteInit) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new(sysvar::rent::id(), false),
    ];
    Instruction::new(
        id(),
        &VoteInstruction::InitializeAccount(*vote_init),
        account_metas,
    )
}

pub fn create_account(
    from_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    vote_init: &VoteInit,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix =
        system_instruction::create_account(from_pubkey, vote_pubkey, lamports, space, &id());
    let init_ix = initialize_account(vote_pubkey, vote_init);
    vec![create_ix, init_ix]
}

pub fn authorize(
    vote_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey, // currently authorized
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_pubkey, false)].with_signer(authorized_pubkey);

    Instruction::new(
        id(),
        &VoteInstruction::Authorize(*new_authorized_pubkey, vote_authorize),
        account_metas,
    )
}

pub fn update_node(
    vote_pubkey: &Pubkey,       // vote account
    authorized_pubkey: &Pubkey, // currently authorized
    node_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_pubkey, false)].with_signer(authorized_pubkey);

    Instruction::new(
        id(),
        &VoteInstruction::UpdateNode(*node_pubkey),
        account_metas,
    )
}

pub fn vote(vote_pubkey: &Pubkey, authorized_voter_pubkey: &Pubkey, vote: Vote) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::slot_hashes::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ]
    .with_signer(authorized_voter_pubkey);

    Instruction::new(id(), &VoteInstruction::Vote(vote), account_metas)
}

pub fn withdraw(
    vote_pubkey: &Pubkey,
    withdrawer_pubkey: &Pubkey,
    lamports: u64,
    to_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
    ]
    .with_signer(withdrawer_pubkey);

    Instruction::new(id(), &VoteInstruction::Withdraw(lamports), account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup_with_filter("solana=info");

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    let keyed_accounts = &mut keyed_accounts.iter_mut();
    let me = &mut next_keyed_account(keyed_accounts)?;

    match limited_deserialize(data)? {
        VoteInstruction::InitializeAccount(vote_init) => {
            sysvar::rent::verify_rent_exemption(me, next_keyed_account(keyed_accounts)?)?;
            vote_state::initialize_account(me, &vote_init)
        }
        VoteInstruction::Authorize(voter_pubkey, vote_authorize) => {
            vote_state::authorize(me, &voter_pubkey, vote_authorize, &signers)
        }
        VoteInstruction::UpdateNode(node_pubkey) => {
            vote_state::update_node(me, &node_pubkey, &signers)
        }
        VoteInstruction::Vote(vote) => {
            datapoint_debug!("vote-native", ("count", 1, i64));
            vote_state::process_vote(
                me,
                &SlotHashes::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &vote,
                &signers,
            )
        }
        VoteInstruction::Withdraw(lamports) => {
            let to = next_keyed_account(keyed_accounts)?;
            vote_state::withdraw(me, lamports, to, &signers)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;
    use solana_sdk::rent::Rent;

    // these are for 100% coverage in this file
    #[test]
    fn test_vote_process_instruction_decode_bail() {
        assert_eq!(
            super::process_instruction(&Pubkey::default(), &mut [], &[],),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                if sysvar::clock::check_id(&meta.pubkey) {
                    sysvar::clock::Clock::default().create_account(1)
                } else if sysvar::slot_hashes::check_id(&meta.pubkey) {
                    sysvar::slot_hashes::create_account(1, &[])
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    Rent::free().create_account(1)
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
            &VoteInit::default(),
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
                Vote::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                VoteAuthorize::Voter,
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&update_node(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );

        assert_eq!(
            process_instruction(&withdraw(
                &Pubkey::default(),
                &Pubkey::default(),
                0,
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_minimum_balance() {
        let rent = solana_sdk::rent::Rent::default();
        let minimum_balance = rent.minimum_balance(VoteState::size_of());
        // vote state cheaper than "my $0.02" ;)
        assert!(minimum_balance as f64 / 10f64.powf(9.0) < 0.02)
    }

    #[test]
    fn test_custom_error_decode() {
        use num_traits::FromPrimitive;
        fn pretty_err<T>(err: InstructionError) -> String
        where
            T: 'static + std::error::Error + DecodeError<T> + FromPrimitive,
        {
            if let InstructionError::CustomError(code) = err {
                let specific_error: T = T::decode_custom_error_to_enum(code).unwrap();
                format!(
                    "{:?}: {}::{:?} - {}",
                    err,
                    T::type_of(),
                    specific_error,
                    specific_error,
                )
            } else {
                "".to_string()
            }
        }
        assert_eq!(
            "CustomError(0): VoteError::VoteTooOld - vote already recorded or not in slot hashes history",
            pretty_err::<VoteError>(VoteError::VoteTooOld.into())
        )
    }
}
