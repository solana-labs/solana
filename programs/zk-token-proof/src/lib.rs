#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{
        ic_msg, invoke_context::InvokeContext, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
        program_memory, system_program,
    },
    solana_zk_token_sdk::{zk_token_proof_instruction::*, zk_token_proof_program::id},
    std::result::Result,
};

fn process_verify_proof<T: Pod + ZkProofData>(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    let verify_proof_data = VerifyProofData::<T>::try_from_bytes(&instruction_data[1..])?;

    verify_proof_data.proof_data.verify_proof().map_err(|err| {
        ic_msg!(invoke_context, "proof_verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })?;

    if let Some(context_state_authority) = verify_proof_data.create_context_state_with_authority {
        let mut proof_context_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

        if *proof_context_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let context_state_data = ProofContextState::encode(
            verify_proof_data.proof_type,
            &context_state_authority,
            verify_proof_data.proof_data.context_data(),
        );

        if proof_context_account.get_data().len() != context_state_data.len() {
            return Err(InstructionError::InvalidAccountData);
        }

        let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;
        if !rent.is_exempt(
            proof_context_account.get_lamports(),
            proof_context_account.get_data().len(),
        ) {
            return Err(InstructionError::InsufficientFunds);
        }

        proof_context_account.set_data(context_state_data)?;
    }

    Ok(())
}

fn process_close_proof_context<T: Pod + ZkProofContext>(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut proof_context_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let proof_context_state =
        ProofContextState::<T>::try_from_bytes(proof_context_account.get_data())?;

    let mut destination_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
    let owner_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 2)?;

    if !owner_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    let expected_owner_pubkey = proof_context_state.context_state_authority;
    if *owner_account.get_key() != expected_owner_pubkey {
        return Err(InstructionError::InvalidAccountOwner);
    }

    destination_account.checked_add_lamports(proof_context_account.get_lamports())?;
    proof_context_account.set_lamports(0)?;

    let account_data = proof_context_account.get_data_mut()?;
    let data_len = account_data.len();
    program_memory::sol_memset(account_data, 0, data_len);
    proof_context_account.set_owner(system_program::id().as_ref())?;

    Ok(())
}

pub fn process_instruction(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
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
    let instruction = ProofInstruction::instruction_type(instruction_data)
        .ok_or(InstructionError::InvalidInstructionData)?;
    let proof_type = ProofInstruction::proof_type(instruction_data)
        .ok_or(InstructionError::InvalidInstructionData)?;

    match instruction {
        ProofInstruction::VerifyProof => match proof_type {
            ProofType::CloseAccount => {
                ic_msg!(invoke_context, "VerifyProof CloseAccount");
                process_verify_proof::<CloseAccountData>(invoke_context)
            }
            ProofType::Withdraw => {
                ic_msg!(invoke_context, "VerifyProof Withdraw");
                process_verify_proof::<WithdrawData>(invoke_context)
            }
            ProofType::WithdrawWithheldTokens => {
                ic_msg!(invoke_context, "VerifyProof WithdrawWithheldTokens");
                process_verify_proof::<WithdrawWithheldTokensData>(invoke_context)
            }
            ProofType::Transfer => {
                ic_msg!(invoke_context, "VerifyProof Transfer");
                process_verify_proof::<TransferData>(invoke_context)
            }
            ProofType::TransferWithFee => {
                ic_msg!(invoke_context, "VerifyProof TransferWithFee");
                process_verify_proof::<TransferWithFeeData>(invoke_context)
            }
            ProofType::PubkeyValidity => {
                ic_msg!(invoke_context, "VerifyProof PubkeyValidity");
                process_verify_proof::<PubkeyValidityData>(invoke_context)
            }
        },
        ProofInstruction::CloseContextState => match proof_type {
            ProofType::CloseAccount => {
                ic_msg!(invoke_context, "CloseContextState CloseAccount");
                process_close_proof_context::<CloseAccountProofContext>(invoke_context)
            }
            ProofType::Withdraw => {
                ic_msg!(invoke_context, "CloseContextState Withdraw");
                process_close_proof_context::<WithdrawProofContext>(invoke_context)
            }
            ProofType::WithdrawWithheldTokens => {
                ic_msg!(invoke_context, "CloseContextState WithdrawWithheldTokens");
                process_close_proof_context::<WithdrawWithheldTokensProofContext>(invoke_context)
            }
            ProofType::Transfer => {
                ic_msg!(invoke_context, "CloseContextState Transfer");
                process_close_proof_context::<TransferProofContext>(invoke_context)
            }
            ProofType::TransferWithFee => {
                ic_msg!(invoke_context, "CloseContextState TransferWithFee");
                process_close_proof_context::<TransferWithFeeProofContext>(invoke_context)
            }
            ProofType::PubkeyValidity => {
                ic_msg!(invoke_context, "CloseContextState PubkeyValidity");
                process_close_proof_context::<PubkeyValidityProofContext>(invoke_context)
            }
        },
    }
}
