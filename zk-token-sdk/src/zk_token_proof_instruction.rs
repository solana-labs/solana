///! Instructions provided by the ZkToken Proof program
pub use crate::instruction::*;

use {
    bytemuck::{bytes_of, Pod},
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{instruction::Instruction, pubkey::Pubkey},
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Verify a `UpdateAccountPkData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `UpdateAccountPkData`
    ///
    VerifyUpdateAccountPk,

    /// Verify a `CloseAccountData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `CloseAccountData`
    ///
    VerifyCloseAccount,

    /// Verify a `WithdrawData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `WithdrawData`
    ///
    VerifyWithdraw,

    /// Verify a `TransferData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferData`
    ///
    VerifyTransfer,
}

impl ProofInstruction {
    pub fn encode<T: Pod>(&self, proof: &T) -> Instruction {
        let mut data = vec![ToPrimitive::to_u8(self).unwrap()];
        data.extend_from_slice(bytes_of(proof));
        Instruction {
            program_id: crate::zk_token_proof_program::id(),
            accounts: vec![],
            data,
        }
    }

    pub fn decode_type(program_id: &Pubkey, input: &[u8]) -> Option<Self> {
        if *program_id != crate::zk_token_proof_program::id() || input.is_empty() {
            None
        } else {
            FromPrimitive::from_u8(input[0])
        }
    }

    pub fn decode_data<T: Pod>(input: &[u8]) -> Option<&T> {
        if input.is_empty() {
            None
        } else {
            bytemuck::try_from_bytes(&input[1..]).ok()
        }
    }
}

pub fn verify_close_account(proof_data: &CloseAccountData) -> Instruction {
    ProofInstruction::VerifyCloseAccount.encode(proof_data)
}

pub fn verify_update_account_pk(proof_data: &UpdateAccountPkData) -> Instruction {
    ProofInstruction::VerifyUpdateAccountPk.encode(proof_data)
}

pub fn verify_withdraw(proof_data: &WithdrawData) -> Instruction {
    ProofInstruction::VerifyWithdraw.encode(proof_data)
}

pub fn verify_transfer(proof_data: &TransferData) -> Instruction {
    ProofInstruction::VerifyTransfer.encode(proof_data)
}
