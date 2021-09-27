#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_sdk::{
        ic_msg, instruction::InstructionError, process_instruction::InvokeContext, pubkey::Pubkey,
    },
    spl_zk_token_crypto::instruction::*,
    std::result::Result,
};

fn verify<T: Pod + Verifiable>(
    input: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let proof = ProofInstruction::decode_data::<T>(input).ok_or_else(|| {
        ic_msg!(invoke_context, "invalid proof data");
        InstructionError::InvalidInstructionData
    })?;

    proof.verify().map_err(|err| {
        ic_msg!(invoke_context, "proof verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })
}

pub fn process_instruction(
    program_id: &Pubkey,
    input: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    match ProofInstruction::decode_type(program_id, input)
        .ok_or(InstructionError::InvalidInstructionData)?
    {
        ProofInstruction::VerifyUpdateAccountPk => {
            ic_msg!(invoke_context, "VerifyUpdateAccountPk");
            verify::<UpdateAccountPkData>(input, invoke_context)
        }
        ProofInstruction::VerifyCloseAccount => {
            ic_msg!(invoke_context, "VerifyCloseAccount");
            verify::<CloseAccountData>(input, invoke_context)
        }
        ProofInstruction::VerifyWithdraw => {
            ic_msg!(invoke_context, "VerifyWithdraw");
            verify::<WithdrawData>(input, invoke_context)
        }
        ProofInstruction::VerifyTransferRangeProofData => {
            ic_msg!(invoke_context, "VerifyTransferRangeProofData");
            verify::<TransferRangeProofData>(input, invoke_context)
        }
        ProofInstruction::VerifyTransferValidityProofData => {
            ic_msg!(invoke_context, "VerifyTransferValidityProofData");
            verify::<TransferValidityProofData>(input, invoke_context)
        }
    }
}
