use crate::{id, vest_state::VestState};
use bincode::serialized_size;
use chrono::prelude::{DateTime, Utc};
use num_derive::FromPrimitive;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    instruction_processor_utils::DecodeError,
    pubkey::Pubkey,
    system_instruction,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive)]
pub enum VestError {
    DestinationMissing,
    Unauthorized,
    UnexpectedProgramId,
}

impl<T> DecodeError<T> for VestError {
    fn type_of() -> &'static str {
        "VestError"
    }
}

impl std::fmt::Display for VestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                VestError::DestinationMissing => "destination missing",
                VestError::Unauthorized => "unauthorized",
                VestError::UnexpectedProgramId => "unexpected program id",
            }
        )
    }
}
impl std::error::Error for VestError {}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VestInstruction {
    /// Declare and instantiate a vesting schedule
    InitializeAccount {
        terminator_pubkey: Pubkey, // The address authorized to terminate this contract with a signed Terminate instruction
        payee_pubkey: Pubkey,      // The address authorized to redeem vested tokens
        start_dt: DateTime<Utc>,   // The day from which the vesting contract begins
        oracle_pubkey: Pubkey, // Address of an account containing a trusted date, used to drive the vesting schedule
        lamports: u64,         // The number of lamports to send the payee if the schedule completes
    },

    /// Load an account and pass its data to the contract for inspection.
    RedeemTokens,

    /// Tell the contract that the `InitializeAccount` with `Signature` has been
    /// signed by the containing transaction's `Pubkey`.
    Terminate,
}

fn initialize_account(
    terminator_pubkey: &Pubkey,
    payee_pubkey: &Pubkey,
    contract_pubkey: &Pubkey,
    start_dt: DateTime<Utc>,
    oracle_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    let keys = vec![AccountMeta::new(*contract_pubkey, false)];
    Instruction::new(
        id(),
        &VestInstruction::InitializeAccount {
            terminator_pubkey: *terminator_pubkey,
            payee_pubkey: *payee_pubkey,
            start_dt,
            oracle_pubkey: *oracle_pubkey,
            lamports,
        },
        keys,
    )
}

pub fn create_account(
    terminator_pubkey: &Pubkey,
    payee_pubkey: &Pubkey,
    contract_pubkey: &Pubkey,
    start_dt: DateTime<Utc>,
    oracle_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    let space = serialized_size(&VestState::default()).unwrap();
    vec![
        system_instruction::create_account(
            &terminator_pubkey,
            contract_pubkey,
            lamports,
            space,
            &id(),
        ),
        initialize_account(
            terminator_pubkey,
            payee_pubkey,
            contract_pubkey,
            start_dt,
            oracle_pubkey,
            lamports,
        ),
    ]
}

pub fn terminate(from: &Pubkey, contract: &Pubkey, to: &Pubkey) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*from, true),
        AccountMeta::new(*contract, false),
    ];
    if from != to {
        account_metas.push(AccountMeta::new_credit_only(*to, false));
    }
    Instruction::new(id(), &VestInstruction::Terminate, account_metas)
}

/// Apply account data to a contract waiting on an AccountData witness.
pub fn redeem_tokens(oracle_pubkey: &Pubkey, contract: &Pubkey, to: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new_credit_only(*oracle_pubkey, false),
        AccountMeta::new(*contract, false),
        AccountMeta::new_credit_only(*to, false),
    ];
    Instruction::new(id(), &VestInstruction::RedeemTokens, account_metas)
}
