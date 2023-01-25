#![forbid(unsafe_code)]

use {
    bytemuck::Pod,
    solana_program_runtime::{
        ic_msg, invoke_context::InvokeContext, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::instruction::{InstructionError, TRANSACTION_LEVEL_STACK_HEIGHT},
    solana_zk_token_sdk::{zk_token_proof_instruction::*, zk_token_proof_program::id},
    std::result::Result,
};

fn verify<T: Pod + ZkProofData>(
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    let instruction = ProofInstruction::decode_data::<T>(instruction_data);

    let proof_data = instruction.ok_or_else(|| {
        ic_msg!(invoke_context, "invalid proof data");
        InstructionError::InvalidInstructionData
    })?;

    proof_data.verify_proof().map_err(|err| {
        ic_msg!(invoke_context, "proof verification failed: {:?}", err);
        InstructionError::InvalidInstructionData
    })?;

    if proof_data.create_context_state() {
        let mut proof_context_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if *proof_context_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        if proof_context_account.get_data().len() != proof_data.context_data().len() {
            return Err(InstructionError::InvalidAccountData);
        }

        let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;

        if !rent.is_exempt(
            proof_context_account.get_lamports(),
            proof_context_account.get_data().len(),
        ) {
            return Err(InstructionError::InsufficientFunds);
        }

        proof_context_account.set_data_from_slice(proof_data.context_data())?;
    }

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

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_sdk::{
            account::{self, AccountSharedData},
            instruction::AccountMeta,
            pubkey::Pubkey,
            rent::Rent,
            sysvar,
        },
        solana_zk_token_sdk::{encryption::elgamal::ElGamalKeypair, instruction::CloseAccountData},
    };

    fn process_instruction(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            None,
            None,
            expected_result,
            super::process_instruction,
        )
    }

    fn create_default_rent_account() -> AccountSharedData {
        account::create_account_shared_data_for_test(&Rent::default())
    }

    #[test]
    fn test_close_account() {
        // success case: do not create proof context
        let elgamal_keypair = ElGamalKeypair::new_rand();
        let ciphertext = elgamal_keypair.public.encrypt(0_u64);
        let close_account_data =
            CloseAccountData::new(&elgamal_keypair, &ciphertext, false).unwrap(); // no context

        let instruction_data = ProofInstruction::VerifyCloseAccount
            .encode(&close_account_data, None)
            .data;

        process_instruction(&instruction_data, vec![], vec![], Ok(()));

        // success case: create proof context
        let close_account_data =
            CloseAccountData::new(&elgamal_keypair, &ciphertext, true).unwrap(); // create context
        let proof_context_length = close_account_data.context_data().len();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(proof_context_length);
        let proof_context_address = solana_sdk::pubkey::new_rand();
        let proof_context_account =
            AccountSharedData::new(rent_exempt_reserve, proof_context_length, &id());

        let instruction_accounts = vec![
            AccountMeta {
                pubkey: proof_context_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: sysvar::rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        let instruction_data = ProofInstruction::VerifyCloseAccount
            .encode(&close_account_data, Some(&proof_context_address))
            .data;

        process_instruction(
            &instruction_data,
            vec![
                (proof_context_address, proof_context_account.clone()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            instruction_accounts.clone(),
            Ok(()),
        );

        // create proof context, but do not provide account info
        process_instruction(
            &instruction_data,
            vec![],
            vec![],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // insufficient rent
        let proof_context_account =
            AccountSharedData::new(rent_exempt_reserve - 1, proof_context_length, &id());

        process_instruction(
            &instruction_data,
            vec![
                (proof_context_address, proof_context_account.clone()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // invalid proof
        let ciphertext = elgamal_keypair.public.encrypt(1_u64); // non-zero amount
        let close_account_data =
            CloseAccountData::new(&elgamal_keypair, &ciphertext, true).unwrap();
        let instruction_data = ProofInstruction::VerifyCloseAccount
            .encode(&close_account_data, Some(&proof_context_address))
            .data;

        process_instruction(
            &instruction_data,
            vec![
                (proof_context_address, proof_context_account.clone()),
                (sysvar::rent::id(), create_default_rent_account()),
            ],
            instruction_accounts.clone(),
            Err(InstructionError::InvalidInstructionData),
        );
    }
}
