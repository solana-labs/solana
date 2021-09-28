///! Instructions provided by the ZkToken Proof program
pub use spl_zk_token_crypto::instruction::*;

use {
    bytemuck::Pod,
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{instruction::Instruction, pubkey::Pubkey},
    spl_zk_token_crypto::pod::*,
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

    /// Verify a `TransferRangeProofData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferRangeProofData`
    ///
    VerifyTransferRangeProofData,

    /// Verify a `TransferValidityProofData` struct
    ///
    /// Accounts expected by this instruction:
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferValidityProofData`
    ///
    VerifyTransferValidityProofData,
}

impl ProofInstruction {
    pub fn encode<T: Pod>(&self, proof: &T) -> Instruction {
        let mut data = vec![ToPrimitive::to_u8(self).unwrap()];
        data.extend_from_slice(pod_bytes_of(proof));
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
            pod_from_bytes(&input[1..])
        }
    }
}
