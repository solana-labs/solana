#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
        transaction_context::IndexOfAccount,
    },
    solana_zk_token_sdk::zk_token_proof_instruction::*,
    std::result::Result,
};

fn verify<T: Pod + Verifiable>(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = ProofInstruction::decode_data::<T>(instruction_data);

    let proof = instruction.ok_or_else(|| {
        ic_msg!(invoke_context, "invalid proof data");
        InstructionError::InvalidInstructionData
    })?;

    proof.verify().map_err(|err| {
        ic_msg!(invoke_context, "proof verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })
}

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    if invoke_context.get_stack_height() != TRANSACTION_LEVEL_STACK_HEIGHT {
        // Not supported as an inner instruction
        return Err(InstructionError::UnsupportedProgramId);
    }

    // Consume compute units since proof verification is an expensive operation
    {
        // TODO: Tune the number of units consumed.  The current value is just a rough estimate
        invoke_context.consume_checked(100_000)?;
    }

    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = ProofInstruction::decode_type(instruction_data);

    match instruction.ok_or(InstructionError::InvalidInstructionData)? {
        ProofInstruction::VerifyCloseAccount => {
            ic_msg!(invoke_context, "VerifyCloseAccount");
            verify::<CloseAccountData>(invoke_context)
        }
        ProofInstruction::VerifyWithdraw => {
            ic_msg!(invoke_context, "VerifyWithdraw");
            verify::<WithdrawData>(invoke_context)
        }
        ProofInstruction::VerifyWithdrawWithheldTokens => {
            ic_msg!(invoke_context, "VerifyWithdrawWithheldTokens");
            verify::<WithdrawWithheldTokensData>(invoke_context)
        }
        ProofInstruction::VerifyTransfer => {
            ic_msg!(invoke_context, "VerifyTransfer");
            verify::<TransferData>(invoke_context)
        }
        ProofInstruction::VerifyTransferWithFee => {
            ic_msg!(invoke_context, "VerifyTransferWithFee");
            verify::<TransferWithFeeData>(invoke_context)
        }
        ProofInstruction::VerifyPubkeyValidity => {
            ic_msg!(invoke_context, "VerifyPubkeyValidity");
            verify::<PubkeyValidityData>(invoke_context)
        }
    }
}
