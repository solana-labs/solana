#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{declare_process_instruction, ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        feature_set,
        instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
        system_program,
    },
    solana_zk_token_sdk::{
        zk_token_proof_instruction::*,
        zk_token_proof_program::id,
        zk_token_proof_state::{ProofContextState, ProofContextStateMeta},
    },
    std::result::Result,
};

fn process_verify_proof<T, U>(invoke_context: &mut InvokeContext) -> Result<(), InstructionError>
where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let proof_data = ProofInstruction::proof_data::<T, U>(instruction_data).ok_or_else(|| {
        ic_msg!(invoke_context, "invalid proof data");
        InstructionError::InvalidInstructionData
    })?;

    proof_data.verify_proof().map_err(|err| {
        ic_msg!(invoke_context, "proof_verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })?;

    // create context state if accounts are provided with the instruction
    if instruction_context.get_number_of_instruction_accounts() > 0 {
        let context_state_authority = *instruction_context
            .try_borrow_instruction_account(transaction_context, 1)?
            .get_key();

        let mut proof_context_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

        if *proof_context_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let proof_context_state_meta =
            ProofContextStateMeta::try_from_bytes(proof_context_account.get_data())?;

        if proof_context_state_meta.proof_type != ProofType::Uninitialized.into() {
            return Err(InstructionError::AccountAlreadyInitialized);
        }

        let context_state_data = ProofContextState::encode(
            &context_state_authority,
            T::PROOF_TYPE,
            proof_data.context_data(),
        );

        if proof_context_account.get_data().len() != context_state_data.len() {
            return Err(InstructionError::InvalidAccountData);
        }

        proof_context_account.set_data(context_state_data)?;
    }

    Ok(())
}

fn process_close_proof_context(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    let owner_pubkey = {
        let owner_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 2)?;

        if !owner_account.is_signer() {
            return Err(InstructionError::MissingRequiredSignature);
        }
        *owner_account.get_key()
    }; // done with `owner_account`, so drop it to prevent a potential double borrow

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
    let proof_context_state_meta =
        ProofContextStateMeta::try_from_bytes(proof_context_account.get_data())?;
    let expected_owner_pubkey = proof_context_state_meta.context_state_authority;

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

declare_process_instruction!(process_instruction, 0, |invoke_context| {
    if invoke_context.get_stack_height() != TRANSACTION_LEVEL_STACK_HEIGHT {
        // Not supported as an inner instruction
        return Err(InstructionError::UnsupportedProgramId);
    }

    // Consume compute units if feature `native_programs_consume_cu` is activated
    let native_programs_consume_cu = invoke_context
        .feature_set
        .is_active(&feature_set::native_programs_consume_cu::id());
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = ProofInstruction::instruction_type(instruction_data)
        .ok_or(InstructionError::InvalidInstructionData)?;

    match instruction {
        ProofInstruction::CloseContextState => {
            ic_msg!(invoke_context, "CloseContextState");
            process_close_proof_context(invoke_context)
        }
        ProofInstruction::VerifyZeroBalance => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(6_012)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyZeroBalance");
            process_verify_proof::<ZeroBalanceProofData, ZeroBalanceProofContext>(invoke_context)
        }
        ProofInstruction::VerifyWithdraw => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(112_454)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyWithdraw");
            process_verify_proof::<WithdrawData, WithdrawProofContext>(invoke_context)
        }
        ProofInstruction::VerifyCiphertextCiphertextEquality => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(7_943)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyCiphertextCiphertextEquality");
            process_verify_proof::<
                CiphertextCiphertextEqualityProofData,
                CiphertextCiphertextEqualityProofContext,
            >(invoke_context)
        }
        ProofInstruction::VerifyTransfer => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(219_290)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyTransfer");
            process_verify_proof::<TransferData, TransferProofContext>(invoke_context)
        }
        ProofInstruction::VerifyTransferWithFee => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(407_121)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyTransferWithFee");
            process_verify_proof::<TransferWithFeeData, TransferWithFeeProofContext>(invoke_context)
        }
        ProofInstruction::VerifyPubkeyValidity => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(2_619)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyPubkeyValidity");
            process_verify_proof::<PubkeyValidityData, PubkeyValidityProofContext>(invoke_context)
        }
        ProofInstruction::VerifyRangeProofU64 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(105_066)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyRangeProof");
            process_verify_proof::<RangeProofU64Data, RangeProofContext>(invoke_context)
        }
        ProofInstruction::VerifyBatchedRangeProofU64 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(111_478)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyBatchedRangeProof64");
            process_verify_proof::<BatchedRangeProofU64Data, BatchedRangeProofContext>(
                invoke_context,
            )
        }
        ProofInstruction::VerifyBatchedRangeProofU128 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(204_512)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyBatchedRangeProof128");
            process_verify_proof::<BatchedRangeProofU128Data, BatchedRangeProofContext>(
                invoke_context,
            )
        }
        ProofInstruction::VerifyBatchedRangeProofU256 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(368_000)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyBatchedRangeProof256");
            process_verify_proof::<BatchedRangeProofU256Data, BatchedRangeProofContext>(
                invoke_context,
            )
        }
    }
});
