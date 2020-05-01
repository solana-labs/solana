use crate::{id, vest_state::VestState};
use bincode::serialized_size;
use chrono::prelude::{Date, DateTime, Utc};
use num_derive::FromPrimitive;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction, InstructionError},
    program_utils::DecodeError,
    pubkey::Pubkey,
    system_instruction,
};
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, FromPrimitive)]
pub enum VestError {
    #[error("destination missing")]
    DestinationMissing,

    #[error("unauthorized")]
    Unauthorized,
}

impl From<VestError> for InstructionError {
    fn from(e: VestError) -> Self {
        InstructionError::Custom(e as u32)
    }
}

impl<T> DecodeError<T> for VestError {
    fn type_of() -> &'static str {
        "VestError"
    }
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VestInstruction {
    /// Declare and instantiate a vesting schedule
    InitializeAccount {
        terminator_pubkey: Pubkey, // The address authorized to terminate this contract with a signed Terminate instruction
        payee_pubkey: Pubkey,      // The address authorized to redeem vested tokens
        start_date_time: DateTime<Utc>, // The day from which the vesting contract begins
        date_pubkey: Pubkey, // Address of an account containing a trusted date, used to drive the vesting schedule
        total_lamports: u64, // The number of lamports to send the payee if the schedule completes
    },

    /// Change the terminator pubkey
    SetTerminator(Pubkey),

    /// Change the payee pubkey
    SetPayee(Pubkey),

    /// Load an account and pass its data to the contract for inspection.
    RedeemTokens,

    /// Tell the contract that the `InitializeAccount` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    Terminate,

    /// Reduce total_lamports by the given number of lamports. Tokens that have
    /// already vested are unaffected. Use this instead of `Terminate` to minimize
    /// the number of token transfers.
    Renege(u64),

    /// Mark all available tokens as redeemable, regardless of the date.
    VestAll,
}

fn initialize_account(
    terminator_pubkey: &Pubkey,
    payee_pubkey: &Pubkey,
    contract_pubkey: &Pubkey,
    start_date: Date<Utc>,
    date_pubkey: &Pubkey,
    total_lamports: u64,
) -> Instruction {
    let keys = vec![AccountMeta::new(*contract_pubkey, false)];
    Instruction::new(
        id(),
        &VestInstruction::InitializeAccount {
            terminator_pubkey: *terminator_pubkey,
            payee_pubkey: *payee_pubkey,
            start_date_time: start_date.and_hms(0, 0, 0),
            date_pubkey: *date_pubkey,
            total_lamports,
        },
        keys,
    )
}

pub fn create_account(
    payer_pubkey: &Pubkey,
    terminator_pubkey: &Pubkey,
    contract_pubkey: &Pubkey,
    payee_pubkey: &Pubkey,
    start_date: Date<Utc>,
    date_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    let space = serialized_size(&VestState::default()).unwrap();
    vec![
        system_instruction::create_account(&payer_pubkey, contract_pubkey, lamports, space, &id()),
        initialize_account(
            terminator_pubkey,
            payee_pubkey,
            contract_pubkey,
            start_date,
            date_pubkey,
            lamports,
        ),
    ]
}

pub fn set_terminator(contract: &Pubkey, old_pubkey: &Pubkey, new_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new(*old_pubkey, true),
    ];
    Instruction::new(
        id(),
        &VestInstruction::SetTerminator(*new_pubkey),
        account_metas,
    )
}

pub fn set_payee(contract: &Pubkey, old_pubkey: &Pubkey, new_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new(*old_pubkey, true),
    ];
    Instruction::new(id(), &VestInstruction::SetPayee(*new_pubkey), account_metas)
}

pub fn redeem_tokens(contract: &Pubkey, date_pubkey: &Pubkey, to: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new_readonly(*date_pubkey, false),
        AccountMeta::new(*to, false),
    ];
    Instruction::new(id(), &VestInstruction::RedeemTokens, account_metas)
}

pub fn terminate(contract: &Pubkey, from: &Pubkey, to: &Pubkey) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new(*from, true),
    ];
    if from != to {
        account_metas.push(AccountMeta::new(*to, false));
    }
    Instruction::new(id(), &VestInstruction::Terminate, account_metas)
}

pub fn renege(contract: &Pubkey, from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new(*from, true),
    ];
    if from != to {
        account_metas.push(AccountMeta::new(*to, false));
    }
    Instruction::new(id(), &VestInstruction::Renege(lamports), account_metas)
}

pub fn vest_all(contract: &Pubkey, from: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*contract, false),
        AccountMeta::new(*from, true),
    ];
    Instruction::new(id(), &VestInstruction::VestAll, account_metas)
}
