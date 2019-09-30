//! Vote program
//! Receive and processes votes from validators

use crate::{
    id,
    vote_state::{self, Vote, VoteAuthorize, VoteInit, VoteState},
};
use bincode::deserialize;
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_metrics::datapoint_info;
use solana_sdk::{
    account::KeyedAccount,
    instruction::{AccountMeta, Instruction, InstructionError},
    instruction_processor_utils::DecodeError,
    pubkey::Pubkey,
    system_instruction, sysvar,
};

/// Reasons the stake might have had an error
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum VoteError {
    VoteTooOld,
    SlotsMismatch,
    SlotHashMismatch,
    EmptySlots,
}
impl<E> DecodeError<E> for VoteError {
    fn type_of() -> &'static str {
        "VoteError"
    }
}

impl std::fmt::Display for VoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                VoteError::VoteTooOld => "vote already recorded or not in slot hashes history",
                VoteError::SlotsMismatch => "vote slots do not match bank history",
                VoteError::SlotHashMismatch => "vote hash does not match bank hash",
                VoteError::EmptySlots => "vote has no slots, invalid",
            }
        )
    }
}
impl std::error::Error for VoteError {}

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
}

fn initialize_account(vote_pubkey: &Pubkey, vote_init: &VoteInit) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_pubkey, false)];
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

// for instructions that whose authorized signer may differ from the account's pubkey
fn metas_for_authorized_signer(
    account_pubkey: &Pubkey,
    authorized_signer: &Pubkey, // currently authorized
    other_params: &[AccountMeta],
) -> Vec<AccountMeta> {
    let is_own_signer = authorized_signer == account_pubkey;

    // vote account
    let mut account_metas = vec![AccountMeta::new(*account_pubkey, is_own_signer)];

    for meta in other_params {
        account_metas.push(meta.clone());
    }

    // append signer at the end
    if !is_own_signer {
        account_metas.push(AccountMeta::new_credit_only(*authorized_signer, true))
        // signer
    }

    account_metas
}

pub fn authorize(
    vote_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey, // currently authorized
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(vote_pubkey, authorized_pubkey, &[]);

    Instruction::new(
        id(),
        &VoteInstruction::Authorize(*new_authorized_pubkey, vote_authorize),
        account_metas,
    )
}

pub fn vote(vote_pubkey: &Pubkey, authorized_voter_pubkey: &Pubkey, vote: Vote) -> Instruction {
    let account_metas = metas_for_authorized_signer(
        vote_pubkey,
        authorized_voter_pubkey,
        &[
            // request slot_hashes sysvar account after vote_pubkey
            AccountMeta::new_credit_only(sysvar::slot_hashes::id(), false),
            // request clock sysvar account after that
            AccountMeta::new_credit_only(sysvar::clock::id(), false),
        ],
    );

    Instruction::new(id(), &VoteInstruction::Vote(vote), account_metas)
}

pub fn withdraw(
    vote_pubkey: &Pubkey,
    withdrawer_pubkey: &Pubkey,
    lamports: u64,
    to_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = metas_for_authorized_signer(
        vote_pubkey,
        withdrawer_pubkey,
        &[AccountMeta::new_credit_only(*to_pubkey, false)],
    );

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

    if keyed_accounts.is_empty() {
        Err(InstructionError::InvalidInstructionData)?;
    }

    // 0th index is vote account
    let (me, rest) = &mut keyed_accounts.split_at_mut(1);
    let me = &mut me[0];

    // TODO: data-driven unpack and dispatch of KeyedAccounts
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount(vote_init) => {
            vote_state::initialize_account(me, &vote_init)
        }
        VoteInstruction::Authorize(voter_pubkey, vote_authorize) => {
            vote_state::authorize(me, rest, &voter_pubkey, vote_authorize)
        }
        VoteInstruction::Vote(vote) => {
            datapoint_info!("vote-native", ("count", 1, i64));
            if rest.len() < 2 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (slot_hashes_and_clock, other_signers) = rest.split_at_mut(2);

            vote_state::process_vote(
                me,
                &sysvar::slot_hashes::from_keyed_account(&slot_hashes_and_clock[0])?,
                &sysvar::clock::from_keyed_account(&slot_hashes_and_clock[1])?,
                other_signers,
                &vote,
            )
        }
        VoteInstruction::Withdraw(lamports) => {
            if rest.is_empty() {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (to, rest) = rest.split_at_mut(1);
            let to = &mut to[0];

            vote_state::withdraw(me, rest, lamports, to)
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
                if sysvar::clock::check_id(&meta.pubkey) {
                    sysvar::clock::new_account(1, 0, 0, 0, 0)
                } else if sysvar::slot_hashes::check_id(&meta.pubkey) {
                    sysvar::slot_hashes::create_account(1, &[])
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
        let rent_calculator = solana_sdk::rent_calculator::RentCalculator::default();
        let minimum_balance = rent_calculator.minimum_balance(VoteState::size_of());
        // vote state cheaper than "my $0.02" ;)
        assert!(minimum_balance as f64 / 2f64.powf(34.0) < 0.02)
    }

    #[test]
    fn test_metas_for_authorized_signer() {
        let account_pubkey = Pubkey::new_rand();
        let authorized_signer = Pubkey::new_rand();

        assert_eq!(
            metas_for_authorized_signer(&account_pubkey, &authorized_signer, &[]).len(),
            2
        );
        assert_eq!(
            metas_for_authorized_signer(&account_pubkey, &account_pubkey, &[]).len(),
            1
        );
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
