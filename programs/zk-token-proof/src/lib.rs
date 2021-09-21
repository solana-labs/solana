#![forbid(unsafe_code)]

use {
    solana_sdk::{
        ic_msg, instruction::InstructionError, process_instruction::InvokeContext, pubkey::Pubkey,
    },
    spl_zk_token_crypto::instruction::*,
    std::result::Result,
};

pub fn process_instruction(
    program_id: &Pubkey,
    input: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    match ProofInstruction::decode_type(program_id, input)
        .ok_or(InstructionError::InvalidInstructionData)?
    {
        ProofInstruction::VerifyUpdateAccountPkData => {
            ic_msg!(invoke_context, "VerifyUpdateAccountPkData");
            let proof = ProofInstruction::decode_data::<UpdateAccountPkData>(input)
                .ok_or(InstructionError::InvalidInstructionData)?;

            proof.verify().map_err(|err| {
                ic_msg!(invoke_context, "proof verification failed: {:?}", err);
                InstructionError::InvalidInstructionData
            })
        }
    }
}
