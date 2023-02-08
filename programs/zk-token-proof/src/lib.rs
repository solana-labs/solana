#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
        system_program,
    },
    solana_zk_token_sdk::{
        zk_token_proof_instruction::*, zk_token_proof_program::id,
        zk_token_proof_state::ProofContextState,
    },
    std::result::Result,
};

fn process_verify_proof<T: Pod + ZkProofData>(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    // will not panic since the first two bytes are checked earlier by the caller
    let verify_proof_data = VerifyProofData::<T>::try_from_bytes(&instruction_data[1..])?;
    let proof_data = &verify_proof_data.proof_data;

    proof_data.verify_proof().map_err(|err| {
        ic_msg!(invoke_context, "proof_verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })?;

    // create context state if accounts are provided with the instruction
    if instruction_context.get_number_of_instruction_accounts() > 0 {
        let context_state_authority = {
            *instruction_context
                .try_borrow_instruction_account(transaction_context, 1)?
                .get_key()
        };

        let mut proof_context_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

        if *proof_context_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let proof_context_state =
            ProofContextState::<T::ProofContext>::try_from_bytes(proof_context_account.get_data())?;
        if proof_context_state.proof_type != ProofType::Uninitialized.into() {
            return Err(InstructionError::AccountAlreadyInitialized);
        }

        let context_state_data = ProofContextState::encode(
            verify_proof_data.proof_type.try_into()?,
            &context_state_authority,
            proof_data.context_data(),
        );

        if proof_context_account.get_data().len() != context_state_data.len() {
            return Err(InstructionError::InvalidAccountData);
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

    let owner_pubkey = {
        let owner_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 2)?;

        if !owner_account.is_signer() {
            return Err(InstructionError::MissingRequiredSignature);
        }
        *owner_account.get_key()
    };

    let proof_context_account_pubkey = *instruction_context
        .try_borrow_instruction_account(transaction_context, 0)?
        .get_key();
    let destination_account_pubkey = *instruction_context
        .try_borrow_instruction_account(transaction_context, 1)?
        .get_key();
    if proof_context_account_pubkey == destination_account_pubkey {
        return Err(InstructionError::InvalidInstructionData);
    }

    let mut proof_context_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    let proof_context_state =
        ProofContextState::<T>::try_from_bytes(proof_context_account.get_data())?;
    let expected_owner_pubkey = proof_context_state.context_state_authority;

    if owner_pubkey != expected_owner_pubkey {
        return Err(InstructionError::InvalidAccountOwner);
    }

    let mut destination_account =
        instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
    destination_account.checked_add_lamports(proof_context_account.get_lamports())?;
    proof_context_account.set_lamports(0)?;
    proof_context_account.set_data_length(0)?;
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
            _ => {
                ic_msg!(invoke_context, "unsupported proof type");
                Err(InstructionError::InvalidInstructionData)
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
            _ => {
                ic_msg!(invoke_context, "unsupported proof type");
                Err(InstructionError::InvalidInstructionData)
            }
        },
    }
}
