///! Instructions provided by the ZkToken Proof program
pub use crate::instruction::*;
use {
    bytemuck::{bytes_of, Pod},
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        sysvar,
    },
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Verify a `CloseAccountData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `CloseAccountData`
    ///
    VerifyCloseAccount,

    /// Verify a `WithdrawData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `WithdrawData`
    ///
    VerifyWithdraw,

    /// Verify a `WithdrawWithheldTokensData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `WithdrawWithheldTokensData`
    ///
    VerifyWithdrawWithheldTokens,

    /// Verify a `TransferData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferData`
    ///
    VerifyTransfer,

    /// Verify a `TransferWithFeeData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferWithFeeData`
    ///
    VerifyTransferWithFee,

    /// Verify a `PubkeyValidityData` struct
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[WRITE]` Uninitialized
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `PubkeyValidityData`
    ///
    VerifyPubkeyValidity,
}

impl ProofInstruction {
    pub fn encode<T: Pod + ZkProofData>(
        &self,
        proof_data: &T,
        context_account: Option<&Pubkey>,
    ) -> Instruction {
        let accounts = if let Some(context_account) = context_account {
            vec![
                AccountMeta::new(*context_account, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
            ]
        } else {
            vec![]
        };

        let mut data = vec![ToPrimitive::to_u8(self).unwrap()];
        data.extend_from_slice(bytes_of(proof_data));

        Instruction {
            program_id: crate::zk_token_proof_program::id(),
            accounts,
            data,
        }
    }

    pub fn decode_type(input: &[u8]) -> Option<Self> {
        input.first().and_then(|x| FromPrimitive::from_u8(*x))
    }

    pub fn decode_data<T: Pod>(input: &[u8]) -> Option<&T> {
        if input.is_empty() {
            None
        } else {
            bytemuck::try_from_bytes(&input[1..]).ok()
        }
    }
}

pub fn verify_close_account(
    proof_data: &CloseAccountData,
    context_account: Option<&Pubkey>,
) -> Instruction {
    ProofInstruction::VerifyCloseAccount.encode(proof_data, context_account)
}

pub fn verify_withdraw(proof_data: &WithdrawData, context_account: Option<&Pubkey>) -> Instruction {
    ProofInstruction::VerifyWithdraw.encode(proof_data, context_account)
}

pub fn verify_withdraw_withheld_tokens(
    proof_data: &WithdrawWithheldTokensData,
    context_account: Option<&Pubkey>,
) -> Instruction {
    ProofInstruction::VerifyWithdrawWithheldTokens.encode(proof_data, context_account)
}

pub fn verify_transfer(proof_data: &TransferData, context_account: Option<&Pubkey>) -> Instruction {
    ProofInstruction::VerifyTransfer.encode(proof_data, context_account)
}

pub fn verify_transfer_with_fee(
    proof_data: &TransferWithFeeData,
    context_account: Option<&Pubkey>,
) -> Instruction {
    ProofInstruction::VerifyTransferWithFee.encode(proof_data, context_account)
}

pub fn verify_pubkey_validity(
    proof_data: &PubkeyValidityData,
    context_account: Option<&Pubkey>,
) -> Instruction {
    ProofInstruction::VerifyPubkeyValidity.encode(proof_data, context_account)
}
