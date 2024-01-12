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

pub const CLOSE_CONTEXT_STATE_COMPUTE_UNITS: u64 = 3_300;
pub const VERIFY_ZERO_BALANCE_COMPUTE_UNITS: u64 = 6_000;
pub const VERIFY_WITHDRAW_COMPUTE_UNITS: u64 = 110_000;
pub const VERIFY_CIPHERTEXT_CIPHERTEXT_EQUALITY_COMPUTE_UNITS: u64 = 8_000;
pub const VERIFY_TRANSFER_COMPUTE_UNITS: u64 = 219_000;
pub const VERIFY_TRANSFER_WITH_FEE_COMPUTE_UNITS: u64 = 407_000;
pub const VERIFY_PUBKEY_VALIDITY_COMPUTE_UNITS: u64 = 2_600;
pub const VERIFY_RANGE_PROOF_U64_COMPUTE_UNITS: u64 = 105_000;
pub const VERIFY_BATCHED_RANGE_PROOF_U64_COMPUTE_UNITS: u64 = 111_000;
pub const VERIFY_BATCHED_RANGE_PROOF_U128_COMPUTE_UNITS: u64 = 200_000;
pub const VERIFY_BATCHED_RANGE_PROOF_U256_COMPUTE_UNITS: u64 = 368_000;
pub const VERIFY_CIPHERTEXT_COMMITMENT_EQUALITY_COMPUTE_UNITS: u64 = 6_400;
pub const VERIFY_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_COMPUTE_UNITS: u64 = 6_400;
pub const VERIFY_BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_COMPUTE_UNITS: u64 = 13_000;
pub const VERIFY_FEE_SIGMA_COMPUTE_UNITS: u64 = 6_500;

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

        proof_context_account.set_data_from_slice(&context_state_data)?;
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

declare_process_instruction!(Entrypoint, 0, |invoke_context| {
    // Consume compute units if feature `native_programs_consume_cu` is activated
    let native_programs_consume_cu = invoke_context
        .feature_set
        .is_active(&feature_set::native_programs_consume_cu::id());

    let enable_zk_transfer_with_fee = invoke_context
        .feature_set
        .is_active(&feature_set::enable_zk_transfer_with_fee::id());

    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = ProofInstruction::instruction_type(instruction_data)
        .ok_or(InstructionError::InvalidInstructionData)?;

    if invoke_context.get_stack_height() != TRANSACTION_LEVEL_STACK_HEIGHT
        && instruction != ProofInstruction::CloseContextState
    {
        // Proof verification instructions are not supported as an inner instruction
        return Err(InstructionError::UnsupportedProgramId);
    }

    match instruction {
        ProofInstruction::CloseContextState => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(CLOSE_CONTEXT_STATE_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "CloseContextState");
            process_close_proof_context(invoke_context)
        }
        ProofInstruction::VerifyZeroBalance => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_ZERO_BALANCE_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyZeroBalance");
            process_verify_proof::<ZeroBalanceProofData, ZeroBalanceProofContext>(invoke_context)
        }
        ProofInstruction::VerifyWithdraw => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_WITHDRAW_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyWithdraw");
            process_verify_proof::<WithdrawData, WithdrawProofContext>(invoke_context)
        }
        ProofInstruction::VerifyCiphertextCiphertextEquality => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_CIPHERTEXT_CIPHERTEXT_EQUALITY_COMPUTE_UNITS)
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
                    .consume_checked(VERIFY_TRANSFER_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyTransfer");
            process_verify_proof::<TransferData, TransferProofContext>(invoke_context)
        }
        ProofInstruction::VerifyTransferWithFee => {
            // transfer with fee related proofs are not enabled
            if !enable_zk_transfer_with_fee {
                return Err(InstructionError::InvalidInstructionData);
            }

            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_TRANSFER_WITH_FEE_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyTransferWithFee");
            process_verify_proof::<TransferWithFeeData, TransferWithFeeProofContext>(invoke_context)
        }
        ProofInstruction::VerifyPubkeyValidity => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_PUBKEY_VALIDITY_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyPubkeyValidity");
            process_verify_proof::<PubkeyValidityData, PubkeyValidityProofContext>(invoke_context)
        }
        ProofInstruction::VerifyRangeProofU64 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_RANGE_PROOF_U64_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyRangeProof");
            process_verify_proof::<RangeProofU64Data, RangeProofContext>(invoke_context)
        }
        ProofInstruction::VerifyBatchedRangeProofU64 => {
            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_BATCHED_RANGE_PROOF_U64_COMPUTE_UNITS)
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
                    .consume_checked(VERIFY_BATCHED_RANGE_PROOF_U128_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyBatchedRangeProof128");
            process_verify_proof::<BatchedRangeProofU128Data, BatchedRangeProofContext>(
                invoke_context,
            )
        }
        ProofInstruction::VerifyBatchedRangeProofU256 => {
            // transfer with fee related proofs are not enabled
            if !enable_zk_transfer_with_fee {
                return Err(InstructionError::InvalidInstructionData);
            }

            if native_programs_consume_cu {
                invoke_context
                    .consume_checked(VERIFY_BATCHED_RANGE_PROOF_U256_COMPUTE_UNITS)
                    .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            }
            ic_msg!(invoke_context, "VerifyBatchedRangeProof256");
            process_verify_proof::<BatchedRangeProofU256Data, BatchedRangeProofContext>(
                invoke_context,
            )
        }
        ProofInstruction::VerifyCiphertextCommitmentEquality => {
            invoke_context
                .consume_checked(VERIFY_CIPHERTEXT_COMMITMENT_EQUALITY_COMPUTE_UNITS)
                .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            ic_msg!(invoke_context, "VerifyCiphertextCommitmentEquality");
            process_verify_proof::<
                CiphertextCommitmentEqualityProofData,
                CiphertextCommitmentEqualityProofContext,
            >(invoke_context)
        }
        ProofInstruction::VerifyGroupedCiphertext2HandlesValidity => {
            invoke_context
                .consume_checked(VERIFY_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_COMPUTE_UNITS)
                .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            ic_msg!(invoke_context, "VerifyGroupedCiphertext2HandlesValidity");
            process_verify_proof::<
                GroupedCiphertext2HandlesValidityProofData,
                GroupedCiphertext2HandlesValidityProofContext,
            >(invoke_context)
        }
        ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity => {
            invoke_context
                .consume_checked(VERIFY_BATCHED_GROUPED_CIPHERTEXT_2_HANDLES_VALIDITY_COMPUTE_UNITS)
                .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            ic_msg!(
                invoke_context,
                "VerifyBatchedGroupedCiphertext2HandlesValidity"
            );
            process_verify_proof::<
                BatchedGroupedCiphertext2HandlesValidityProofData,
                BatchedGroupedCiphertext2HandlesValidityProofContext,
            >(invoke_context)
        }
        ProofInstruction::VerifyFeeSigma => {
            // transfer with fee related proofs are not enabled
            if !enable_zk_transfer_with_fee {
                return Err(InstructionError::InvalidInstructionData);
            }

            invoke_context
                .consume_checked(VERIFY_FEE_SIGMA_COMPUTE_UNITS)
                .map_err(|_| InstructionError::ComputationalBudgetExceeded)?;
            ic_msg!(invoke_context, "VerifyFeeSigma");
            process_verify_proof::<FeeSigmaProofData, FeeSigmaProofContext>(invoke_context)
        }
    }
});
