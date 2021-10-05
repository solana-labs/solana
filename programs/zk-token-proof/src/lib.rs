#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_sdk::{
        ic_msg, instruction::InstructionError, process_instruction::InvokeContext, pubkey::Pubkey,
    },
    spl_zk_token_sdk::zk_token_proof_instruction::*,
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
    if invoke_context.invoke_depth() != 1 {
        // Not supported as an inner instruction
        return Err(InstructionError::UnsupportedProgramId);
    }

    // Consume compute units since proof verification is an expensive operation
    {
        let compute_meter = invoke_context.get_compute_meter();
        compute_meter.borrow_mut().consume(25_000)?; // TODO: Tune the number of units consumed?
    }

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
        ProofInstruction::VerifyTransfer => {
            ic_msg!(invoke_context, "VerifyTransfer");
            verify::<TransferData>(input, invoke_context)
        }
    }
}
