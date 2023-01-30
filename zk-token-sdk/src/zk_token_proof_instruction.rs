///! Instructions provided by the ZkToken Proof program
pub use crate::instruction::*;
use {
    bytemuck::{bytes_of, Pod},
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{
        instruction::{
            AccountMeta, Instruction,
            {
                InstructionError,
                InstructionError::{InvalidAccountData, InvalidInstructionData},
            },
        },
        pubkey::{Pubkey, PUBKEY_BYTES},
        sysvar,
    },
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofType {
    CloseAccount,
    Withdraw,
    WithdrawWithheldTokens,
    Transfer,
    TransferWithFee,
    PubkeyValidity,
}

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Verify a zero-knowledge proof data
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` Rent sysvar
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `VerifyProofData`
    ///
    VerifyProof,

    /// Close a zero-knowledge proof context state
    ///
    /// Accounts expected by this instruction:
    ///   0. `[writable]` The proof context account to close
    ///   1. `[writable]` The destination account for lamports
    ///   2. `[signer]` The context account's owner
    ///
    /// Data expected by this instruction:
    ///   `ProofType`
    ///
    CloseContextState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct VerifyProofData<T: Pod + ZkProofData> {
    /// The zero-knowledge proof type
    pub proof_type: ProofType,
    /// If `Some`, initialize a proof context state with an authority pubkey
    pub create_context_state_with_authority: Option<Pubkey>,
    /// The proof data that contains the zero-knowledge proof and its context
    pub proof_data: T,
}

impl<T: Pod + ZkProofData> VerifyProofData<T> {
    // A `VerifyProofData::encode(&self)` syntax could require the caller to make unnecessary
    // copy of `T: ZkProofData`, which can be quite large. This syntax takes in references to the
    // individual `VerifyProofData` components to provide flexibility to the caller.
    pub fn encode(
        proof_type: ProofType,
        create_context_state_with_authority: Option<&Pubkey>,
        proof_data: &T,
    ) -> Vec<u8> {
        let mut buf = vec![ToPrimitive::to_u8(&proof_type).unwrap()];
        if let Some(authority_pubkey) = create_context_state_with_authority {
            buf.push(1);
            buf.extend_from_slice(&authority_pubkey.to_bytes())
        } else {
            buf.push(0);
        }
        buf.extend_from_slice(bytes_of(proof_data));
        buf
    }

    pub fn try_from_bytes(input: &[u8]) -> Result<Self, InstructionError> {
        let (proof_type, rest) = decode_proof_type(input)?;
        let (create_context_state_with_authority, proof_data) = decode_optional_pubkey(rest)?;
        let proof_data =
            bytemuck::try_from_bytes::<T>(proof_data).map_err(|_| InvalidInstructionData)?;

        Ok(Self {
            proof_type,
            create_context_state_with_authority,
            proof_data: *proof_data,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ProofContextState<T: Pod + ZkProofContext> {
    /// The zero-knowledge proof type
    pub proof_type: ProofType,
    /// The proof context authority that can close the account
    pub context_state_authority: Pubkey,
    /// The proof context data
    pub proof_context: T,
}

impl<T: Pod + ZkProofContext> ProofContextState<T> {
    pub fn encode(
        proof_type: ProofType,
        context_state_authority: &Pubkey,
        proof_context: &T,
    ) -> Vec<u8> {
        let mut buf = vec![ToPrimitive::to_u8(&proof_type).unwrap()];
        buf.extend_from_slice(&context_state_authority.to_bytes());
        buf.extend_from_slice(bytes_of(proof_context));
        buf
    }

    pub fn try_from_bytes(input: &[u8]) -> Result<Self, InstructionError> {
        let (proof_type, rest) = decode_proof_type(input)?;
        let (context_state_authority, proof_context) = decode_pubkey(rest)?;
        let proof_context =
            bytemuck::try_from_bytes::<T>(proof_context).map_err(|_| InvalidAccountData)?;

        Ok(Self {
            proof_type,
            context_state_authority,
            proof_context: *proof_context,
        })
    }

    pub fn size() -> usize {
        T::LEN.saturating_add(1).saturating_add(PUBKEY_BYTES)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextStateInfo<'a> {
    pub context_state_account: &'a Pubkey,
    pub context_state_authority: &'a Pubkey,
}

pub fn verify_proof<T: Pod + ZkProofData>(
    proof_type: ProofType,
    context_state_info: Option<ContextStateInfo>,
    proof_data: &T,
) -> Instruction {
    let accounts = if let Some(context_state_account) =
        context_state_info.map(|info| info.context_state_account)
    {
        vec![
            AccountMeta::new(*context_state_account, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ]
    } else {
        vec![]
    };

    let create_context_state_with_authority =
        context_state_info.map(|info| info.context_state_authority);
    let data = [
        vec![ToPrimitive::to_u8(&ProofInstruction::VerifyProof).unwrap()],
        VerifyProofData::encode(proof_type, create_context_state_with_authority, proof_data),
    ]
    .concat();

    Instruction {
        program_id: crate::zk_token_proof_program::id(),
        accounts,
        data,
    }
}

pub fn close_context_state(
    context_account: &Pubkey,
    destination_account: &Pubkey,
    account_owner: &Pubkey,
    proof_type: ProofType,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(*context_account, false),
        AccountMeta::new(*destination_account, false),
        AccountMeta::new_readonly(*account_owner, true),
    ];

    let data = vec![
        ToPrimitive::to_u8(&ProofInstruction::CloseContextState).unwrap(),
        ToPrimitive::to_u8(&proof_type).unwrap(),
    ];

    Instruction {
        program_id: crate::zk_token_proof_program::id(),
        accounts,
        data,
    }
}

impl ProofInstruction {
    pub fn instruction_type(input: &[u8]) -> Option<Self> {
        input
            .first()
            .and_then(|instruction| FromPrimitive::from_u8(*instruction))
    }

    pub fn proof_type(input: &[u8]) -> Option<ProofType> {
        input
            .get(1)
            .and_then(|proof_type| FromPrimitive::from_u8(*proof_type))
    }
}

pub fn verify_close_account(proof_data: &CloseAccountData) -> Instruction {
    verify_proof(ProofType::CloseAccount, None, proof_data)
}

pub fn verify_withdraw(proof_data: &WithdrawData) -> Instruction {
    verify_proof(ProofType::Withdraw, None, proof_data)
}

pub fn verify_withdraw_withheld_tokens(proof_data: &WithdrawWithheldTokensData) -> Instruction {
    verify_proof(ProofType::WithdrawWithheldTokens, None, proof_data)
}

pub fn verify_transfer(proof_data: &TransferData) -> Instruction {
    verify_proof(ProofType::Transfer, None, proof_data)
}

pub fn verify_transfer_with_fee(proof_data: &TransferWithFeeData) -> Instruction {
    verify_proof(ProofType::TransferWithFee, None, proof_data)
}

pub fn verify_pubkey_validity(proof_data: &PubkeyValidityData) -> Instruction {
    verify_proof(ProofType::PubkeyValidity, None, proof_data)
}

fn decode_proof_type(input: &[u8]) -> Result<(ProofType, &[u8]), InstructionError> {
    let proof_type = input
        .first()
        .and_then(|b| FromPrimitive::from_u8(*b))
        .ok_or(InvalidInstructionData)?;
    Ok((proof_type, &input[1..]))
}

fn decode_pubkey(input: &[u8]) -> Result<(Pubkey, &[u8]), InstructionError> {
    let pubkey = input
        .get(..PUBKEY_BYTES)
        .and_then(|pubkey| Pubkey::try_from(pubkey).ok())
        .ok_or(InvalidInstructionData)?;
    Ok((pubkey, &input[PUBKEY_BYTES..]))
}

fn decode_optional_pubkey(input: &[u8]) -> Result<(Option<Pubkey>, &[u8]), InstructionError> {
    let create_context_state = input
        .first()
        .map(|b| *b == 1)
        .ok_or(InvalidInstructionData)?;
    if create_context_state {
        let pubkey_bytes = input
            .get(1..PUBKEY_BYTES + 1)
            .ok_or(InvalidInstructionData)?;
        Ok((
            Pubkey::try_from(pubkey_bytes).ok(),
            &input[PUBKEY_BYTES + 1..],
        ))
    } else {
        Ok((None, &input[1..]))
    }
}
