#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
    solana_zk_token_sdk::zk_token_proof_instruction::*,
    std::result::Result,
};

fn verify<T: Pod + Verifiable>(
    input: &[u8],
    invoke_context: &mut InvokeContext,
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
    _first_instruction_account: usize,
    input: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    if invoke_context.get_stack_height() != TRANSACTION_LEVEL_STACK_HEIGHT {
        // Not supported as an inner instruction
        return Err(InstructionError::UnsupportedProgramId);
    }

    // Consume compute units since proof verification is an expensive operation
    {
        let compute_meter = invoke_context.get_compute_meter();
        // TODO: Tune the number of units consumed.  The current value is just a rough estimate
        compute_meter.borrow_mut().consume(100_000)?;
    }

    match ProofInstruction::decode_type(input).ok_or(InstructionError::InvalidInstructionData)? {
        ProofInstruction::VerifyCloseAccount => {
            ic_msg!(invoke_context, "VerifyCloseAccount");
            verify::<CloseAccountData>(input, invoke_context)
        }
        ProofInstruction::VerifyWithdraw => {
            ic_msg!(invoke_context, "VerifyWithdraw");
            verify::<WithdrawData>(input, invoke_context)
        }
        ProofInstruction::VerifyTransfer => {
            ic_msg!(invoke_context, "VerifyTransfer");
            verify::<TransferData>(input, invoke_context)
        }
        ProofInstruction::VerifyTransferWithFee => {
            ic_msg!(invoke_context, "VerifyTransferWithFee");
            verify::<TransferWithFeeData>(input, invoke_context)
        }
    }
}
